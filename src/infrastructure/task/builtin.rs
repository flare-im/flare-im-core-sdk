//! 内置同步任务执行器
//!
//! 提供全量同步、消息同步、会话同步等内置任务

// 注意：旧版 SyncService 已废弃，应使用 SyncCommandHandler
// TODO: 重构 builtin tasks 使用新的 SyncCommandHandler
use crate::application::handlers::SyncCommandHandler;
use crate::infrastructure::task::executor::{SyncContext, SyncTaskExecutor};
use crate::infrastructure::task::standard::{TaskExecutionMode, TaskResult, TaskType, priority};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::time::Instant;

// ============================================================================
// 全量同步任务
// ============================================================================

/// 全量同步任务执行器（强制任务）
///
/// **优先级**: `priority::HIGH` (10)
/// **类型**: `Mandatory` (强制)
/// **执行模式**: `Async` (异步)
pub struct FullSyncTask {
    sync_command_handler: Arc<SyncCommandHandler>,
}

impl FullSyncTask {
    pub fn new(sync_command_handler: Arc<SyncCommandHandler>) -> Self {
        Self {
            sync_command_handler,
        }
    }
}

#[async_trait]
impl SyncTaskExecutor for FullSyncTask {
    fn name(&self) -> &str {
        "FullSync"
    }

    fn description(&self) -> &str {
        "全量同步：同步所有会话和最近消息"
    }

    fn task_type(&self) -> TaskType {
        TaskType::Background // 全量同步是后台任务（可以异步执行）
    }

    fn priority(&self) -> u32 {
        priority::HIGH // 高优先级
    }

    fn execution_mode(&self) -> TaskExecutionMode {
        TaskExecutionMode::Async // 可以异步执行
    }

    async fn execute(&self, _context: &SyncContext) -> Result<TaskResult> {
        let start_time = Instant::now();
        let task_id = format!("full-sync-{}", uuid::Uuid::new_v4());

        // 使用 SyncCommandHandler 实现全量同步
        // 按照微信/Telegram/飞书标准：全量同步拉取最近 50 条消息
        use crate::application::commands::sync::SyncMessagesCommand;
        use crate::domain::SyncType;

        match self
            .sync_command_handler
            .handle_sync_messages(SyncMessagesCommand {
                session_id: None, // 全量同步所有会话
                sync_type: SyncType::Full,
                after_seq: None,
            })
            .await
        {
            Ok(result) => {
                let duration_ms = start_time.elapsed().as_millis() as u64;
                Ok(TaskResult::success(
                    task_id,
                    0, // session_count
                    result.message_count,
                    duration_ms,
                ))
            }
            Err(e) => {
                let duration_ms = start_time.elapsed().as_millis() as u64;
                Ok(TaskResult::failure(
                    task_id,
                    format!("Full sync failed: {}", e),
                    duration_ms,
                ))
            }
        }
    }
}

// ============================================================================
// 消息同步任务
// ============================================================================

/// 消息同步任务执行器（可选任务）
///
/// **优先级**: `priority::MEDIUM` (50)
/// **类型**: `Optional` (可选)
/// **执行模式**: `Async` (异步)
pub struct MessageSyncTask {
    sync_command_handler: Arc<SyncCommandHandler>,
    session_id: String,
    after_seq: Option<i64>,
}

impl MessageSyncTask {
    pub fn new(
        sync_command_handler: Arc<SyncCommandHandler>,
        session_id: String,
        after_seq: Option<i64>,
    ) -> Self {
        Self {
            sync_command_handler,
            session_id,
            after_seq,
        }
    }
}

#[async_trait]
impl SyncTaskExecutor for MessageSyncTask {
    fn name(&self) -> &str {
        "MessageSync"
    }

    fn description(&self) -> &str {
        "同步会话消息"
    }

    fn task_type(&self) -> TaskType {
        TaskType::Background // 消息同步是后台任务（可以异步执行）
    }

    fn priority(&self) -> u32 {
        priority::MEDIUM // 中等优先级
    }

    fn execution_mode(&self) -> TaskExecutionMode {
        TaskExecutionMode::Async // Background 任务异步执行
    }

    async fn execute(&self, _context: &SyncContext) -> Result<TaskResult> {
        let start_time = Instant::now();
        let task_id = format!("message-sync-{}-{}", self.session_id, uuid::Uuid::new_v4());

        // 使用 SyncCommandHandler 实现消息同步
        // 按照微信/Telegram/飞书标准：增量同步基于 last_seq
        use crate::application::commands::sync::SyncMessagesCommand;
        use crate::domain::{SessionId, SyncType};

        match self
            .sync_command_handler
            .handle_sync_messages(SyncMessagesCommand {
                session_id: Some(SessionId::new(self.session_id.clone())),
                sync_type: SyncType::Incremental,
                after_seq: self.after_seq,
            })
            .await
        {
            Ok(result) => {
                let duration_ms = start_time.elapsed().as_millis() as u64;
                Ok(TaskResult::success(
                    task_id,
                    0, // session_count
                    result.message_count,
                    duration_ms,
                ))
            }
            Err(e) => {
                let duration_ms = start_time.elapsed().as_millis() as u64;
                Ok(TaskResult::failure(
                    task_id,
                    format!("Message sync failed: {}", e),
                    duration_ms,
                ))
            }
        }
    }
}

// ============================================================================
// 会话同步任务
// ============================================================================

/// 会话同步任务执行器（可选任务）
///
/// **优先级**: `priority::MEDIUM_HIGH` (30)
/// **类型**: `Optional` (可选)
/// **执行模式**: `Async` (异步)
pub struct SessionSyncTask {
    sync_command_handler: Arc<SyncCommandHandler>,
    cursor: Option<String>,
}

impl SessionSyncTask {
    pub fn new(sync_command_handler: Arc<SyncCommandHandler>, cursor: Option<String>) -> Self {
        Self {
            sync_command_handler,
            cursor,
        }
    }
}

#[async_trait]
impl SyncTaskExecutor for SessionSyncTask {
    fn name(&self) -> &str {
        "SessionSync"
    }

    fn description(&self) -> &str {
        "同步会话列表"
    }

    fn task_type(&self) -> TaskType {
        TaskType::Blocking // 会话同步是强制加载任务（必须在登录后立即完成）
    }

    fn priority(&self) -> u32 {
        priority::MEDIUM_HIGH // 中高优先级
    }

    fn execution_mode(&self) -> TaskExecutionMode {
        TaskExecutionMode::Sync // Blocking 任务必须同步执行
    }

    async fn execute(&self, _context: &SyncContext) -> Result<TaskResult> {
        let start_time = Instant::now();
        let task_id = format!("session-sync-{}", uuid::Uuid::new_v4());

        // 使用 SyncCommandHandler 实现会话同步
        // 按照微信/Telegram/飞书标准：分页同步使用 cursor
        use crate::application::commands::sync::SyncSessionsCommand;

        match self
            .sync_command_handler
            .handle_sync_sessions(SyncSessionsCommand {
                cursor: self.cursor.clone(),
            })
            .await
        {
            Ok(result) => {
                let duration_ms = start_time.elapsed().as_millis() as u64;
                Ok(TaskResult::success(
                    task_id,
                    result.count, // session_count
                    0,            // message_count
                    duration_ms,
                ))
            }
            Err(e) => {
                let duration_ms = start_time.elapsed().as_millis() as u64;
                Ok(TaskResult::failure(
                    task_id,
                    format!("Session sync failed: {}", e),
                    duration_ms,
                ))
            }
        }
    }
}
