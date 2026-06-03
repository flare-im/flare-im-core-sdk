//! 强类型 ID — 区分 UserId / ConversationId，防止非法状态
//!
//! 使用 Newtype 模式，避免将 `user_id` 与 `conversation_id` 混用。

use std::fmt;

/// 用户 ID（发送者、接收者、操作者等）
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UserId(pub String);

impl UserId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for UserId {
    fn from(s: String) -> Self {
        UserId(s)
    }
}

impl From<&str> for UserId {
    fn from(s: &str) -> Self {
        UserId(s.to_string())
    }
}

/// 会话 ID（与 flare-core CID 规则一致）
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConversationId(pub String);

impl ConversationId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ConversationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ConversationId {
    fn from(s: String) -> Self {
        ConversationId(s)
    }
}

impl From<&str> for ConversationId {
    fn from(s: &str) -> Self {
        ConversationId(s.to_string())
    }
}
