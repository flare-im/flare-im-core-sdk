//! 同步引擎：仅任务注册与编排；协议由应用层任务在构造时注入处理器并自行调用。

use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;

use crate::event::EventBus;
use crate::store::StoreProvider;

use super::orchestrator::Orchestrator;
use super::{SyncMode, SyncRunContext, SyncTask};

pub struct SyncManager {
    tasks: Mutex<Vec<Arc<dyn SyncTask>>>,
    running: Mutex<Option<JoinHandle<()>>>,
}

impl SyncManager {
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(Vec::new()),
            running: Mutex::new(None),
        }
    }

    pub fn register_task_arc(&self, task: Arc<dyn SyncTask>) {
        match self.tasks.lock() {
            Ok(mut guard) => guard.push(task),
            Err(poisoned) => {
                tracing::warn!("sync manager task lock poisoned, recovering");
                poisoned.into_inner().push(task);
            }
        }
    }

    /// 导出已注册任务（供 `IMClient::login` 合并子 engine 时保留社交 Background 任务）。
    pub fn registered_tasks(&self) -> Vec<Arc<dyn SyncTask>> {
        match self.tasks.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => {
                tracing::warn!("sync manager task lock poisoned during export, recovering");
                poisoned.into_inner().clone()
            }
        }
    }

    pub fn run_sync(&self, user_id: &str, store: StoreProvider, bus: EventBus) {
        self.run_with_context(user_id, SyncRunContext::initial_login(), store, bus);
    }

    /// 执行全部已注册任务（任务内自行调用注入的 SyncProtocolAdapter 等）。
    pub fn run_with_context(
        &self,
        user_id: &str,
        run: SyncRunContext,
        store: StoreProvider,
        bus: EventBus,
    ) {
        self.stop_sync();
        let tasks = match self.tasks.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => {
                tracing::warn!("sync manager task lock poisoned during run, recovering");
                poisoned.into_inner().clone()
            }
        };
        let handle = Orchestrator::new(store, bus).run(user_id, run, tasks);
        match self.running.lock() {
            Ok(mut guard) => *guard = Some(handle),
            Err(poisoned) => {
                tracing::warn!("sync manager running lock poisoned during run, recovering");
                *poisoned.into_inner() = Some(handle);
            }
        }
    }

    /// 立即停止当前同步编排（用于登出/被踢/断连后的会话硬重置）。
    pub fn stop_sync(&self) {
        let handle = match self.running.lock() {
            Ok(mut guard) => guard.take(),
            Err(poisoned) => {
                tracing::warn!("sync manager running lock poisoned during stop, recovering");
                poisoned.into_inner().take()
            }
        };
        if let Some(handle) = handle {
            handle.abort();
        }
    }

    /// 按 task id 静默触发 Background 同步（多端私有数据补偿等）。
    pub fn spawn_background_tasks_by_ids(
        &self,
        user_id: &str,
        task_ids: &[&str],
        store: StoreProvider,
        bus: EventBus,
    ) {
        let selected: Vec<Arc<dyn SyncTask>> = self
            .registered_tasks()
            .into_iter()
            .filter(|task| {
                task.mode() == SyncMode::Background && task_ids.iter().any(|id| *id == task.id())
            })
            .collect();
        if selected.is_empty() {
            return;
        }
        let run = SyncRunContext::silent_multidevice_private_data();
        Orchestrator::new(store, bus).spawn_background_tasks(user_id.to_string(), run, selected);
    }
}

impl Default for SyncManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SyncManager {
    fn drop(&mut self) {
        if let Some(handle) = self.running.get_mut().ok().and_then(|h| h.take()) {
            handle.abort();
        }
    }
}
