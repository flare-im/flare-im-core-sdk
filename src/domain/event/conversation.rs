//! Conversation 领域事件
//!
//! 定义所有 Conversation 聚合根相关的领域事件

use serde::{Deserialize, Serialize};

/// Conversation 已创建
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationCreated {
    pub conversation_id: String,
    pub conversation_type: String,
}

/// Conversation 未读数已更新
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationUnreadUpdated {
    pub conversation_id: String,
    pub unread_count: u32,
}

/// Conversation 最后消息已更新
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationLastMessageUpdated {
    pub conversation_id: String,
    pub message_id: String,
    pub seq: u64,
}

/// Conversation 已标记为已读
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMarkedAsRead {
    pub conversation_id: String,
    pub user_id: String,
    pub unread_count: u32,
}

/// Conversation 草稿已更新
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationDraftUpdated {
    pub conversation_id: String,
    pub draft: Option<String>,
}

/// Conversation 已隐藏
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationHidden {
    pub conversation_id: String,
}

/// Conversation 所有会话已隐藏
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationAllHidden;

/// Conversation 已删除
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationDeleted {
    pub conversation_id: String,
    pub delete_messages: bool,
}

/// Conversation 消息已清空
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessagesCleared {
    pub conversation_id: String,
}

/// Conversation 信息已更新
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationUpdated {
    pub conversation_id: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub description: Option<String>,
    pub announcement: Option<String>,
}

/// Conversation 已静音
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMuted {
    pub conversation_id: String,
    pub mute_until: Option<chrono::DateTime<chrono::Utc>>,
}

/// Conversation 已取消静音
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationUnmuted {
    pub conversation_id: String,
}

/// Conversation 已置顶
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationPinned {
    pub conversation_id: String,
}

/// Conversation 已取消置顶
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationUnpinned {
    pub conversation_id: String,
}

/// Conversation 已归档
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationArchived {
    pub conversation_id: String,
}

/// Conversation 已取消归档
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationUnarchived {
    pub conversation_id: String,
}

/// Conversation 输入状态已更新
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationInputStateUpdated {
    pub conversation_id: String,
    pub user_id: String,
    pub state_type: String,
}

/// Conversation 输入状态已清除
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationInputStateCleared {
    pub conversation_id: String,
}
