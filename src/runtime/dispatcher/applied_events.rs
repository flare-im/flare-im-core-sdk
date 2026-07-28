//! 已应用操作事件的会话 seq 注册表（AUDIT-25-01）。
//!
//! 服务端给**所有**通用事件（recall/edit/reaction/read/pin/delete…）从与消息同一个
//! 会话 seq 分配器分配 `conversation_seq`：操作事件占据的 seq 永远不会出现消息行。
//! 若 waterline / seq 缺口判定只认消息行，这些 seq 会被当成「缺口」→ 每条操作事件
//! 在接收端触发一次全量单会话同步（RPC 风暴）+ seq_repair 冗余补拉与 warn 噪音。
//!
//! 本注册表记录「已应用的操作事件 seq」，使连续性判定把它们视作已填充。
//!
//! 有界内存态：按会话 LRU（上限 [`MAX_CONVERSATIONS`]），每会话只保留最大的
//! [`MAX_SEQS_PER_CONVERSATION`] 个 seq（淘汰最小 = 最老）。不持久化：持久化的连续性
//! 由会话同步游标承载（操作事件应用成功后游标照常推进并落库，见
//! `Dispatcher::advance_cursor_over_applied_event`）；重启后最坏情况是针对陈旧事件
//! seq 的 waterline ping 多触发一次合并补拉，可接受。

use std::collections::{BTreeSet, HashMap};
use std::ops::Bound;
use std::sync::Arc;

use tokio::sync::Mutex as AsyncMutex;

const MAX_CONVERSATIONS: usize = 2_048;
const MAX_SEQS_PER_CONVERSATION: usize = 1_024;

#[derive(Debug, Default)]
struct ConversationSeqs {
    seqs: BTreeSet<u64>,
    touched_at_ms: u64,
}

/// 可克隆句柄；克隆共享同一份状态（供 waterline 后台补拉任务使用）。
#[derive(Clone, Default)]
pub(super) struct AppliedEventSeqs {
    inner: Arc<AsyncMutex<HashMap<String, ConversationSeqs>>>,
}

impl AppliedEventSeqs {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// 记录一个已应用（或正在应用）的操作事件 seq。
    pub(super) async fn record(&self, conversation_id: &str, seq: u64) {
        let conversation_id = conversation_id.trim();
        if seq == 0 || conversation_id.is_empty() {
            return;
        }
        let now = crate::shared::util::time::now_millis();
        let mut map = self.inner.lock().await;
        if !map.contains_key(conversation_id) && map.len() >= MAX_CONVERSATIONS {
            // LRU：淘汰最久未触达的会话
            if let Some(oldest) = map
                .iter()
                .min_by_key(|(_, entry)| entry.touched_at_ms)
                .map(|(key, _)| key.clone())
            {
                map.remove(&oldest);
            }
        }
        let entry = map.entry(conversation_id.to_string()).or_default();
        entry.touched_at_ms = now;
        entry.seqs.insert(seq);
        while entry.seqs.len() > MAX_SEQS_PER_CONVERSATION {
            entry.seqs.pop_first();
        }
    }

    /// 回滚一次记录（事件应用失败、去重键回滚待重放时调用）。
    pub(super) async fn unrecord(&self, conversation_id: &str, seq: u64) {
        let conversation_id = conversation_id.trim();
        if seq == 0 || conversation_id.is_empty() {
            return;
        }
        let mut map = self.inner.lock().await;
        if let Some(entry) = map.get_mut(conversation_id) {
            entry.seqs.remove(&seq);
            if entry.seqs.is_empty() {
                map.remove(conversation_id);
            }
        }
    }

    pub(super) async fn contains(&self, conversation_id: &str, seq: u64) -> bool {
        let map = self.inner.lock().await;
        map.get(conversation_id.trim())
            .is_some_and(|entry| entry.seqs.contains(&seq))
    }

    /// 返回 `(above, upto]` 区间内已应用的操作事件 seq（升序）。`upto = u64::MAX` 表示不设上界。
    pub(super) async fn seqs_in_range(
        &self,
        conversation_id: &str,
        above: u64,
        upto: u64,
    ) -> Vec<u64> {
        let map = self.inner.lock().await;
        map.get(conversation_id.trim())
            .map(|entry| {
                entry
                    .seqs
                    .range((Bound::Excluded(above), Bound::Included(upto)))
                    .copied()
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn record_contains_unrecord_roundtrip() {
        let registry = AppliedEventSeqs::new();
        registry.record("c1", 5).await;
        assert!(registry.contains("c1", 5).await);
        assert!(!registry.contains("c1", 6).await);
        registry.unrecord("c1", 5).await;
        assert!(!registry.contains("c1", 5).await);
    }

    #[tokio::test]
    async fn per_conversation_capacity_evicts_lowest_seq() {
        let registry = AppliedEventSeqs::new();
        for seq in 1..=(MAX_SEQS_PER_CONVERSATION as u64 + 8) {
            registry.record("c1", seq).await;
        }
        // 最小（最老）的 seq 被淘汰，最新的保留
        assert!(!registry.contains("c1", 1).await);
        assert!(
            registry
                .contains("c1", MAX_SEQS_PER_CONVERSATION as u64 + 8)
                .await
        );
    }

    #[tokio::test]
    async fn seqs_in_range_is_exclusive_inclusive() {
        let registry = AppliedEventSeqs::new();
        for seq in [3_u64, 4, 6, 9] {
            registry.record("c1", seq).await;
        }
        assert_eq!(registry.seqs_in_range("c1", 3, 6).await, vec![4, 6]);
        assert_eq!(
            registry.seqs_in_range("c1", 0, u64::MAX).await,
            vec![3, 4, 6, 9]
        );
        assert!(registry.seqs_in_range("missing", 0, u64::MAX).await.is_empty());
    }

    #[tokio::test]
    async fn zero_seq_and_empty_conversation_are_ignored() {
        let registry = AppliedEventSeqs::new();
        registry.record("c1", 0).await;
        registry.record("  ", 7).await;
        assert!(registry.seqs_in_range("c1", 0, u64::MAX).await.is_empty());
    }
}
