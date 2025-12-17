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
    ///
    /// **用于**：Blocking 任务（强制加载任务）
    /// **特点**：
    /// - 立即执行，不加入队列
    /// - 阻塞调用者，等待完成
    /// - 错误立即抛出
    Sync,

    /// 异步执行：在后台执行，不阻塞
    ///
    /// **用于**：Background 任务（后台慢加载任务）
    /// **特点**：
    /// - 加入队列，按优先级调度
    /// - 不阻塞调用者
    /// - 失败自动重试
    Async,
}

/// 任务类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskType {
    /// 强制加载任务（Blocking Task）：必须在主流程中立即完成
    ///
    /// **特性**：
    /// - 必须在主流程中立即完成，阻塞等待
    /// - 影响会话、消息的核心一致性与实时性
    /// - 错误必须立即抛出，不能忽略
    /// - 不能丢、不能延迟
    /// - 用于关键同步操作，如：登录后的会话列表同步、消息状态同步
    ///
    /// **执行方式**：
    /// - 同步执行（`TaskExecutionMode::Sync`）
    /// - 立即执行，不加入队列
    /// - 错误立即抛出给调用者
    Blocking,

    /// 后台慢加载任务（Background Task）：可异步执行，不阻塞主流程
    ///
    /// **特性**：
    /// - 可异步执行，不阻塞主流程
    /// - 用于非关键逻辑，如：缓存预热、索引构建、记录日志、推送历史消息、同步扩展模块元数据
    /// - 失败可自动重试，可延迟
    /// - 支持任务去重和优先级调度
    ///
    /// **执行方式**：
    /// - 异步执行（`TaskExecutionMode::Async`）
    /// - 加入队列，按优先级调度
    /// - 失败自动重试
    Background,
}
