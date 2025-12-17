//! 强制加载任务（Blocking Task）与后台慢加载任务（Background Task）示例
//!
//! 演示如何创建和使用两种任务类型，以及如何监听任务事件

use anyhow::Result;
use async_trait::async_trait;
use flare_im_core_sdk::{
    ClientConfig, FlareIMClient,
    infrastructure::event::{Event, TaskEvent},
    infrastructure::task::{
        executor::SyncTaskExecutor,
        standard::{TaskExecutionMode, TaskResult, TaskType, priority},
    },
};
use std::sync::Arc;
use tokio::time::{Duration, sleep};

/// 强制加载任务示例：会话列表同步
///
/// **特性**：
/// - 必须在主流程中立即完成
/// - 影响会话的核心一致性与实时性
/// - 错误必须立即抛出
/// - 不能丢、不能延迟
struct BlockingSessionSyncTask;

#[async_trait]
impl SyncTaskExecutor for BlockingSessionSyncTask {
    fn name(&self) -> &str {
        "BlockingSessionSync"
    }

    fn description(&self) -> &str {
        "强制加载任务：同步会话列表（必须在登录后立即完成）"
    }

    fn task_type(&self) -> TaskType {
        TaskType::Blocking // 强制加载任务
    }

    fn priority(&self) -> u32 {
        priority::MEDIUM_HIGH // 中高优先级
    }

    fn execution_mode(&self) -> TaskExecutionMode {
        TaskExecutionMode::Sync // 必须同步执行
    }

    async fn execute(
        &self,
        context: &flare_im_core_sdk::infrastructure::task::executor::SyncContext,
    ) -> Result<TaskResult> {
        use std::time::Instant;
        let start_time = Instant::now();
        let task_id = format!("blocking-session-sync-{}", uuid::Uuid::new_v4());

        println!("[Blocking Task] 开始同步会话列表...");

        // 模拟同步操作（实际应该调用 storage 或网络请求）
        // 注意：Blocking 任务应该尽可能快地完成
        sleep(Duration::from_millis(500)).await;

        let duration_ms = start_time.elapsed().as_millis() as u64;
        println!("[Blocking Task] 会话列表同步完成，耗时: {}ms", duration_ms);

        Ok(TaskResult::success(
            task_id,
            10, // 同步了 10 个会话
            0,  // 消息数量
            duration_ms,
        ))
    }
}

/// 后台慢加载任务示例：缓存预热
///
/// **特性**：
/// - 可异步执行，不阻塞主流程
/// - 用于非关键逻辑
/// - 失败可自动重试
/// - 可延迟
struct BackgroundCacheWarmupTask {
    cache_keys: Vec<String>,
}

impl BackgroundCacheWarmupTask {
    fn new(cache_keys: Vec<String>) -> Self {
        Self { cache_keys }
    }
}

#[async_trait]
impl SyncTaskExecutor for BackgroundCacheWarmupTask {
    fn name(&self) -> &str {
        "BackgroundCacheWarmup"
    }

    fn description(&self) -> &str {
        "后台慢加载任务：缓存预热（可异步执行，不阻塞主流程）"
    }

    fn task_type(&self) -> TaskType {
        TaskType::Background // 后台慢加载任务
    }

    fn priority(&self) -> u32 {
        priority::LOW // 低优先级
    }

    fn execution_mode(&self) -> TaskExecutionMode {
        TaskExecutionMode::Async // 异步执行
    }

    async fn execute(
        &self,
        context: &flare_im_core_sdk::infrastructure::task::executor::SyncContext,
    ) -> Result<TaskResult> {
        use std::time::Instant;
        let start_time = Instant::now();
        let task_id = format!("background-cache-warmup-{}", uuid::Uuid::new_v4());

        println!("[Background Task] 开始缓存预热...");

        // 模拟缓存预热操作（实际应该从数据库或网络加载数据到缓存）
        for (i, key) in self.cache_keys.iter().enumerate() {
            println!("[Background Task] 预热缓存: {}", key);
            sleep(Duration::from_millis(100)).await;

            // 可以在这里发布进度事件（如果任务支持）
            // context.event_bus.publish(Event::Task(TaskEvent::BackgroundTaskProgress { ... }));
        }

        let duration_ms = start_time.elapsed().as_millis() as u64;
        println!("[Background Task] 缓存预热完成，耗时: {}ms", duration_ms);

        Ok(TaskResult::success(
            task_id,
            0,                     // 会话数量
            self.cache_keys.len(), // 预热了 N 个缓存项
            duration_ms,
        ))
    }
}

/// 后台慢加载任务示例：索引构建
struct BackgroundIndexBuildTask;

#[async_trait]
impl SyncTaskExecutor for BackgroundIndexBuildTask {
    fn name(&self) -> &str {
        "BackgroundIndexBuild"
    }

    fn description(&self) -> &str {
        "后台慢加载任务：构建搜索索引"
    }

    fn task_type(&self) -> TaskType {
        TaskType::Background
    }

    fn priority(&self) -> u32 {
        priority::LOWEST // 最低优先级
    }

    fn execution_mode(&self) -> TaskExecutionMode {
        TaskExecutionMode::Async
    }

    async fn execute(
        &self,
        _context: &flare_im_core_sdk::infrastructure::task::executor::SyncContext,
    ) -> Result<TaskResult> {
        use std::time::Instant;
        let start_time = Instant::now();
        let task_id = format!("background-index-build-{}", uuid::Uuid::new_v4());

        println!("[Background Task] 开始构建搜索索引...");

        // 模拟索引构建（实际应该构建搜索索引）
        sleep(Duration::from_secs(2)).await;

        let duration_ms = start_time.elapsed().as_millis() as u64;
        println!("[Background Task] 索引构建完成，耗时: {}ms", duration_ms);

        Ok(TaskResult::success(task_id, 0, 0, duration_ms))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt::init();

    // 1. 创建客户端
    let config = ClientConfig::builder()
        .server_url("ws://localhost:60051".to_string())
        .device_id("example_device".to_string())
        .build()?;

    let client = FlareIMClient::new(config).await?;

    // 2. 订阅任务事件（监听任务执行情况）
    let event_bus = client.event_bus();
    let mut event_rx = event_bus.subscribe();
    tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            match event {
                // Blocking 任务事件
                Event::Task(TaskEvent::BlockingTaskStarted {
                    task_id, task_name, ..
                }) => {
                    println!("📢 [Event] Blocking 任务开始: {} ({})", task_name, task_id);
                }
                Event::Task(TaskEvent::BlockingTaskCompleted {
                    task_id,
                    task_name,
                    result,
                }) => {
                    println!(
                        "✅ [Event] Blocking 任务完成: {} ({}) - 会话: {}, 消息: {}, 耗时: {}ms",
                        task_name,
                        task_id,
                        result.session_count,
                        result.message_count,
                        result.duration_ms
                    );
                }
                Event::Task(TaskEvent::BlockingTaskFailed {
                    task_id,
                    task_name,
                    error,
                    ..
                }) => {
                    println!(
                        "❌ [Event] Blocking 任务失败: {} ({}) - {}",
                        task_name, task_id, error
                    );
                }

                // Background 任务事件
                Event::Task(TaskEvent::BackgroundTaskStarted {
                    task_id, task_name, ..
                }) => {
                    println!(
                        "📢 [Event] Background 任务开始: {} ({})",
                        task_name, task_id
                    );
                }
                Event::Task(TaskEvent::BackgroundTaskProgress {
                    task_id,
                    task_name,
                    progress,
                    ..
                }) => {
                    println!(
                        "📊 [Event] Background 任务进度: {} ({}) - {}%",
                        task_name, task_id, progress
                    );
                }
                Event::Task(TaskEvent::BackgroundTaskCompleted {
                    task_id,
                    task_name,
                    result,
                }) => {
                    println!(
                        "✅ [Event] Background 任务完成: {} ({}) - 会话: {}, 消息: {}, 耗时: {}ms",
                        task_name,
                        task_id,
                        result.session_count,
                        result.message_count,
                        result.duration_ms
                    );
                }
                Event::Task(TaskEvent::BackgroundTaskFailed {
                    task_id,
                    task_name,
                    error,
                    will_retry,
                    ..
                }) => {
                    if will_retry {
                        println!(
                            "⚠️  [Event] Background 任务失败（将重试）: {} ({}) - {}",
                            task_name, task_id, error
                        );
                    } else {
                        println!(
                            "❌ [Event] Background 任务失败（不再重试）: {} ({}) - {}",
                            task_name, task_id, error
                        );
                    }
                }
                Event::Task(TaskEvent::BackgroundTaskRetry {
                    task_id,
                    task_name,
                    retry_count,
                    ..
                }) => {
                    println!(
                        "🔄 [Event] Background 任务重试: {} ({}) - 第 {} 次重试",
                        task_name, task_id, retry_count
                    );
                }
                _ => {}
            }
        }
    });

    // 3. 注册任务
    let blocking_task = Arc::new(BlockingSessionSyncTask);
    client.register_task(blocking_task).await;

    let background_cache_task = Arc::new(BackgroundCacheWarmupTask::new(vec![
        "user:1001".to_string(),
        "user:1002".to_string(),
        "user:1003".to_string(),
    ]));
    client.register_task(background_cache_task).await;

    let background_index_task = Arc::new(BackgroundIndexBuildTask);
    client.register_task(background_index_task).await;

    println!("✅ 任务已注册");

    // 4. 登录（登录后会自动调度 SessionSync Blocking 任务）
    println!("\n=== 登录 ===");
    match client.login("user_123", "token_123").await {
        Ok(result) => {
            println!("✅ 登录成功: user_id={}", result.user_id);
            // 登录后会自动调度 SessionSync Blocking 任务
        }
        Err(e) => {
            eprintln!("❌ 登录失败: {}", e);
            return Ok(());
        }
    }

    // 等待 Blocking 任务完成
    sleep(Duration::from_millis(1000)).await;

    // 5. 手动调度 Blocking 任务（会立即执行，阻塞等待）
    println!("\n=== 调度 Blocking 任务 ===");
    match client
        .schedule_task_by_name("BlockingSessionSync", None)
        .await
    {
        Ok(task_id) => {
            println!("✅ Blocking 任务已调度: {}", task_id);
            // Blocking 任务会立即执行，阻塞等待完成
        }
        Err(e) => {
            eprintln!("❌ 调度 Blocking 任务失败: {}", e);
        }
    }

    // 6. 调度 Background 任务（异步执行，不阻塞）
    println!("\n=== 调度 Background 任务 ===");
    let background_tasks = vec!["BackgroundCacheWarmup", "BackgroundIndexBuild"];

    for task_name in background_tasks {
        match client.schedule_task_by_name(task_name, None).await {
            Ok(task_id) => {
                println!("✅ Background 任务已调度: {} ({})", task_name, task_id);
            }
            Err(e) => {
                eprintln!("❌ 调度 Background 任务失败: {} - {}", task_name, e);
            }
        }
    }

    // 7. 等待 Background 任务完成
    println!("\n=== 等待 Background 任务完成 ===");
    sleep(Duration::from_secs(5)).await;

    // 8. 获取任务调度器统计信息
    let stats = client.get_task_scheduler_stats().await;
    println!("\n=== 任务调度器统计 ===");
    println!("已注册任务数: {}", stats.registered_tasks);
    println!("待执行任务数: {}", stats.pending_tasks);
    println!("正在执行任务数: {}", stats.running_tasks);
    println!("是否启用: {}", stats.enabled);

    // 9. 登出
    client.logout().await?;
    println!("\n✅ 登出成功");

    Ok(())
}
