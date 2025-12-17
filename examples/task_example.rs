//! 任务调度器使用示例
//!
//! 演示如何注册自定义任务并通过任务调度器执行

use anyhow::Result;
use async_trait::async_trait;
use flare_im_core_sdk::{
    ClientConfig, FlareIMClient,
    infrastructure::task::{
        executor::SyncTaskExecutor,
        standard::{TaskExecutionMode, TaskResult, TaskType, priority},
    },
};
use std::sync::Arc;

/// 自定义任务示例：数据备份任务
struct DataBackupTask {
    backup_path: String,
}

impl DataBackupTask {
    fn new(backup_path: String) -> Self {
        Self { backup_path }
    }
}

#[async_trait]
impl SyncTaskExecutor for DataBackupTask {
    fn name(&self) -> &str {
        "DataBackup"
    }

    fn description(&self) -> &str {
        "数据备份任务：备份本地数据到指定路径"
    }

    fn task_type(&self) -> TaskType {
        TaskType::Background // 后台任务，可异步执行
    }

    fn priority(&self) -> u32 {
        priority::LOW // 低优先级，后台任务
    }

    fn execution_mode(&self) -> TaskExecutionMode {
        TaskExecutionMode::Async // 异步执行
    }

    async fn execute(
        &self,
        _context: &flare_im_core_sdk::infrastructure::task::executor::SyncContext,
    ) -> Result<TaskResult> {
        use std::time::Instant;
        let start_time = Instant::now();
        let task_id = format!("backup-{}", uuid::Uuid::new_v4());

        // 执行备份逻辑
        // 这里只是示例，实际应该调用 storage 进行备份
        println!("开始备份数据到: {}", self.backup_path);

        // 模拟备份操作
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        let duration_ms = start_time.elapsed().as_millis() as u64;
        println!("备份完成，耗时: {}ms", duration_ms);

        Ok(TaskResult::success(
            task_id,
            0, // 会话数量
            0, // 消息数量
            duration_ms,
        ))
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

    // 2. 注册自定义任务
    let backup_task = Arc::new(DataBackupTask::new("/tmp/backup".to_string()));
    client.register_task(backup_task).await;
    println!("✅ 自定义任务已注册");

    // 3. 查看已注册的任务
    let registered_tasks = client.get_registered_tasks().await;
    println!("已注册的任务: {:?}", registered_tasks);

    // 4. 登录（登录后会自动调度会话同步任务）
    println!("正在登录...");
    match client.login("user_123", "token_123").await {
        Ok(result) => {
            println!("✅ 登录成功: user_id={}", result.user_id);
        }
        Err(e) => {
            eprintln!("❌ 登录失败: {}", e);
            return Ok(());
        }
    }

    // 5. 等待登录后的任务执行
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // 6. 手动调度自定义任务
    println!("调度自定义任务...");
    match client.schedule_task_by_name("DataBackup", None).await {
        Ok(task_id) => {
            println!("✅ 任务已调度: task_id={}", task_id);

            // 7. 查询任务状态
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            if let Some(status) = client.get_task_status(&task_id).await {
                println!("任务状态: {:?}", status);
            }
        }
        Err(e) => {
            eprintln!("❌ 调度任务失败: {}", e);
        }
    }

    // 8. 获取任务调度器统计信息
    let stats = client.get_task_scheduler_stats().await;
    println!("任务调度器统计:");
    println!("  - 已注册任务数: {}", stats.registered_tasks);
    println!("  - 待执行任务数: {}", stats.pending_tasks);
    println!("  - 正在执行任务数: {}", stats.running_tasks);
    println!("  - 是否启用: {}", stats.enabled);

    // 9. 等待任务完成
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    // 10. 登出
    client.logout().await?;
    println!("✅ 登出成功");

    Ok(())
}
