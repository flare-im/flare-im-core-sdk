//! 注意力优先级调度器（核心策略）：收敛顺序跟随用户注意力从"眼睛所在处"向外扩散。
//! P0 前台会话 > P1 可见列表窗口 > P2 近期活跃 > P3 长尾/维护——取代扁平轮询 + 周期前台轮询。
//!
//! 本模块是**领域无关**的调度策略 + 有界优先队列（同步、可单测）。客户端只上报视口（[AttentionState]），
//! **打分归 core**。异步 drain worker（真正调 [super::SyncDomain] 的 pull/apply）在全局流 B 落地喂数据后接上。

use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use super::domain::ConvergencePriority;

/// 收敛目标：调度最小单元指向的对象。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ConvergenceTarget {
    /// 单会话收敛（消息/已读等）。
    Conversation(String),
    /// 全局收敛（会话列表增量、跨会话关键事件等）。
    Global,
}

/// 注意力状态：客户端上报的视口。core 据此打分，UI 不决定优先级。
#[derive(Clone, Debug, Default)]
pub struct AttentionState {
    /// 前台打开的会话（timeline open）。
    foreground: Option<String>,
    /// 可见会话列表窗口（列表视口区间内的会话）。
    visible: HashSet<String>,
    /// 近期活跃会话（按 last_message_ts 由业务侧喂入）。
    recent: HashSet<String>,
    /// I9：应用是否处于后台/锁屏——后台态收敛降配（批/页减半，让电让带宽）。
    app_in_background: bool,
}

impl AttentionState {
    /// 打开会话时间线 → 该会话升为前台。
    pub fn open_timeline(&mut self, conversation_id: impl Into<String>) {
        self.foreground = Some(conversation_id.into());
    }

    /// 关闭会话时间线 → 若关的是当前前台则清空。
    pub fn close_timeline(&mut self, conversation_id: &str) {
        if self.foreground.as_deref() == Some(conversation_id) {
            self.foreground = None;
        }
    }

    /// 设置可见列表窗口（滑动时整窗替换；滑走即退出可见域）。
    pub fn set_visible<I, S>(&mut self, conversation_ids: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.visible = conversation_ids.into_iter().map(Into::into).collect();
    }

    /// 标记近期活跃（业务侧按 last_message_ts 喂入）。
    pub fn mark_recent(&mut self, conversation_id: impl Into<String>) {
        self.recent.insert(conversation_id.into());
    }

    /// I9：更新应用前后台状态（来自平台生命周期回调）。
    pub fn set_app_in_background(&mut self, in_background: bool) {
        self.app_in_background = in_background;
    }

    pub fn app_in_background(&self) -> bool {
        self.app_in_background
    }

    /// 注意力驱动打分：前台 P0 > 可见 P1 > 近期 P2 > 其余/全局 P3。
    pub fn priority_for(&self, target: &ConvergenceTarget) -> ConvergencePriority {
        match target {
            ConvergenceTarget::Conversation(id) => self.priority_for_conversation(id),
            // 全局收敛是环境层，让位于任何可见会话。
            ConvergenceTarget::Global => ConvergencePriority::P3Ambient,
        }
    }

    /// 按会话 id 打分（免 `ConvergenceTarget` 包装/克隆——热路径批间重排用）。
    pub fn priority_for_conversation(&self, conversation_id: &str) -> ConvergencePriority {
        if self.foreground.as_deref() == Some(conversation_id) {
            ConvergencePriority::P0Foreground
        } else if self.visible.contains(conversation_id) {
            ConvergencePriority::P1VisibleWindow
        } else if self.recent.contains(conversation_id) {
            ConvergencePriority::P2Recent
        } else {
            ConvergencePriority::P3Ambient
        }
    }
}

/// 线程安全的共享注意力：引擎持有一份，客户端经视口 API 更新，收敛任务快照读取。
/// `Clone` 为浅拷贝（共享同一 `Arc`），随处传递不复制状态。
#[derive(Clone, Default)]
pub struct AttentionRegistry {
    inner: Arc<RwLock<AttentionState>>,
}

impl AttentionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, AttentionState> {
        self.inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 打开会话时间线（前台）。
    pub fn open_timeline(&self, conversation_id: impl Into<String>) {
        self.write().open_timeline(conversation_id);
    }

    /// 关闭会话时间线。
    pub fn close_timeline(&self, conversation_id: &str) {
        self.write().close_timeline(conversation_id);
    }

    /// 设置可见会话列表窗口。
    pub fn set_visible<I, S>(&self, conversation_ids: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.write().set_visible(conversation_ids);
    }

    /// 标记近期活跃会话。
    pub fn mark_recent(&self, conversation_id: impl Into<String>) {
        self.write().mark_recent(conversation_id);
    }

    /// I9：更新应用前后台状态（后台收敛降配）。
    pub fn set_app_in_background(&self, in_background: bool) {
        self.write().set_app_in_background(in_background);
    }

    /// 取当前注意力快照（用于播种一次收敛的调度器）。
    pub fn snapshot(&self) -> AttentionState {
        self.inner
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }
}

/// 有界注意力优先级调度器：去重的待收敛目标集 + 按当前注意力弹出最高优先项 + 背压。
pub struct ConvergenceScheduler {
    attention: AttentionState,
    pending: HashSet<ConvergenceTarget>,
    max_pending: usize,
}

impl ConvergenceScheduler {
    /// `max_pending` 为待收敛目标上限（背压边界，防止长尾无界堆积）。
    pub fn new(max_pending: usize) -> Self {
        Self {
            attention: AttentionState::default(),
            pending: HashSet::new(),
            max_pending: max_pending.max(1),
        }
    }

    /// 用注意力快照播种（引擎共享注意力 → 本次收敛的优先级）。
    pub fn with_attention(max_pending: usize, attention: AttentionState) -> Self {
        Self {
            attention,
            pending: HashSet::new(),
            max_pending: max_pending.max(1),
        }
    }

    pub fn attention_mut(&mut self) -> &mut AttentionState {
        &mut self.attention
    }

    pub fn attention(&self) -> &AttentionState {
        &self.attention
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// 入队一个待收敛目标（去重）。满时按背压规则：仅当新目标优先级严格高于当前最差项时，
    /// 逐出最差项接纳新目标；否则拒绝（返回 false）——高优先永远挤掉低优先，绝不无界堆积。
    pub fn enqueue(&mut self, target: ConvergenceTarget) -> bool {
        if self.pending.contains(&target) {
            return true;
        }
        if self.pending.len() < self.max_pending {
            self.pending.insert(target);
            return true;
        }
        let new_priority = self.attention.priority_for(&target);
        let Some((worst, worst_priority)) = self.worst_pending() else {
            self.pending.insert(target);
            return true;
        };
        // Ord: P0 < P3，值越小优先级越高。new 优先级更高 = new_priority < worst_priority。
        if new_priority < worst_priority {
            self.pending.remove(&worst);
            self.pending.insert(target);
            true
        } else {
            false
        }
    }

    /// 弹出当前注意力下优先级最高（P0 优先）的待收敛目标。
    pub fn pop_next(&mut self) -> Option<ConvergenceTarget> {
        let best = self
            .pending
            .iter()
            .min_by(|a, b| {
                self.attention
                    .priority_for(a)
                    .cmp(&self.attention.priority_for(b))
            })
            .cloned()?;
        self.pending.remove(&best);
        Some(best)
    }

    /// 当前待收敛项里优先级最低（最该被背压逐出）的一项。
    fn worst_pending(&self) -> Option<(ConvergenceTarget, ConvergencePriority)> {
        self.pending
            .iter()
            .map(|t| (t.clone(), self.attention.priority_for(t)))
            .max_by(|a, b| a.1.cmp(&b.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(id: &str) -> ConvergenceTarget {
        ConvergenceTarget::Conversation(id.to_string())
    }

    #[test]
    fn priority_follows_attention_foreground_visible_recent_other() {
        let mut a = AttentionState::default();
        a.open_timeline("c-fg");
        a.set_visible(["c-vis-1", "c-vis-2"]);
        a.mark_recent("c-recent");

        assert_eq!(
            a.priority_for(&conv("c-fg")),
            ConvergencePriority::P0Foreground
        );
        assert_eq!(
            a.priority_for(&conv("c-vis-1")),
            ConvergencePriority::P1VisibleWindow
        );
        assert_eq!(
            a.priority_for(&conv("c-recent")),
            ConvergencePriority::P2Recent
        );
        assert_eq!(
            a.priority_for(&conv("c-cold")),
            ConvergencePriority::P3Ambient
        );
        assert_eq!(
            a.priority_for(&ConvergenceTarget::Global),
            ConvergencePriority::P3Ambient
        );
    }

    #[test]
    fn pop_next_returns_foreground_first_regardless_of_insert_order() {
        let mut s = ConvergenceScheduler::new(16);
        s.enqueue(conv("cold-1"));
        s.enqueue(conv("cold-2"));
        s.enqueue(conv("fg")); // 最后入队，但它是前台
        s.attention_mut().open_timeline("fg");

        assert_eq!(s.pop_next(), Some(conv("fg")));
    }

    #[test]
    fn opening_timeline_bumps_that_conversation_to_front() {
        let mut s = ConvergenceScheduler::new(16);
        s.enqueue(conv("a"));
        s.enqueue(conv("b"));
        // 未开会话时按 P3 平级，开 b 后 b 升 P0 优先弹出。
        s.attention_mut().open_timeline("b");
        assert_eq!(s.pop_next(), Some(conv("b")));
        assert_eq!(s.pop_next(), Some(conv("a")));
        assert_eq!(s.pop_next(), None);
    }

    #[test]
    fn bounded_backpressure_high_priority_evicts_low_and_low_is_rejected() {
        let mut s = ConvergenceScheduler::new(2);
        assert!(s.enqueue(conv("cold-1"))); // P3
        assert!(s.enqueue(conv("cold-2"))); // P3, 满
        // 前台会话（P0）应逐出一个 P3 冷会话被接纳。
        s.attention_mut().open_timeline("fg");
        assert!(s.enqueue(conv("fg")));
        assert_eq!(s.pending_len(), 2);
        // 再来一个冷会话（P3），队列满且它不高于最差项 → 拒绝（背压）。
        assert!(!s.enqueue(conv("cold-3")));
        assert_eq!(s.pending_len(), 2);
        // 前台仍最先弹出。
        assert_eq!(s.pop_next(), Some(conv("fg")));
    }

    #[test]
    fn enqueue_dedups_targets() {
        let mut s = ConvergenceScheduler::new(8);
        assert!(s.enqueue(conv("a")));
        assert!(s.enqueue(conv("a")));
        assert_eq!(s.pending_len(), 1);
    }

    #[test]
    fn shared_attention_registry_seeds_scheduler_priority() {
        let reg = AttentionRegistry::new();
        reg.open_timeline("fg");
        reg.set_visible(["vis"]);
        // 快照播种：调度器据引擎共享注意力排序，前台/可见优先。
        let mut s = ConvergenceScheduler::with_attention(16, reg.snapshot());
        s.enqueue(conv("cold"));
        s.enqueue(conv("fg"));
        s.enqueue(conv("vis"));
        assert_eq!(s.pop_next(), Some(conv("fg"))); // P0
        assert_eq!(s.pop_next(), Some(conv("vis"))); // P1
        assert_eq!(s.pop_next(), Some(conv("cold"))); // P3
        // 快照与后续 registry 变更解耦。
        reg.close_timeline("fg");
        assert_eq!(
            reg.snapshot().priority_for(&conv("fg")),
            ConvergencePriority::P3Ambient
        );
    }

    #[test]
    fn close_timeline_clears_only_matching_foreground() {
        let mut a = AttentionState::default();
        a.open_timeline("x");
        a.close_timeline("y"); // 非当前前台，不影响
        assert_eq!(
            a.priority_for(&conv("x")),
            ConvergencePriority::P0Foreground
        );
        a.close_timeline("x");
        assert_eq!(a.priority_for(&conv("x")), ConvergencePriority::P3Ambient);
    }
}
