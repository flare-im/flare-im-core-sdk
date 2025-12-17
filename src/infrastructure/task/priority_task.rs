//! 优先级任务包装器
//!
//! 用于实现优先队列（BinaryHeap）

use crate::infrastructure::task::task::SyncTask;
use std::cmp::Ordering;

/// 优先级任务包装器
///
/// 实现 Ord trait，用于 BinaryHeap 按优先级排序
/// 优先级小的（数字小）排在前面（最大堆的逆序）
#[derive(Clone)]
pub struct PriorityTask {
    pub task: SyncTask,
}

impl PriorityTask {
    pub fn new(task: SyncTask) -> Self {
        Self { task }
    }

    pub fn into_task(self) -> SyncTask {
        self.task
    }
}

impl PartialEq for PriorityTask {
    fn eq(&self, other: &Self) -> bool {
        self.task.priority() == other.task.priority()
    }
}

impl Eq for PriorityTask {}

impl PartialOrd for PriorityTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityTask {
    fn cmp(&self, other: &Self) -> Ordering {
        // 优先级小的（数字小）排在前面
        // BinaryHeap 是最大堆，所以需要反转顺序
        other.task.priority().cmp(&self.task.priority())
    }
}
