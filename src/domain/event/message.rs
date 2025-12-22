//! Message 领域事件
//!
//! 定义所有 Message 聚合根相关的领域事件

use serde::{Deserialize, Serialize};

/// Message 已创建
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageCreated {
    pub message_id: String,
    pub conversation_id: String,
    pub sender_id: String,
    pub content: serde_json::Value,
}

/// Message 已发送
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSent {
    pub message_id: String,
    pub seq: u64,
}

/// Message 发送失败
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSendFailed {
    pub message_id: String,
    pub error: String,
}

/// Message 已送达
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDelivered {
    pub message_id: String,
}

/// Message 已读
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRead {
    pub message_id: String,
    pub reader_id: String,
}

/// Message 已撤回
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRecalled {
    pub message_id: String,
    pub recaller_id: String,
}

/// Message 已编辑
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEdited {
    pub message_id: String,
    pub editor_id: String,
    pub new_content: serde_json::Value,
}

/// Message 已删除
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDeleted {
    pub message_id: String,
    pub operator_id: String,
    pub delete_type: String, // "soft" or "hard"
}

/// Message 已添加反应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReactionAdded {
    pub message_id: String,
    pub emoji: String,
    pub user_id: String,
}

/// Message 已移除反应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReactionRemoved {
    pub message_id: String,
    pub emoji: String,
    pub user_id: String,
}

/// Message 已置顶
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePinned {
    pub message_id: String,
    pub operator_id: String,
}

/// Message 已取消置顶
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageUnpinned {
    pub message_id: String,
    pub operator_id: String,
}

/// Message 已收藏
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageFavorited {
    pub message_id: String,
    pub user_id: String,
}

/// Message 已取消收藏
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageUnfavorited {
    pub message_id: String,
    pub user_id: String,
}

/// Message 已标记
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMarked {
    pub message_id: String,
    pub user_id: String,
    pub mark_type: String,
}

/// Message 已取消标记
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageUnmarked {
    pub message_id: String,
    pub user_id: String,
}

/// Message 已转发
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageForwarded {
    pub message_id: String,
    pub forwarder_id: String,
    pub target_conversation_id: String,
}

/// Message 已回复
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReplied {
    pub message_id: String,
    pub reply_to_message_id: String,
    pub replier_id: String,
}
