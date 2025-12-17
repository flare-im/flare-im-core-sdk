//! 消息领域事件
//!
//! 表示消息相关的业务事件
//!
//! ## 事件设计原则
//!
//! 1. **不可变**：事件一旦创建就不能修改
//! 2. **时间戳**：所有事件都包含时间戳
//! 3. **聚合根ID**：包含相关的聚合根ID（message_id, session_id等）
//! 4. **业务语义**：事件名称清晰表达业务含义

use crate::domain::message::model::{MessageId, SessionId, UserId};
use chrono::DateTime;
use chrono::Utc;

/// 消息已发送事件
#[derive(Debug, Clone)]
pub struct MessageSentEvent {
    pub message_id: MessageId,
    pub session_id: SessionId,
    pub sender_id: UserId,
    pub timestamp: DateTime<Utc>,
}

/// 消息已接收事件
#[derive(Debug, Clone)]
pub struct MessageReceivedEvent {
    pub message_id: MessageId,
    pub session_id: SessionId,
    pub receiver_id: UserId,
    pub timestamp: DateTime<Utc>,
}

/// 消息已撤回事件
#[derive(Debug, Clone)]
pub struct MessageRecalledEvent {
    pub message_id: MessageId,
    pub session_id: SessionId,
    pub reason: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// 消息已删除事件
#[derive(Debug, Clone)]
pub struct MessageDeletedEvent {
    pub message_id: MessageId,
    pub session_id: SessionId,
    pub delete_type: i32, // 0=软删除, 1=硬删除
    pub timestamp: DateTime<Utc>,
}

/// 消息已读事件
#[derive(Debug, Clone)]
pub struct MessageReadEvent {
    pub message_id: MessageId,
    pub session_id: SessionId,
    pub reader_id: UserId,
    pub timestamp: DateTime<Utc>,
}

/// 消息已编辑事件
#[derive(Debug, Clone)]
pub struct MessageEditedEvent {
    pub message_id: MessageId,
    pub session_id: SessionId,
    pub editor_id: UserId,
    pub new_content: String,
    pub timestamp: DateTime<Utc>,
}

/// 消息已转发事件
#[derive(Debug, Clone)]
pub struct MessageForwardedEvent {
    pub original_message_id: MessageId,
    pub original_session_id: SessionId,
    pub new_message_id: MessageId,
    pub target_session_id: SessionId,
    pub forwarder_id: UserId,
    pub timestamp: DateTime<Utc>,
}

/// 消息反应已添加事件
#[derive(Debug, Clone)]
pub struct MessageReactionAddedEvent {
    pub message_id: MessageId,
    pub session_id: SessionId,
    pub user_id: UserId,
    pub emoji: String,
    pub timestamp: DateTime<Utc>,
}

/// 消息反应已移除事件
#[derive(Debug, Clone)]
pub struct MessageReactionRemovedEvent {
    pub message_id: MessageId,
    pub session_id: SessionId,
    pub user_id: UserId,
    pub emoji: String,
    pub timestamp: DateTime<Utc>,
}

/// 消息已置顶事件
#[derive(Debug, Clone)]
pub struct MessagePinnedEvent {
    pub message_id: MessageId,
    pub session_id: SessionId,
    pub user_id: UserId,
    pub expire_at: Option<prost_types::Timestamp>,
    pub timestamp: DateTime<Utc>,
}

/// 消息已取消置顶事件
#[derive(Debug, Clone)]
pub struct MessageUnpinnedEvent {
    pub message_id: MessageId,
    pub session_id: SessionId,
    pub user_id: UserId,
    pub timestamp: DateTime<Utc>,
}

/// 消息已收藏事件
#[derive(Debug, Clone)]
pub struct MessageFavoritedEvent {
    pub message_id: MessageId,
    pub session_id: SessionId,
    pub user_id: UserId,
    pub tags: Option<Vec<String>>,
    pub note: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// 消息已取消收藏事件
#[derive(Debug, Clone)]
pub struct MessageUnfavoritedEvent {
    pub message_id: MessageId,
    pub session_id: SessionId,
    pub user_id: UserId,
    pub timestamp: DateTime<Utc>,
}
