//! 同步进度模型与进度上报
//!
//! [SyncProgress] 供 UI 展示总体进度（权重、当前任务）；[SyncProgressReporter] 由引擎注入到 [super::SyncContext]，
//! 任务通过它上报权重完成与当前描述，引擎汇总后通过 EventBus 发出 [crate::kernel::event::SdkEvent::SyncProgress].

use std::sync::{Arc, Mutex};

use crate::kernel::event::{EventBus, SdkEvent, SyncNotify};

use super::task::SyncRunContext;

/// 同步进度快照（供 UI 展示）
///
/// 使用权重聚合多任务进度：每个任务有 weight，完成时 completed_weight 累加，
/// UI 可显示 `completed_weight / total_weight` 与 current_task 描述。
#[derive(Clone, Debug)]
pub struct SyncProgress {
    /// 所有参与本次同步的任务权重之和
    pub total_weight: u32,
    /// 已完成任务权重累计
    pub completed_weight: u32,
    /// 当前正在执行的任务名（或阶段描述）
    pub current_task: String,
}

impl SyncProgress {
    pub fn new(total_weight: u32) -> Self {
        Self {
            total_weight,
            completed_weight: 0,
            current_task: String::new(),
        }
    }

    /// 进度比例 0.0..=1.0
    pub fn ratio(&self) -> f32 {
        if self.total_weight == 0 {
            1.0
        } else {
            self.completed_weight as f32 / self.total_weight as f32
        }
    }

    /// wire 契约上的进度值：**0–100 的整数百分比**（见各端 `SyncEvent.progress`
    /// "Progress percentage from 0 to 100"）。发 0–1 的 `ratio()` 会让强类型端
    /// （Flutter/Apple 的整数解码）在非整数比例上抛异常崩事件流。
    pub fn percent(&self) -> f32 {
        (self.ratio() * 100.0).round()
    }
}

/// 进度上报器：任务与引擎内部用于上报「当前任务描述」与「权重完成」
///
/// 实现由 [super::orchestrator] 提供，内部更新 [SyncProgress] 并发布 [SdkEvent::SyncProgress]。
pub trait SyncProgressReporter: Send + Sync {
    /// 更新当前任务描述（如 "Contacts syncing..."）
    fn report_current(&self, current_task: String);
    /// 上报本任务完成的权重（累加到 completed_weight）
    fn report_weight_completed(&self, task_id: &str, weight: u32, detail: String);
}

/// 基于 EventBus 的进度上报器实现
///
/// 持有 total_weight 与当前 completed_weight/current_task，每次 report 时发布 SyncProgress 事件。
pub struct EventBusProgressReporter {
    bus: EventBus,
    run: SyncRunContext,
    progress: Arc<Mutex<SyncProgress>>,
}

impl EventBusProgressReporter {
    pub fn new(bus: EventBus, run: SyncRunContext, total_weight: u32) -> Self {
        Self {
            bus,
            run,
            progress: Arc::new(Mutex::new(SyncProgress::new(total_weight))),
        }
    }

    #[allow(dead_code)]
    pub fn progress(&self) -> Arc<Mutex<SyncProgress>> {
        self.progress.clone()
    }

    fn publish(&self, progress: &SyncProgress) {
        self.bus.publish(SdkEvent::Sync(SyncNotify::Progress {
            run: self.run.clone(),
            task: progress.current_task.clone(),
            progress: progress.percent(),
            detail: format!("{} / {}", progress.completed_weight, progress.total_weight),
        }));
    }
}

impl SyncProgressReporter for EventBusProgressReporter {
    fn report_current(&self, current_task: String) {
        if let Ok(mut g) = self.progress.lock() {
            g.current_task = current_task;
            self.publish(&g);
        }
    }

    fn report_weight_completed(&self, task_id: &str, weight: u32, detail: String) {
        if let Ok(mut g) = self.progress.lock() {
            g.completed_weight = g.completed_weight.saturating_add(weight);
            g.current_task = task_id.to_string();
            self.bus.publish(SdkEvent::Sync(SyncNotify::Progress {
                run: self.run.clone(),
                task: g.current_task.clone(),
                progress: g.percent(),
                detail,
            }));
        }
    }
}
