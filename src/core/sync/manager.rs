//! 同步引擎：仅任务注册与编排；协议由应用层任务在构造时注入处理器并自行调用。

use std::sync::{Arc, Mutex};

use crate::event::EventBus;
use crate::store::StoreProvider;

use super::SyncTask;
use super::orchestrator::Orchestrator;

pub struct SyncManager {
    tasks: Mutex<Vec<Arc<dyn SyncTask>>>,
}

impl SyncManager {
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(Vec::new()),
        }
    }

    pub fn register_task_arc(&self, task: Arc<dyn SyncTask>) {
        self.tasks.lock().unwrap().push(task);
    }

    /// 执行全部已注册任务（任务内自行调用注入的 SyncHandler 等）。
    pub fn run_sync(&self, user_id: &str, store: StoreProvider, bus: EventBus) {
        let tasks = self.tasks.lock().unwrap().clone();
        Orchestrator::new(store, bus).run(user_id, tasks);
    }
}

impl Default for SyncManager {
    fn default() -> Self {
        Self::new()
    }
}
