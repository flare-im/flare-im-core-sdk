//! 会话命令定义
//!
//! 定义所有会话相关的写操作命令

use crate::domain::conversation::InputStateType;

/// 标记会话消息已读命令
#[derive(Debug, Clone)]
pub struct MarkConversationReadCommand {
    pub conversation_id: String,
}

/// 标记所有会话消息已读命令
#[derive(Debug, Clone)]
pub struct MarkAllConversationsReadCommand {
}

/// 设置会话草稿命令
#[derive(Debug, Clone)]
pub struct SetConversationDraftCommand {
    pub conversation_id: String,
    pub draft: String,
}

/// 清除会话草稿命令
#[derive(Debug, Clone)]
pub struct ClearConversationDraftCommand {
    pub conversation_id: String,
}

/// 设置会话置顶命令
#[derive(Debug, Clone)]
pub struct PinConversationCommand {
    pub conversation_id: String,
    pub expire_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// 取消会话置顶命令
#[derive(Debug, Clone)]
pub struct UnpinConversationCommand {
    pub conversation_id: String,
}

/// 设置会话免打扰命令
#[derive(Debug, Clone)]
pub struct MuteConversationCommand {
    pub conversation_id: String,
    pub mute_until: Option<chrono::DateTime<chrono::Utc>>,
}

/// 取消会话免打扰命令
#[derive(Debug, Clone)]
pub struct UnmuteConversationCommand {
    pub conversation_id: String,
}

/// 设置会话输入状态命令
#[derive(Debug, Clone)]
pub struct SetInputStateCommand {
    pub conversation_id: String,
    pub state_type: InputStateType,
}

/// 清除会话输入状态命令
#[derive(Debug, Clone)]
pub struct ClearInputStateCommand {
    pub conversation_id: String,
}

/// 删除会话命令
#[derive(Debug, Clone)]
pub struct DeleteConversationCommand {
    pub conversation_id: String,
}

/// 隐藏会话命令
#[derive(Debug, Clone)]
pub struct HideConversationCommand {
    pub conversation_id: String,
}

/// 隐藏所有会话命令
#[derive(Debug, Clone)]
pub struct HideAllConversationCommand {
}

/// 清空会话消息命令
#[derive(Debug, Clone)]
pub struct ClearConversationMessagesCommand {
    pub conversation_id: String,
}

/// 设置会话信息命令
#[derive(Debug, Clone)]
pub struct SetConversationInfoCommand {
    pub conversation_id: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub description: Option<String>,
    pub announcement: Option<String>,
}
