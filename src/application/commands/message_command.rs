//! 消息命令定义
//!
//! 定义所有消息相关的写操作命令

use crate::domain::message::{Message, DeleteType, MarkType, TenantContext};

/// 发送消息命令
#[derive(Debug, Clone)]
pub struct SendMessageCommand {
    pub message: Message,
}

/// 撤回消息命令
#[derive(Debug, Clone)]
pub struct RecallMessageCommand {
    pub message_id: String,
    pub recaller_id: String,
    pub reason: Option<String>,
}

/// 编辑消息命令
#[derive(Debug, Clone)]
pub struct EditMessageCommand {
    pub message_id: String,
    pub editor_id: String,
    pub new_content: Vec<u8>,
    pub reason: Option<String>,
}

/// 删除消息命令
#[derive(Debug, Clone)]
pub struct DeleteMessageCommand {
    pub message_id: String,
    pub operator_id: String,
    pub delete_type: DeleteType,
    pub reason: Option<String>,
}

/// 标记消息已读命令
#[derive(Debug, Clone)]
pub struct MarkMessagesReadCommand {
    pub message_ids: Vec<String>,
    pub user_id: String,
    pub burn_after_read: bool,
}

/// 回复消息命令
#[derive(Debug, Clone)]
pub struct ReplyMessageCommand {
    pub conversation_id: String,
    pub sender_id: String,
    pub reply_to_message_id: String,
    pub reply_content: Vec<u8>,
    pub tenant: TenantContext,
}

/// 转发消息命令
#[derive(Debug, Clone)]
pub struct ForwardMessagesCommand {
    pub message_ids: Vec<String>,
    pub target_conversation_id: String,
    pub sender_id: String,
    pub merge_forward: bool,
    pub tenant: TenantContext,
}

/// 添加反应命令
#[derive(Debug, Clone)]
pub struct AddReactionCommand {
    pub message_id: String,
    pub user_id: String,
    pub emoji: String,
}

/// 移除反应命令
#[derive(Debug, Clone)]
pub struct RemoveReactionCommand {
    pub message_id: String,
    pub user_id: String,
    pub emoji: String,
}

/// 引用消息命令
#[derive(Debug, Clone)]
pub struct QuoteMessageCommand {
    pub conversation_id: String,
    pub sender_id: String,
    pub quoted_message_id: String,
    pub reply_content: Vec<u8>,
    pub tenant: TenantContext,
}

/// 置顶消息命令
#[derive(Debug, Clone)]
pub struct PinMessageCommand {
    pub message_id: String,
    pub operator_id: String,
    pub reason: Option<String>,
    pub expire_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// 取消置顶命令
#[derive(Debug, Clone)]
pub struct UnpinMessageCommand {
    pub message_id: String,
    pub operator_id: String,
}

/// 收藏消息命令
#[derive(Debug, Clone)]
pub struct FavoriteMessageCommand {
    pub message_id: String,
    pub operator_id: String,
    pub tags: Vec<String>,
    pub note: Option<String>,
}

/// 取消收藏命令
#[derive(Debug, Clone)]
pub struct UnfavoriteMessageCommand {
    pub message_id: String,
    pub operator_id: String,
}

/// 标记消息命令
#[derive(Debug, Clone)]
pub struct MarkMessageCommand {
    pub message_id: String,
    pub operator_id: String,
    pub mark_type: MarkType,
    pub color: Option<String>,
}
