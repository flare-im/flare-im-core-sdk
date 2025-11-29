//! 同步任务标准定义
//!
//! 定义任务优先级、状态、结果等标准

use serde::{Deserialize, Serialize};

/// 任务优先级标准
/// 
/// 数字越小优先级越高
pub mod priority {
    /// 最高优先级（0-9）：关键系统任务
    pub const CRITICAL: u32 = 0;
    
    /// 高优先级（10-29）：重要同步任务
    pub const HIGH: u32 = 10;
    
    /// 中高优先级（30-49）：常规同步任务
    pub const MEDIUM_HIGH: u32 = 30;
    
    /// 中等优先级（50-69）：普通同步任务
    pub const MEDIUM: u32 = 50;
    
    /// 低优先级（70-89）：后台同步任务
    pub const LOW: u32 = 70;
    
    /// 最低优先级（90+）：可延迟任务
    pub const LOWEST: u32 = 90;
    
    /// 默认优先级
    pub const DEFAULT: u32 = 100;
}

/// 同步任务状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// 等待中
    Pending,
    
    /// 执行中
    Running,
    
    /// 已完成
    Completed,
    
    /// 失败
    Failed,
    
    /// 已取消
    Cancelled,
}

/// 同步任务结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    /// 任务 ID
    pub task_id: String,
    
    /// 是否成功
    pub success: bool,
    
    /// 同步的会话数量
    pub session_count: usize,
    
    /// 同步的消息数量
    pub message_count: usize,
    
    /// 错误信息（如果失败）
    pub error: Option<String>,
    
    /// 执行耗时（毫秒）
    pub duration_ms: u64,
    
    /// 开始时间戳（毫秒）
    pub start_time_ms: Option<u64>,
    
    /// 结束时间戳（毫秒）
    pub end_time_ms: Option<u64>,
}

impl TaskResult {
    /// 创建成功结果
    pub fn success(
        task_id: String,
        session_count: usize,
        message_count: usize,
        duration_ms: u64,
    ) -> Self {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        
        Self {
            task_id,
            success: true,
            session_count,
            message_count,
            error: None,
            duration_ms,
            start_time_ms: Some(now_ms.saturating_sub(duration_ms)),
            end_time_ms: Some(now_ms),
        }
    }
    
    /// 创建失败结果
    pub fn failure(task_id: String, error: String, duration_ms: u64) -> Self {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        
        Self {
            task_id,
            success: false,
            session_count: 0,
            message_count: 0,
            error: Some(error),
            duration_ms,
            start_time_ms: Some(now_ms.saturating_sub(duration_ms)),
            end_time_ms: Some(now_ms),
        }
    }
}

/// 任务执行模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskExecutionMode {
    /// 同步执行：在当前上下文中执行，阻塞等待完成
    Sync,
    
    /// 异步执行：在后台执行，不阻塞
    Async,
}

/// 任务类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskType {
    /// 强制任务：必须执行
    Mandatory,
    
    /// 可选任务：由客户端决定是否执行
    Optional,
}

