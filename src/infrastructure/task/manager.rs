//! 任务管理器
//!
//! 统一管理所有后台任务，确保资源正确清理
//!
//! 参考顶级 IM SDK（微信、Telegram）的设计：
//! - 统一的任务生命周期管理
//! - 优雅关闭机制
//! - 任务取消和清理

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

/// 任务类型
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TaskType {
    /// 事件监听任务
    EventListener,
    /// 消息处理任务
    MessageHandler,
    /// 同步任务
    SyncTask,
    /// 清理任务
    CleanupTask,
    /// 重连任务
    ReconnectTask,
    /// 其他任务
    Other(String),
}

/// 任务信息
pub struct TaskInfo {
    /// 任务句柄
    handle: JoinHandle<()>,
    /// 任务类型
    task_type: TaskType,
    /// 任务名称（用于调试）
    name: String,
    /// 是否可取消
    cancellable: bool,
}

impl TaskInfo {
    pub fn new(
        handle: JoinHandle<()>,
        task_type: TaskType,
        name: String,
        cancellable: bool,
    ) -> Self {
        Self {
            handle,
            task_type,
            name,
            cancellable,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn task_type(&self) -> TaskType {
        self.task_type.clone()
    }

    pub fn is_cancellable(&self) -> bool {
        self.cancellable
    }

    pub fn abort(&self) {
        if self.cancellable {
            self.handle.abort();
        }
    }

    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }
}

/// 任务管理器
///
/// 统一管理所有后台任务，支持：
/// - 任务注册和取消
/// - 按类型管理任务
/// - 优雅关闭
pub struct TaskManager {
    /// 任务列表（任务 ID -> 任务信息）
    tasks: Arc<Mutex<HashMap<String, TaskInfo>>>,

    /// 是否正在关闭
    shutting_down: Arc<RwLock<bool>>,

    /// 关闭超时时间（秒）
    shutdown_timeout_secs: u64,
}

impl TaskManager {
    /// 创建新的任务管理器
    pub fn new(shutdown_timeout_secs: u64) -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            shutting_down: Arc::new(RwLock::new(false)),
            shutdown_timeout_secs,
        }
    }

    /// 注册任务
    ///
    /// # 参数
    /// - `task_id`: 任务 ID（唯一标识）
    /// - `handle`: 任务句柄
    /// - `task_type`: 任务类型
    /// - `name`: 任务名称
    /// - `cancellable`: 是否可取消
    pub async fn register(
        &self,
        task_id: String,
        handle: JoinHandle<()>,
        task_type: TaskType,
        name: String,
        cancellable: bool,
    ) {
        // 检查是否正在关闭
        if *self.shutting_down.read().await {
            warn!(task_id = %task_id, "Attempting to register task during shutdown, aborting immediately");
            handle.abort();
            return;
        }

        let task_id_for_log = task_id.clone();
        let task_info = TaskInfo::new(handle, task_type, name, cancellable);
        let mut tasks = self.tasks.lock().await;
        tasks.insert(task_id, task_info);
        debug!(task_id = %task_id_for_log, "Task registered");
    }

    /// 取消任务
    pub async fn cancel(&self, task_id: &str) -> bool {
        let mut tasks = self.tasks.lock().await;
        if let Some(task) = tasks.remove(task_id) {
            let task_id_clone = task_id.to_string();
            if task.is_cancellable() {
                task.abort();
                debug!(task_id = %task_id_clone, "Task cancelled");
                true
            } else {
                warn!(task_id = %task_id_clone, "Task is not cancellable");
                // 重新插入，因为不能取消
                tasks.insert(task_id_clone, task);
                false
            }
        } else {
            warn!(task_id = %task_id, "Task not found");
            false
        }
    }

    /// 取消所有指定类型的任务
    pub async fn cancel_by_type(&self, task_type: TaskType) -> usize {
        let mut tasks = self.tasks.lock().await;
        let mut cancelled = 0;

        tasks.retain(|task_id, task| {
            if task.task_type() == task_type && task.is_cancellable() {
                task.abort();
                cancelled += 1;
                debug!(task_id = %task_id, task_type = ?task_type, "Task cancelled by type");
                false // 移除
            } else {
                true // 保留
            }
        });

        cancelled
    }

    /// 取消所有任务
    pub async fn cancel_all(&self) -> usize {
        let mut tasks = self.tasks.lock().await;
        let count = tasks.len();

        for (task_id, task) in tasks.iter() {
            if task.is_cancellable() {
                task.abort();
                debug!(task_id = %task_id, "Task cancelled");
            }
        }

        tasks.clear();
        count
    }

    /// 优雅关闭所有任务
    ///
    /// 1. 标记为关闭中
    /// 2. 等待任务完成（带超时）
    /// 3. 强制取消未完成的任务
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        use tokio::time::{Duration, timeout};

        // 标记为关闭中
        *self.shutting_down.write().await = true;
        info!("Task manager shutting down...");

        // 获取所有任务句柄（需要拥有所有权才能 await）
        let mut tasks_handles: Vec<(String, JoinHandle<()>)> = {
            let mut tasks_guard = self.tasks.lock().await;
            tasks_guard
                .drain()
                .map(|(id, info)| (id, info.handle))
                .collect()
        };

        if tasks_handles.is_empty() {
            info!("No tasks to shutdown");
            return Ok(());
        }

        info!(
            task_count = tasks_handles.len(),
            "Waiting for tasks to complete..."
        );

        // 等待所有任务完成（带超时）
        let shutdown_timeout = Duration::from_secs(self.shutdown_timeout_secs);
        let shutdown_result = timeout(shutdown_timeout, async {
            // 顺序等待所有任务（需要可变引用才能 await）
            for (task_id, handle) in tasks_handles.iter_mut() {
                if !handle.is_finished() {
                    match handle.await {
                        Ok(_) => {
                            debug!(task_id = %task_id, "Task completed");
                        }
                        Err(e) => {
                            warn!(task_id = %task_id, error = %e, "Task failed during shutdown");
                        }
                    }
                }
            }
        })
        .await;

        match shutdown_result {
            Ok(_) => {
                info!("All tasks completed gracefully");
            }
            Err(_) => {
                warn!(
                    timeout_secs = self.shutdown_timeout_secs,
                    "Shutdown timeout, forcing cancellation"
                );

                // 强制取消所有未完成的任务
                for (task_id, handle) in tasks_handles.iter() {
                    if !handle.is_finished() {
                        handle.abort();
                        warn!(task_id = %task_id, "Task force cancelled");
                    }
                }
            }
        }

        // 清空任务列表
        self.tasks.lock().await.clear();
        info!("Task manager shutdown complete");

        Ok(())
    }

    /// 获取任务数量
    pub async fn task_count(&self) -> usize {
        self.tasks.lock().await.len()
    }

    /// 获取指定类型的任务数量
    pub async fn task_count_by_type(&self, task_type: TaskType) -> usize {
        let tasks = self.tasks.lock().await;
        tasks
            .values()
            .filter(|task| task.task_type() == task_type)
            .count()
    }

    /// 检查是否正在关闭
    pub async fn is_shutting_down(&self) -> bool {
        *self.shutting_down.read().await
    }

    /// 清理已完成的任务
    pub async fn cleanup_finished(&self) -> usize {
        let mut tasks = self.tasks.lock().await;
        let before_count = tasks.len();

        tasks.retain(|task_id, task| {
            if task.is_finished() {
                debug!(task_id = %task_id, "Removing finished task");
                false
            } else {
                true
            }
        });

        let after_count = tasks.len();
        before_count - after_count
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new(10) // 默认 10 秒超时
    }
}

/// 任务管理器构建器
pub struct TaskManagerBuilder {
    shutdown_timeout_secs: u64,
}

impl TaskManagerBuilder {
    pub fn new() -> Self {
        Self {
            shutdown_timeout_secs: 10,
        }
    }

    pub fn shutdown_timeout(mut self, secs: u64) -> Self {
        self.shutdown_timeout_secs = secs;
        self
    }

    pub fn build(self) -> TaskManager {
        TaskManager::new(self.shutdown_timeout_secs)
    }
}

impl Default for TaskManagerBuilder {
    fn default() -> Self {
        Self::new()
    }
}
