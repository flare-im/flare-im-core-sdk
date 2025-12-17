//! 消息相关命令

use crate::domain::MessageContent;
use crate::domain::{MessageId, MessageType, SessionId, UserId};

/// 发送消息命令
#[derive(Debug, Clone)]
pub struct SendMessageCommand {
    pub session_id: SessionId,
    pub sender_id: UserId,
    pub receiver_id: Option<UserId>,
    pub channel_id: Option<String>,
    pub content: MessageContent,
    pub message_type: MessageType,
    pub seq: Option<i64>, // 消息序列号（可选，服务端分配）
}

/// 撤回消息命令
#[derive(Debug, Clone)]
pub struct RecallMessageCommand {
    pub message_id: MessageId,
    pub user_id: UserId,
    pub reason: Option<String>,
}

/// 删除消息命令
#[derive(Debug, Clone)]
pub struct DeleteMessageCommand {
    pub message_id: MessageId,
    pub user_id: UserId,
    pub delete_type: i32, // 0=软删除，1=硬删除
}

/// 编辑消息命令
#[derive(Debug, Clone)]
pub struct EditMessageCommand {
    pub message_id: MessageId,
    pub user_id: UserId,
    pub new_content: String,
}

/// 转发消息命令
#[derive(Debug, Clone)]
pub struct ForwardMessageCommand {
    pub message_id: MessageId,
    pub target_session_id: SessionId,
    pub sender_id: UserId,
}

/// 添加消息反应命令
#[derive(Debug, Clone)]
pub struct AddReactionCommand {
    pub message_id: MessageId,
    pub user_id: UserId,
    pub emoji: String,
}

/// 移除消息反应命令
#[derive(Debug, Clone)]
pub struct RemoveReactionCommand {
    pub message_id: MessageId,
    pub user_id: UserId,
    pub emoji: String,
}

/// 置顶消息命令
#[derive(Debug, Clone)]
pub struct PinMessageCommand {
    pub message_id: MessageId,
    pub user_id: UserId,
    pub expire_at: Option<prost_types::Timestamp>,
}

/// 取消置顶命令
#[derive(Debug, Clone)]
pub struct UnpinMessageCommand {
    pub message_id: MessageId,
    pub user_id: UserId,
}

/// 收藏消息命令
#[derive(Debug, Clone)]
pub struct FavoriteMessageCommand {
    pub message_id: MessageId,
    pub user_id: UserId,
    pub tags: Option<Vec<String>>,
    pub note: Option<String>,
}

/// 取消收藏命令
#[derive(Debug, Clone)]
pub struct UnfavoriteMessageCommand {
    pub message_id: MessageId,
    pub user_id: UserId,
}
