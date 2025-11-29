//! 内置同步任务执行器
//!
//! 提供全量同步、消息同步、会话同步等内置任务

use crate::service::sync::SyncService;
use crate::task::executor::{SyncTaskExecutor, SyncContext};
use crate::task::standard::{TaskResult, TaskType, TaskExecutionMode, priority};
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
    sync_service: Arc<SyncService>,
}

impl FullSyncTask {
    pub fn new(sync_service: Arc<SyncService>) -> Self {
        Self { sync_service }
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
        TaskType::Mandatory  // 全量同步是强制任务
    }
    
    fn priority(&self) -> u32 {
        priority::HIGH  // 高优先级
    }
    
    fn execution_mode(&self) -> TaskExecutionMode {
        TaskExecutionMode::Async  // 可以异步执行
    }
    
    async fn execute(&self, _context: &SyncContext) -> Result<TaskResult> {
        let start_time = Instant::now();
        let task_id = format!("full-sync-{}", uuid::Uuid::new_v4());
        
        // 直接调用全量同步，避免任务系统循环
        // 注意：这里需要临时禁用任务系统，然后调用 full_sync
        // 或者 SyncService 应该提供内部方法
        // 为了简化，我们直接调用，但需要确保任务系统不会循环
                match self.sync_service.full_sync_internal().await {
            Ok(result) => {
                let duration_ms = start_time.elapsed().as_millis() as u64;
                Ok(TaskResult::success(
                    task_id,
                    result.session_count,
                    result.total_message_count,
                    duration_ms,
                ))
            }
            Err(e) => {
                let duration_ms = start_time.elapsed().as_millis() as u64;
                Ok(TaskResult::failure(
                    task_id,
                    e.to_string(),
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
    sync_service: Arc<SyncService>,
    session_id: String,
    after_seq: Option<i64>,
}

impl MessageSyncTask {
    pub fn new(
        sync_service: Arc<SyncService>,
        session_id: String,
        after_seq: Option<i64>,
    ) -> Self {
        Self {
            sync_service,
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
        TaskType::Optional  // 消息同步是可选任务
    }
    
    fn priority(&self) -> u32 {
        priority::MEDIUM  // 中等优先级
    }
    
    fn execution_mode(&self) -> TaskExecutionMode {
        TaskExecutionMode::Async  // 可以异步执行
    }
    
    async fn execute(&self, _context: &SyncContext) -> Result<TaskResult> {
        let start_time = Instant::now();
        let task_id = format!("message-sync-{}-{}", self.session_id, uuid::Uuid::new_v4());
        
        // 直接调用内部方法，避免任务系统循环调用
        // 注意：不能调用 sync_messages，因为它会检查任务系统并再次调用 sync_messages_via_task
        match self.sync_service.sync_messages_internal(&self.session_id, self.after_seq).await {
            Ok(result) => {
                let duration_ms = start_time.elapsed().as_millis() as u64;
                Ok(TaskResult::success(
                    task_id,
                    0,  // 会话数量
                    result.message_count,
                    duration_ms,
                ))
            }
            Err(e) => {
                let duration_ms = start_time.elapsed().as_millis() as u64;
                Ok(TaskResult::failure(
                    task_id,
                    e.to_string(),
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
    sync_service: Arc<SyncService>,
    cursor: Option<String>,
}

impl SessionSyncTask {
    pub fn new(sync_service: Arc<SyncService>, cursor: Option<String>) -> Self {
        Self {
            sync_service,
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
        TaskType::Optional  // 会话同步是可选任务
    }
    
    fn priority(&self) -> u32 {
        priority::MEDIUM_HIGH  // 中高优先级
    }
    
    fn execution_mode(&self) -> TaskExecutionMode {
        TaskExecutionMode::Async  // 可以异步执行
    }
    
    async fn execute(&self, _context: &SyncContext) -> Result<TaskResult> {
        let start_time = Instant::now();
        let task_id = format!("session-sync-{}", uuid::Uuid::new_v4());
        
        // 直接调用内部方法，避免任务系统循环调用
        // 注意：不能调用 sync_sessions，因为它会检查任务系统并再次调用 sync_sessions_via_task
        match self.sync_service.sync_sessions_internal(self.cursor.clone()).await {
            Ok(result) => {
                let duration_ms = start_time.elapsed().as_millis() as u64;
                Ok(TaskResult::success(
                    task_id,
                    result.count,
                    0,  // 消息数量
                    duration_ms,
                ))
            }
            Err(e) => {
                let duration_ms = start_time.elapsed().as_millis() as u64;
                Ok(TaskResult::failure(
                    task_id,
                    e.to_string(),
                    duration_ms,
                ))
            }
        }
    }
}

