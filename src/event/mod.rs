pub mod bus;

pub use bus::EventBus;

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
}

/// 连接事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectionEvent {
    Connected { protocol: Option<flare_core::common::config_types::TransportProtocol> },
    Disconnected,
    Authenticated,
    AuthenticationFailed(String),
    Reconnecting,
    Reconnected,
    Error(String),
    ErrorWithCode { code: i32, message: String },
    FrameReceived(flare_core::common::protocol::Frame),
}

/// 消息事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageEvent {
    MessageReceived { message_id: String, session_id: String },
    MessageSent { message_id: String, session_id: String },
    MessageFailed { message_id: String, error: String },
    MessageRecalled { message_id: String, session_id: String },
    /// 消息状态更新（当收到 ACK 或状态变化时）
    MessageStatusUpdated { message_id: String, session_id: String, status: i32 },
}

/// 会话事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionEvent {
    SessionCreated { session_id: String },
    SessionUpdated { session_id: String },
    SessionDeleted { session_id: String },
    UnreadCountChanged { session_id: String, count: i32 },
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
    BackgroundSyncCompleted {
        sessions: usize,
        messages: usize,
    },
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
