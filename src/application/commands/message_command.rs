//! 消息命令定义
//!
//! 定义所有消息相关的写操作命令

use crate::domain::message::{Message, DeleteType, MarkType};

/// 发送消息命令
#[derive(Debug, Clone)]
pub struct SendMessageCommand {
    pub message: Message,
}

/// 撤回消息命令
#[derive(Debug, Clone)]
pub struct RecallMessageCommand {
    pub client_msg_id: String,
    pub reason: Option<String>,
}

/// 编辑消息命令
#[derive(Debug, Clone)]
pub struct EditMessageCommand {
    pub client_msg_id: String,
    pub new_content: Vec<u8>,
    pub reason: Option<String>,
}

/// 删除消息命令
#[derive(Debug, Clone)]
pub struct DeleteMessageCommand {
    pub client_msg_id: String,
    pub delete_type: DeleteType,
    pub reason: Option<String>,
}

/// 标记消息已读命令
#[derive(Debug, Clone)]
pub struct MarkMessagesReadCommand {
    pub message_ids: Vec<String>,
    pub burn_after_read: bool,
}

/// 回复消息命令
#[derive(Debug, Clone)]
pub struct ReplyMessageCommand {
    pub conversation_id: String,
    /// 被引用的消息ID（用于标识回复关系）
    pub quoted_message_id: String,
    /// 被引用消息的发送者ID（可选）
    pub quoted_sender_id: Option<String>,
    /// 引用内容预览（可选）
    pub quoted_text_preview: Option<String>,
    pub reply_content: Vec<u8>,
}

/// 转发消息命令
#[derive(Debug, Clone)]
pub struct ForwardMessagesCommand {
    pub message_ids: Vec<String>,
    pub target_conversation_id: String,
    pub merge_forward: bool,
}

/// 添加反应命令
#[derive(Debug, Clone)]
pub struct AddReactionCommand {
    pub message_id: String,
    pub emoji: String,
}

/// 移除反应命令
#[derive(Debug, Clone)]
pub struct RemoveReactionCommand {
    pub message_id: String,
    pub emoji: String,
}

/// 置顶消息命令
#[derive(Debug, Clone)]
pub struct PinMessageCommand {
    pub message_id: String,
    pub reason: Option<String>,
    pub expire_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// 取消置顶命令
#[derive(Debug, Clone)]
pub struct UnpinMessageCommand {
    pub message_id: String,
}

/// 收藏消息命令
#[derive(Debug, Clone)]
pub struct FavoriteMessageCommand {
    pub message_id: String,
    pub tags: Vec<String>,
    pub note: Option<String>,
}

/// 取消收藏命令
#[derive(Debug, Clone)]
pub struct UnfavoriteMessageCommand {
    pub message_id: String,
}

/// 标记消息命令
#[derive(Debug, Clone)]
pub struct MarkMessageCommand {
    pub message_id: String,
    pub mark_type: MarkType,
    pub color: Option<String>,
}

/// 线程回复命令
#[derive(Debug, Clone)]
pub struct AddThreadReplyCommand {
    pub conversation_id: String,
    pub thread_id: String,
    pub reply_content: Vec<u8>,
}

/// 批量标记已读命令
#[derive(Debug, Clone)]
pub struct BatchMarkMessageReadCommand {
    pub conversation_id: String,
    pub message_ids: Option<Vec<String>>, // None 表示标记会话中所有未读消息
    pub burn_after_read: bool,
}
