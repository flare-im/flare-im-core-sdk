//! 会话相关命令

use crate::domain::message::model::SessionId;
use std::collections::HashMap;

/// 创建会话命令
#[derive(Debug, Clone)]
pub struct CreateSessionCommand {
    pub session_id: Option<SessionId>,
    pub session_type: String,
    pub business_type: String,
    pub display_name: Option<String>,
    pub participants: Vec<String>,
}

/// 更新会话命令
#[derive(Debug, Clone)]
pub struct UpdateSessionCommand {
    pub session_id: SessionId,
    pub updates: HashMap<String, String>,
}

/// 删除会话命令
#[derive(Debug, Clone)]
pub struct DeleteSessionCommand {
    pub session_id: SessionId,
    pub delete_messages: bool,
}

/// 隐藏会话命令
#[derive(Debug, Clone)]
pub struct HideSessionCommand {
    pub session_id: SessionId,
}

/// 显示会话命令
#[derive(Debug, Clone)]
pub struct ShowSessionCommand {
    pub session_id: SessionId,
}

/// 标记已读命令
#[derive(Debug, Clone)]
pub struct MarkReadCommand {
    pub session_id: SessionId,
    pub message_seq: Option<i64>,
}

/// 批量标记已读命令
#[derive(Debug, Clone)]
pub struct MarkReadBatchCommand {
    pub session_ids: Vec<SessionId>,
}

/// 设置草稿命令
#[derive(Debug, Clone)]
pub struct SetDraftCommand {
    pub session_id: SessionId,
    pub draft: Option<String>,
}

/// 发送输入状态命令
#[derive(Debug, Clone)]
pub struct SendTypingCommand {
    pub session_id: SessionId,
    pub user_id: crate::domain::message::model::UserId,
    pub is_typing: bool,
}
