//! 事件类型定义

use serde::{Deserialize, Serialize};

/// 事件类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    /// 连接事件
    Connection(ConnectionEvent),

    /// 消息事件
    Message(MessageEvent),

    /// 会话事件
    Session(SessionEvent),

    /// 同步事件
    Sync(SyncEvent),

    /// 任务事件（新增：用于任务执行状态通知）
    Task(TaskEvent),
}

/// 连接事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectionEvent {
    Connected {
        protocol: Option<flare_core::common::config_types::TransportProtocol>,
    },
    Disconnected,
    /// 被踢下线（设备冲突等），不应该自动重连
    Kicked {
        reason: String,
    },
    Authenticated,
    AuthenticationFailed(String),
    Reconnecting,
    Reconnected,
    Error(String),
    ErrorWithCode {
        code: i32,
        message: String,
    },
    FrameReceived(flare_core::common::protocol::Frame),
}

/// 消息事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageEvent {
    MessageCreated {
        message_id: String,
        session_id: String,
    },
    MessageReceived {
        message_id: String,
        session_id: String,
    },
    MessageSent {
        message_id: String,
        session_id: String,
    },
    MessageFailed {
        message_id: String,
        error: String,
    },
    MessageRecalled {
        message_id: String,
        session_id: String,
    },
    MessageDeleted {
        message_id: String,
        session_id: String,
    },
    MessageRead {
        message_id: String,
        session_id: String,
        user_id: String,
    },
    /// 消息状态更新（当收到 ACK 或状态变化时）
    MessageStatusUpdated {
        message_id: String,
        session_id: String,
        status: i32,
    },
    /// 消息已编辑
    MessageEdited {
        message_id: String,
        session_id: String,
    },
    /// 消息反应已添加
    MessageReactionAdded {
        message_id: String,
        session_id: String,
        user_id: String,
        emoji: String,
    },
    /// 消息反应已移除
    MessageReactionRemoved {
        message_id: String,
        session_id: String,
        user_id: String,
        emoji: String,
    },
    /// 消息已置顶
    MessagePinned {
        message_id: String,
        session_id: String,
        user_id: String,
    },
    /// 消息已取消置顶
    MessageUnpinned {
        message_id: String,
        session_id: String,
        user_id: String,
    },
    /// 消息已收藏
    MessageFavorited {
        message_id: String,
        session_id: String,
        user_id: String,
    },
    /// 消息已取消收藏
    MessageUnfavorited {
        message_id: String,
        session_id: String,
        user_id: String,
    },
}

/// 会话事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionEvent {
    SessionCreated {
        session_id: String,
    },
    SessionUpdated {
        session_id: String,
    },
    SessionDeleted {
        session_id: String,
    },
    UnreadCountChanged {
        session_id: String,
        count: i32,
    },
    /// 会话已标记为已读
    SessionMarkedRead {
        session_id: String,
        message_seq: i64,
    },
    /// 会话草稿已设置
    SessionDraftSet {
        session_id: String,
        draft: String,
    },
    /// 会话已隐藏
    SessionHidden {
        session_id: String,
    },
    /// 会话已显示
    SessionShown {
        session_id: String,
    },
    /// 会话输入状态已发送
    SessionTypingSent {
        session_id: String,
        user_id: String,
        is_typing: bool,
    },
}

/// 同步事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncEvent {
    /// 同步开始
    SyncStarted {
        /// 同步类型（全量/增量）
        sync_type: String,
        /// 预计需要同步的会话数（如果已知）
        estimated_sessions: Option<usize>,
    },
    /// 同步进度更新
    SyncProgress {
        /// 当前进度（0-100）
        progress: u8,
        /// 已同步会话数
        sessions_synced: usize,
        /// 已同步消息数
        messages_synced: usize,
        /// 当前正在同步的会话ID（如果有）
        current_session_id: Option<String>,
        /// 预计剩余时间（秒，如果可计算）
        estimated_remaining_seconds: Option<u64>,
    },
    /// 会话同步开始
    SessionSyncStarted {
        session_id: String,
        session_type: String,
        /// 预计需要同步的消息数（如果已知）
        estimated_messages: Option<usize>,
    },
    /// 会话同步进度
    SessionSyncProgress {
        session_id: String,
        /// 当前进度（0-100）
        progress: u8,
        /// 已同步消息数
        messages_synced: usize,
        /// 预计剩余时间（秒）
        estimated_remaining_seconds: Option<u64>,
    },
    /// 会话同步完成
    SessionSyncCompleted {
        session_id: String,
        messages_synced: usize,
        duration_ms: u64,
    },
    /// 同步阶段完成（用于渐进式同步）
    SyncPhaseCompleted {
        phase: SyncPhase,
        sessions: usize,
        messages: usize,
    },
    /// 同步完成
    SyncCompleted {
        sessions: usize,
        messages: usize,
        duration_ms: u64,
        /// 是否有后台同步任务
        has_background_sync: bool,
    },
    /// 同步失败
    SyncFailed {
        error: String,
        /// 已同步的会话数（部分成功）
        sessions_synced: Option<usize>,
        /// 已同步的消息数（部分成功）
        messages_synced: Option<usize>,
    },
    /// 后台同步开始（渐进式同步的第二阶段）
    BackgroundSyncStarted {
        /// 需要后台同步的会话数
        sessions_count: usize,
    },
    /// 后台同步完成
    BackgroundSyncCompleted { sessions: usize, messages: usize },
}

/// 同步阶段（用于渐进式同步）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncPhase {
    /// 第一阶段：快速同步（最近消息）
    QuickSync,
    /// 第二阶段：完整同步（历史消息）
    FullSync,
    /// 第三阶段：后台同步（非活跃会话）
    BackgroundSync,
}

/// 任务事件（新增：用于任务执行状态通知和回调）
///
/// 客户端可以通过订阅这些事件来了解任务执行情况并定制自己的逻辑
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskEvent {
    /// 强制加载任务（Blocking Task）开始
    ///
    /// **触发时机**：Blocking 任务开始执行时
    /// **用途**：通知客户端关键任务开始，可以显示加载状态
    BlockingTaskStarted {
        /// 任务 ID
        task_id: String,
        /// 任务名称
        task_name: String,
        /// 任务描述
        task_description: String,
    },

    /// 强制加载任务（Blocking Task）完成
    ///
    /// **触发时机**：Blocking 任务成功完成时
    /// **用途**：通知客户端关键任务完成，可以更新 UI
    BlockingTaskCompleted {
        /// 任务 ID
        task_id: String,
        /// 任务名称
        task_name: String,
        /// 任务结果
        result: crate::infrastructure::task::standard::TaskResult,
    },

    /// 强制加载任务（Blocking Task）失败
    ///
    /// **触发时机**：Blocking 任务失败时（错误会立即抛出给调用者）
    /// **用途**：通知客户端关键任务失败，可以显示错误信息
    BlockingTaskFailed {
        /// 任务 ID
        task_id: String,
        /// 任务名称
        task_name: String,
        /// 错误信息
        error: String,
        /// 执行耗时（毫秒）
        duration_ms: u64,
    },

    /// 后台慢加载任务（Background Task）开始
    ///
    /// **触发时机**：Background 任务开始执行时
    /// **用途**：通知客户端后台任务开始，可以记录日志或更新统计
    BackgroundTaskStarted {
        /// 任务 ID
        task_id: String,
        /// 任务名称
        task_name: String,
        /// 任务描述
        task_description: String,
    },

    /// 后台慢加载任务（Background Task）进度更新
    ///
    /// **触发时机**：Background 任务执行过程中（如果任务支持进度报告）
    /// **用途**：通知客户端后台任务进度，可以更新进度条
    BackgroundTaskProgress {
        /// 任务 ID
        task_id: String,
        /// 任务名称
        task_name: String,
        /// 当前进度（0-100）
        progress: u8,
        /// 进度描述（可选）
        message: Option<String>,
    },

    /// 后台慢加载任务（Background Task）完成
    ///
    /// **触发时机**：Background 任务成功完成时
    /// **用途**：通知客户端后台任务完成，可以更新缓存或索引
    BackgroundTaskCompleted {
        /// 任务 ID
        task_id: String,
        /// 任务名称
        task_name: String,
        /// 任务结果
        result: crate::infrastructure::task::standard::TaskResult,
    },

    /// 后台慢加载任务（Background Task）失败
    ///
    /// **触发时机**：Background 任务失败时（会自动重试）
    /// **用途**：通知客户端后台任务失败，可以记录错误日志
    BackgroundTaskFailed {
        /// 任务 ID
        task_id: String,
        /// 任务名称
        task_name: String,
        /// 错误信息
        error: String,
        /// 当前重试次数
        retry_count: u32,
        /// 最大重试次数
        max_retries: u32,
        /// 是否还会重试
        will_retry: bool,
    },

    /// 后台慢加载任务（Background Task）重试
    ///
    /// **触发时机**：Background 任务失败后开始重试时
    /// **用途**：通知客户端任务正在重试
    BackgroundTaskRetry {
        /// 任务 ID
        task_id: String,
        /// 任务名称
        task_name: String,
        /// 当前重试次数
        retry_count: u32,
        /// 最大重试次数
        max_retries: u32,
        /// 重试延迟（毫秒）
        retry_delay_ms: u64,
    },
}
