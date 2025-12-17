//! 统一任务调度器
//!
//! 负责统一调度所有任务（包括内置任务和用户自定义任务）
//!
//! ## 设计原则
//!
//! 1. **统一调度**：所有任务都通过调度器执行，包括同步会话、同步消息等
//! 2. **优先级调度**：根据任务优先级决定执行顺序
//! 3. **支持重试**：失败任务自动重试（可配置）
//! 4. **去重机制**：相同任务只执行一次（基于任务 ID）
//! 5. **用户扩展**：允许开发者和各端使用者注册自定义任务

use crate::infrastructure::task::PriorityTask;
use crate::infrastructure::task::executor::{SyncContext, SyncTaskExecutor};
use crate::infrastructure::task::manager::{TaskManager, TaskType as ManagerTaskType};
use crate::infrastructure::task::scheduler_stats::{
    TaskExecutionStats, TaskSchedulerInternalStats, TaskSchedulerPerformanceSnapshot,
};
use crate::infrastructure::task::standard::{TaskResult, TaskStatus};
use crate::infrastructure::task::task::SyncTask;
use anyhow::Result;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{Duration, Instant, sleep};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// 任务调度器配置
#[derive(Debug, Clone)]
pub struct TaskSchedulerConfig {
    /// 最大并发任务数
    pub max_concurrent_tasks: usize,

    /// 任务重试次数
    pub max_retries: u32,

    /// 重试延迟（毫秒）
    pub retry_delay_ms: u64,

    /// 任务去重窗口（秒）
    pub dedup_window_secs: u64,

    /// 是否启用任务去重
    pub enable_dedup: bool,
}

impl Default for TaskSchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_tasks: 5,
            max_retries: 3,
            retry_delay_ms: 1000,
            dedup_window_secs: 60,
            enable_dedup: true,
        }
    }
}

/// 任务调度器
///
/// 统一调度所有任务，支持：
/// - 优先级调度
/// - 自动重试
/// - 任务去重
/// - 用户自定义任务注册
pub struct TaskScheduler {
    /// 任务管理器（用于管理后台任务句柄）
    task_manager: Arc<TaskManager>,

    /// 同步上下文
    context: Arc<SyncContext>,

    /// 配置
    config: Arc<RwLock<TaskSchedulerConfig>>,

    /// 已注册的任务执行器（任务名称 -> 执行器）
    executors: Arc<Mutex<HashMap<String, Arc<dyn SyncTaskExecutor>>>>,

    /// 待执行任务队列（优先队列，按优先级自动排序）
    pending_tasks: Arc<Mutex<BinaryHeap<PriorityTask>>>,

    /// 正在执行的任务（任务 ID -> 任务）
    running_tasks: Arc<Mutex<HashMap<String, SyncTask>>>,

    /// 已完成的任务（任务 ID -> 结果，用于去重）
    /// 优化：使用 BTreeMap 按时间排序，方便清理过期任务
    completed_tasks: Arc<Mutex<HashMap<String, (TaskResult, Instant)>>>,

    /// 任务执行统计（用于性能监控和限流）
    stats: Arc<Mutex<TaskSchedulerInternalStats>>,

    /// 是否启用
    enabled: Arc<RwLock<bool>>,

    /// 调度器任务句柄
    scheduler_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl TaskScheduler {
    /// 创建新的任务调度器
    pub fn new(
        task_manager: Arc<TaskManager>,
        context: SyncContext,
        config: TaskSchedulerConfig,
    ) -> Self {
        Self {
            task_manager,
            context: Arc::new(context),
            config: Arc::new(RwLock::new(config)),
            executors: Arc::new(Mutex::new(HashMap::new())),
            pending_tasks: Arc::new(Mutex::new(BinaryHeap::new())),
            running_tasks: Arc::new(Mutex::new(HashMap::new())),
            completed_tasks: Arc::new(Mutex::new(HashMap::new())),
            enabled: Arc::new(RwLock::new(false)),
            scheduler_handle: Arc::new(Mutex::new(None)),
            stats: Arc::new(Mutex::new(TaskSchedulerInternalStats::new())),
        }
    }

    /// 启用调度器
    pub async fn enable(&self) -> Result<()> {
        let mut enabled = self.enabled.write().await;
        if *enabled {
            return Ok(());
        }

        *enabled = true;
        info!("Task scheduler enabled");

        // 启动调度器循环
        self.start_scheduler_loop().await?;

        Ok(())
    }

    /// 禁用调度器
    pub async fn disable(&self) {
        let mut enabled = self.enabled.write().await;
        if !*enabled {
            return;
        }

        *enabled = false;
        info!("Task scheduler disabled");

        // 停止调度器循环
        let mut handle_guard = self.scheduler_handle.lock().await;
        if let Some(handle) = handle_guard.take() {
            handle.abort();
        }
    }

    /// 注册任务执行器
    ///
    /// # 参数
    /// - `executor`: 任务执行器
    ///
    /// # 示例
    ///
    /// ```rust,no_run
    /// use flare_im_core_sdk::infrastructure::task::executor::SyncTaskExecutor;
    /// use flare_im_core_sdk::infrastructure::task::standard::{priority, TaskResult, TaskType};
    /// use async_trait::async_trait;
    ///
    /// struct MyCustomTask;
    ///
    /// #[async_trait]
    /// impl SyncTaskExecutor for MyCustomTask {
    ///     fn name(&self) -> &str { "MyCustomTask" }
    ///     fn priority(&self) -> u32 { priority::MEDIUM }
    ///     async fn execute(&self, context: &SyncContext) -> Result<TaskResult> {
    ///         // 执行自定义逻辑
    ///         Ok(TaskResult::success("my-task".to_string(), 0, 0, 100))
    ///     }
    /// }
    ///
    /// // 注册任务
    /// scheduler.register_task(Arc::new(MyCustomTask)).await;
    /// ```
    pub async fn register_task(&self, executor: Arc<dyn SyncTaskExecutor>) {
        let name = executor.name().to_string();
        let mut executors = self.executors.lock().await;
        executors.insert(name.clone(), executor);
        info!(task_name = %name, "Task executor registered");
    }

    /// 取消注册任务执行器
    pub async fn unregister_task(&self, name: &str) -> bool {
        let mut executors = self.executors.lock().await;
        executors.remove(name).is_some()
    }

    /// 获取所有已注册的任务名称
    pub async fn get_registered_tasks(&self) -> Vec<String> {
        let executors = self.executors.lock().await;
        executors.keys().cloned().collect()
    }

    /// 调度任务（立即执行或加入队列）
    ///
    /// # 参数
    /// - `executor`: 任务执行器
    /// - `task_id`: 任务 ID（可选，如果不提供则自动生成）
    ///
    /// # 返回
    /// - `String`: 任务 ID
    pub async fn schedule_task(
        &self,
        executor: Arc<dyn SyncTaskExecutor>,
        task_id: Option<String>,
    ) -> Result<String> {
        if !*self.enabled.read().await {
            return Err(anyhow::anyhow!("Task scheduler is not enabled"));
        }

        let task_id = task_id.unwrap_or_else(|| format!("{}-{}", executor.name(), Uuid::new_v4()));

        // 检查去重
        if self.should_dedup(&task_id, executor.name()).await {
            debug!(task_id = %task_id, "Task deduplicated, skipping");
            return Ok(task_id);
        }

        // 创建任务实例
        let mut task = SyncTask::new(task_id.clone(), executor);

        // 根据任务类型和执行模式决定执行方式
        let task_type = task.task_type();
        match task_type {
            crate::infrastructure::task::standard::TaskType::Blocking => {
                // 强制加载任务（Blocking Task）：必须在主流程中立即完成
                // - 立即执行，阻塞等待完成
                // - 错误必须立即抛出
                // - 不能丢、不能延迟
                // - 发布事件通知客户端
                self.execute_blocking_task(&mut task).await?;
            }
            crate::infrastructure::task::standard::TaskType::Background => {
                // 后台慢加载任务（Background Task）：可异步执行，不阻塞主流程
                // - 加入队列，按优先级调度
                // - 失败可自动重试
                // - 发布事件通知客户端
                self.enqueue_task(task).await;
            }
        }

        Ok(task_id)
    }

    /// 调度内置任务（通过任务名称）
    ///
    /// # 参数
    /// - `task_name`: 任务名称（必须是已注册的任务）
    /// - `task_id`: 任务 ID（可选）
    ///
    /// # 返回
    /// - `Result<String>`: 任务 ID
    pub async fn schedule_task_by_name(
        &self,
        task_name: &str,
        task_id: Option<String>,
    ) -> Result<String> {
        let executors = self.executors.lock().await;
        let executor = executors
            .get(task_name)
            .ok_or_else(|| anyhow::anyhow!("Task executor not found: {}", task_name))?
            .clone();
        drop(executors);

        self.schedule_task(executor, task_id).await
    }

    /// 获取任务状态
    pub async fn get_task_status(&self, task_id: &str) -> Option<TaskStatus> {
        // 检查正在执行的任务
        let running = self.running_tasks.lock().await;
        if let Some(task) = running.get(task_id) {
            return Some(task.status);
        }
        drop(running);

        // 检查已完成的任务
        let completed = self.completed_tasks.lock().await;
        if completed.contains_key(task_id) {
            return Some(TaskStatus::Completed);
        }

        // 检查待执行队列
        let pending = self.pending_tasks.lock().await;
        if pending.iter().any(|t| t.task.task_id == task_id) {
            return Some(TaskStatus::Pending);
        }

        None
    }

    /// 取消任务（优化：BinaryHeap 不支持 retain，需要重建）
    pub async fn cancel_task(&self, task_id: &str) -> bool {
        // 从待执行队列中移除
        let mut pending = self.pending_tasks.lock().await;
        // BinaryHeap 不支持 retain，需要重建
        let mut new_heap = BinaryHeap::new();
        let mut found = false;
        while let Some(priority_task) = pending.pop() {
            if priority_task.task.task_id != task_id {
                new_heap.push(priority_task);
            } else {
                found = true;
            }
        }
        *pending = new_heap;
        drop(pending);

        if found {
            return true;
        }

        // 从正在执行的任务中移除（标记为取消）
        let mut running = self.running_tasks.lock().await;
        if running.remove(task_id).is_some() {
            return true;
        }

        false
    }

    /// 获取调度器统计信息（优化：并行获取，减少锁持有时间）
    pub async fn get_stats(&self) -> TaskSchedulerStats {
        // 优化：并行获取多个锁，减少总等待时间
        let (pending_len, running_len, executors_len, enabled) = tokio::join!(
            async { self.pending_tasks.lock().await.len() },
            async { self.running_tasks.lock().await.len() },
            async { self.executors.lock().await.len() },
            async { *self.enabled.read().await },
        );

        TaskSchedulerStats {
            registered_tasks: executors_len,
            pending_tasks: pending_len,
            running_tasks: running_len,
            enabled,
        }
    }

    /// 获取性能快照（新增：用于性能监控）
    pub async fn get_performance_snapshot(&self) -> TaskSchedulerPerformanceSnapshot {
        let stats = self.stats.lock().await;
        TaskSchedulerPerformanceSnapshot::from(&*stats)
    }

    // ============================================================================
    // 内部方法
    // ============================================================================

    /// 启动调度器循环
    async fn start_scheduler_loop(&self) -> Result<()> {
        let scheduler = Arc::new(self.clone_for_scheduler());
        let handle = tokio::spawn(async move {
            scheduler.scheduler_loop().await;
        });

        let mut handle_guard = self.scheduler_handle.lock().await;
        *handle_guard = Some(handle);

        Ok(())
    }

    /// 调度器主循环
    async fn scheduler_loop(&self) {
        let mut interval = tokio::time::interval(Duration::from_millis(100));

        loop {
            // 检查是否已禁用
            {
                let enabled = self.enabled.read().await;
                if !*enabled {
                    break;
                }
            }

            // 处理待执行任务
            self.process_pending_tasks().await;

            // 清理过期的已完成任务
            self.cleanup_completed_tasks().await;

            // 等待下一次循环
            interval.tick().await;
        }

        debug!("Scheduler loop stopped");
    }

    /// 处理待执行任务（优化：使用优先队列，O(log n) 插入和删除）
    async fn process_pending_tasks(&self) {
        let config = self.config.read().await;
        let max_concurrent = config.max_concurrent_tasks;
        drop(config);

        // 检查当前并发数
        let running = self.running_tasks.lock().await;
        let current_running = running.len();
        drop(running);

        if current_running >= max_concurrent {
            return; // 已达到最大并发数
        }

        // 批量处理任务（最多处理到最大并发数）
        let mut tasks_to_execute = Vec::new();
        {
            let mut pending = self.pending_tasks.lock().await;
            while !pending.is_empty() && tasks_to_execute.len() < max_concurrent {
                if let Some(priority_task) = pending.pop() {
                    tasks_to_execute.push(priority_task.into_task());
                } else {
                    break;
                }
            }
        }

        // 并发执行任务（不阻塞）
        for task in tasks_to_execute {
            self.execute_task_async(task).await;
        }
    }

    /// 将任务加入队列（优化：使用优先队列，O(log n) 插入）
    async fn enqueue_task(&self, task: SyncTask) {
        let priority_task = PriorityTask::new(task);
        let queue_len = {
            let mut pending = self.pending_tasks.lock().await;
            pending.push(priority_task);
            pending.len()
        };

        debug!(task_count = queue_len, "Task enqueued (priority queue)");
    }

    /// 异步执行任务（用于 Background 任务）
    ///
    /// **特性**：
    /// - 异步执行，不阻塞主流程
    /// - 失败可自动重试
    /// - 发布事件通知客户端
    async fn execute_task_async(&self, task: SyncTask) {
        use crate::infrastructure::event::{Event, TaskEvent};

        let task_id = task.task_id.clone();
        let task_name = task.name().to_string();
        let task_description = task.description().to_string();
        let executor = task.get_executor();
        let context = Arc::clone(&self.context);
        let event_bus = Arc::clone(&context.event_bus);
        let scheduler = Arc::new(self.clone_for_scheduler());
        let config = self.config.read().await.clone();
        drop(config);

        // 发布任务开始事件
        event_bus.publish(Event::Task(TaskEvent::BackgroundTaskStarted {
            task_id: task_id.clone(),
            task_name: task_name.clone(),
            task_description: task_description.clone(),
        }));

        // 添加到正在执行的任务列表
        {
            let mut running = self.running_tasks.lock().await;
            running.insert(task_id.clone(), task);
        }

        // 在后台执行任务
        let task_id_clone = task_id.clone();
        let task_name_clone = task_name.clone();
        let event_bus_clone = Arc::clone(&event_bus);
        let stats_clone = Arc::clone(&scheduler.stats);
        let execution_start = Instant::now();
        let handle = tokio::spawn(async move {
            let mut retries = 0;
            let max_retries = scheduler.config.read().await.max_retries;
            let retry_delay_ms = scheduler.config.read().await.retry_delay_ms;

            // 记录任务开始执行
            let mut execution_stats = TaskExecutionStats {
                task_id: task_id_clone.clone(),
                task_name: task_name_clone.clone(),
                start_time: execution_start,
                end_time: None,
                success: false,
                retry_count: 0,
            };

            loop {
                // 执行任务
                let result = executor.execute(&context).await;

                match result {
                    Ok(task_result) => {
                        // 记录执行成功
                        execution_stats.end_time = Some(Instant::now());
                        execution_stats.success = true;
                        execution_stats.retry_count = retries;
                        {
                            let mut stats = stats_clone.lock().await;
                            stats.record_execution(execution_stats.clone());
                        }

                        // 成功：保存结果并移除，发布完成事件
                        {
                            let mut running = scheduler.running_tasks.lock().await;
                            running.remove(&task_id_clone);
                        }

                        {
                            let mut completed = scheduler.completed_tasks.lock().await;
                            completed.insert(
                                task_id_clone.clone(),
                                (task_result.clone(), Instant::now()),
                            );
                        }

                        // 发布完成事件（优化：异步发布，不阻塞）
                        let event_bus_final = Arc::clone(&event_bus_clone);
                        let task_id_final = task_id_clone.clone();
                        let task_name_final = task_name_clone.clone();
                        let task_result_final = task_result.clone();
                        tokio::spawn(async move {
                            event_bus_final.publish(Event::Task(
                                TaskEvent::BackgroundTaskCompleted {
                                    task_id: task_id_final,
                                    task_name: task_name_final,
                                    result: task_result_final,
                                },
                            ));
                        });

                        info!(
                            task_id = %task_id_clone,
                            task_name = %task_name_clone,
                            "Background task completed successfully"
                        );
                        break;
                    }
                    Err(e) => {
                        // 失败：检查是否需要重试
                        if retries < max_retries {
                            retries += 1;
                            execution_stats.retry_count = retries;

                            // 发布重试事件（优化：异步发布，不阻塞）
                            let event_bus_retry = Arc::clone(&event_bus_clone);
                            let task_id_retry = task_id_clone.clone();
                            let task_name_retry = task_name_clone.clone();
                            tokio::spawn(async move {
                                event_bus_retry.publish(Event::Task(
                                    TaskEvent::BackgroundTaskRetry {
                                        task_id: task_id_retry,
                                        task_name: task_name_retry,
                                        retry_count: retries,
                                        max_retries,
                                        retry_delay_ms,
                                    },
                                ));
                            });

                            warn!(
                                task_id = %task_id_clone,
                                task_name = %task_name_clone,
                                retry = retries,
                                max_retries = max_retries,
                                error = %e,
                                "Background task failed, retrying..."
                            );
                            sleep(Duration::from_millis(retry_delay_ms)).await;
                            continue;
                        } else {
                            // 记录执行失败
                            execution_stats.end_time = Some(Instant::now());
                            execution_stats.success = false;
                            execution_stats.retry_count = retries;
                            {
                                let mut stats = stats_clone.lock().await;
                                stats.record_execution(execution_stats.clone());
                            }

                            // 重试次数用完，标记为失败，发布失败事件
                            {
                                let mut running = scheduler.running_tasks.lock().await;
                                running.remove(&task_id_clone);
                            }

                            // 发布失败事件（优化：异步发布，不阻塞）
                            let event_bus_failed = Arc::clone(&event_bus_clone);
                            let task_id_failed = task_id_clone.clone();
                            let task_name_failed = task_name_clone.clone();
                            let error_msg = e.to_string();
                            tokio::spawn(async move {
                                event_bus_failed.publish(Event::Task(
                                    TaskEvent::BackgroundTaskFailed {
                                        task_id: task_id_failed,
                                        task_name: task_name_failed,
                                        error: error_msg,
                                        retry_count: retries,
                                        max_retries,
                                        will_retry: false,
                                    },
                                ));
                            });

                            error!(
                                task_id = %task_id_clone,
                                task_name = %task_name_clone,
                                error = %e,
                                "Background task failed after {} retries", max_retries
                            );
                            break;
                        }
                    }
                }
            }
        });

        // 注册到任务管理器
        self.task_manager
            .register(
                task_id.clone(),
                handle,
                ManagerTaskType::SyncTask,
                task_name,
                true, // 可取消
            )
            .await;
    }

    /// 执行强制加载任务（Blocking Task）
    ///
    /// **特性**：
    /// - 立即执行，阻塞等待完成
    /// - 错误必须立即抛出
    /// - 不能丢、不能延迟
    /// - 发布事件通知客户端
    async fn execute_blocking_task(&self, task: &mut SyncTask) -> Result<()> {
        use crate::infrastructure::event::{Event, TaskEvent};
        use std::time::Instant;

        let task_id = task.task_id.clone();
        let task_name = task.name().to_string();
        let task_description = task.description().to_string();
        let executor = task.get_executor();
        let context = Arc::clone(&self.context);
        let event_bus = Arc::clone(&context.event_bus);

        let start_time = Instant::now();

        // 发布任务开始事件
        event_bus.publish(Event::Task(TaskEvent::BlockingTaskStarted {
            task_id: task_id.clone(),
            task_name: task_name.clone(),
            task_description: task_description.clone(),
        }));

        task.status = TaskStatus::Running;
        info!(
            task_id = %task_id,
            task_name = %task_name,
            "Blocking task started (must complete immediately)"
        );

        // 执行任务（阻塞等待）
        let result = executor.execute(&context).await;

        let duration_ms = start_time.elapsed().as_millis() as u64;

        match result {
            Ok(task_result) => {
                // 成功：更新状态并发布完成事件（优化：异步发布）
                task.status = TaskStatus::Completed;
                task.result = Some(task_result.clone());

                let event_bus_clone = Arc::clone(&event_bus);
                let task_id_clone = task_id.clone();
                let task_name_clone = task_name.clone();
                let task_result_clone = task_result.clone();
                tokio::spawn(async move {
                    event_bus_clone.publish(Event::Task(TaskEvent::BlockingTaskCompleted {
                        task_id: task_id_clone,
                        task_name: task_name_clone,
                        result: task_result_clone,
                    }));
                });

                info!(
                    task_id = %task_id,
                    task_name = %task_name,
                    duration_ms = duration_ms,
                    "Blocking task completed successfully"
                );
                Ok(())
            }
            Err(e) => {
                // 失败：更新状态并发布失败事件，然后立即抛出错误（优化：异步发布）
                task.status = TaskStatus::Failed;
                let error_msg = e.to_string();
                task.error = Some(error_msg.clone());

                let event_bus_clone = Arc::clone(&event_bus);
                let task_id_clone = task_id.clone();
                let task_name_clone = task_name.clone();
                let error_msg_clone = error_msg.clone();
                tokio::spawn(async move {
                    event_bus_clone.publish(Event::Task(TaskEvent::BlockingTaskFailed {
                        task_id: task_id_clone,
                        task_name: task_name_clone,
                        error: error_msg_clone,
                        duration_ms,
                    }));
                });

                error!(
                    task_id = %task_id,
                    task_name = %task_name,
                    error = %error_msg,
                    duration_ms = duration_ms,
                    "Blocking task failed (error thrown immediately)"
                );

                // 错误必须立即抛出，不能忽略
                Err(anyhow::anyhow!("Blocking task failed: {}", error_msg))
            }
        }
    }

    /// 同步执行任务（兼容旧版本，内部使用）
    async fn execute_task_sync(&self, task: &mut SyncTask) -> Result<()> {
        self.execute_blocking_task(task).await
    }

    /// 检查是否应该去重（优化：减少锁持有时间）
    async fn should_dedup(&self, task_id: &str, _task_name: &str) -> bool {
        // 快速检查：配置是否启用去重
        let enable_dedup = {
            let config = self.config.read().await;
            config.enable_dedup
        };

        if !enable_dedup {
            return false;
        }

        // 检查是否正在执行（优先检查，因为更常见）
        {
            let running = self.running_tasks.lock().await;
            if running.contains_key(task_id) {
                return true; // 正在执行，应该去重
            }
        }

        // 检查是否在去重窗口内
        let window = {
            let config = self.config.read().await;
            Duration::from_secs(config.dedup_window_secs)
        };

        let now = Instant::now();
        {
            let completed = self.completed_tasks.lock().await;
            if let Some((_, completed_time)) = completed.get(task_id) {
                if now.duration_since(*completed_time) < window {
                    return true; // 在去重窗口内，应该去重
                }
            }
        }

        false
    }

    /// 清理过期的已完成任务
    async fn cleanup_completed_tasks(&self) {
        let window = Duration::from_secs(self.config.read().await.dedup_window_secs);
        let now = Instant::now();

        let mut completed = self.completed_tasks.lock().await;
        completed.retain(|_, (_, completed_time)| now.duration_since(*completed_time) < window);
    }

    /// 克隆用于调度器循环
    fn clone_for_scheduler(&self) -> Self {
        Self {
            task_manager: Arc::clone(&self.task_manager),
            context: Arc::clone(&self.context),
            config: Arc::clone(&self.config),
            executors: Arc::clone(&self.executors),
            pending_tasks: Arc::clone(&self.pending_tasks),
            running_tasks: Arc::clone(&self.running_tasks),
            completed_tasks: Arc::clone(&self.completed_tasks),
            enabled: Arc::clone(&self.enabled),
            scheduler_handle: Arc::clone(&self.scheduler_handle),
            stats: Arc::clone(&self.stats),
        }
    }
}

/// 任务调度器统计信息
#[derive(Debug, Clone)]
pub struct TaskSchedulerStats {
    /// 已注册的任务数
    pub registered_tasks: usize,

    /// 待执行任务数
    pub pending_tasks: usize,

    /// 正在执行的任务数
    pub running_tasks: usize,

    /// 是否启用
    pub enabled: bool,
}
