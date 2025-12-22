//! 领域事件名称常量
//!
//! 所有领域事件类型名称的常量定义，避免使用魔法字符串

/// Session 相关事件名称
pub mod session_events {
    /// Session 已登录
    pub const LOGGED_IN: &str = "Session.LoggedIn";
    
    /// Session 已登出
    pub const LOGGED_OUT: &str = "Session.LoggedOut";
    
    /// Session 已过期
    pub const EXPIRED: &str = "Session.Expired";
    
    /// Session Token 已刷新
    pub const TOKEN_REFRESHED: &str = "Session.TokenRefreshed";
}

/// Connection 相关事件名称
pub mod connection_events {
    /// Connection 已连接
    pub const CONNECTED: &str = "Connection.Connected";
    
    /// Connection 已断开
    pub const DISCONNECTED: &str = "Connection.Disconnected";
    
    /// Connection 重连中
    pub const RECONNECTING: &str = "Connection.Reconnecting";
    
    /// Connection 重连成功
    pub const RECONNECTED: &str = "Connection.Reconnected";
    
    /// Connection 连接失败
    pub const CONNECT_FAILED: &str = "Connection.ConnectFailed";
}

/// Message 相关事件名称
pub mod message_events {
    /// Message 已创建
    pub const CREATED: &str = "Message.Created";
    
    /// Message 已发送
    pub const SENT: &str = "Message.Sent";
    
    /// Message 发送失败
    pub const SEND_FAILED: &str = "Message.SendFailed";
    
    /// Message 已送达
    pub const DELIVERED: &str = "Message.Delivered";
    
    /// Message 已读
    pub const READ: &str = "Message.Read";
    
    /// Message 已撤回
    pub const RECALLED: &str = "Message.Recalled";
    
    /// Message 已编辑
    pub const EDITED: &str = "Message.Edited";
    
    /// Message 已删除
    pub const DELETED: &str = "Message.Deleted";
    
    /// Message 已添加反应
    pub const REACTION_ADDED: &str = "Message.ReactionAdded";
    
    /// Message 已移除反应
    pub const REACTION_REMOVED: &str = "Message.ReactionRemoved";
    
    /// Message 已置顶
    pub const PINNED: &str = "Message.Pinned";
    
    /// Message 已取消置顶
    pub const UNPINNED: &str = "Message.Unpinned";
    
    /// Message 已收藏
    pub const FAVORITED: &str = "Message.Favorited";
    
    /// Message 已取消收藏
    pub const UNFAVORITED: &str = "Message.Unfavorited";
    
    /// Message 已标记
    pub const MARKED: &str = "Message.Marked";
    
    /// Message 已取消标记
    pub const UNMARKED: &str = "Message.Unmarked";
    
    /// Message 已转发
    pub const FORWARDED: &str = "Message.Forwarded";
    
    /// Message 已回复
    pub const REPLIED: &str = "Message.Replied";
}

/// Conversation 相关事件名称
pub mod conversation_events {
    /// Conversation 已创建
    pub const CREATED: &str = "Conversation.Created";
    
    /// Conversation 未读数已更新
    pub const UNREAD_UPDATED: &str = "Conversation.UnreadUpdated";
    
    /// Conversation 最后消息已更新
    pub const LAST_MESSAGE_UPDATED: &str = "Conversation.LastMessageUpdated";
    
    /// Conversation 已标记为已读
    pub const MARKED_AS_READ: &str = "Conversation.MarkedAsRead";
    
    /// Conversation 草稿已更新
    pub const DRAFT_UPDATED: &str = "Conversation.DraftUpdated";
    
    /// Conversation 已隐藏
    pub const HIDDEN: &str = "Conversation.Hidden";
    
    /// Conversation 所有会话已隐藏
    pub const ALL_HIDDEN: &str = "Conversation.AllHidden";
    
    /// Conversation 已删除
    pub const DELETED: &str = "Conversation.Deleted";
    
    /// Conversation 消息已清空
    pub const MESSAGES_CLEARED: &str = "Conversation.MessagesCleared";
    
    /// Conversation 信息已更新
    pub const UPDATED: &str = "Conversation.Updated";
    
    /// Conversation 已静音
    pub const MUTED: &str = "Conversation.Muted";
    
    /// Conversation 已取消静音
    pub const UNMUTED: &str = "Conversation.Unmuted";
    
    /// Conversation 已置顶
    pub const PINNED: &str = "Conversation.Pinned";
    
    /// Conversation 已取消置顶
    pub const UNPINNED: &str = "Conversation.Unpinned";
    
    /// Conversation 已归档
    pub const ARCHIVED: &str = "Conversation.Archived";
    
    /// Conversation 已取消归档
    pub const UNARCHIVED: &str = "Conversation.Unarchived";
    
    /// Conversation 输入状态已更新
    pub const INPUT_STATE_UPDATED: &str = "Conversation.InputStateUpdated";
    
    /// Conversation 输入状态已清除
    pub const INPUT_STATE_CLEARED: &str = "Conversation.InputStateCleared";
}

/// Sync 相关事件名称
pub mod sync_events {
    /// Sync Bootstrap 已开始
    pub const BOOTSTRAP_STARTED: &str = "Sync.BootstrapStarted";
    
    /// Sync Bootstrap 已完成
    pub const BOOTSTRAP_COMPLETED: &str = "Sync.BootstrapCompleted";
    
    /// Sync Bootstrap 已失败
    pub const BOOTSTRAP_FAILED: &str = "Sync.BootstrapFailed";
    
    /// Sync Async 已开始
    pub const ASYNC_STARTED: &str = "Sync.AsyncStarted";
    
    /// Sync Async 已完成
    pub const ASYNC_COMPLETED: &str = "Sync.AsyncCompleted";
    
    /// Sync Async 已失败
    pub const ASYNC_FAILED: &str = "Sync.AsyncFailed";
    
    /// Sync 进度更新
    pub const PROGRESS_UPDATED: &str = "Sync.ProgressUpdated";
}
