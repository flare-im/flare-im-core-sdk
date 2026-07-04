//! 用户资料（显示名称、头像）— 用于消息/会话展示
//!
//! 可由 SyncTask 同步或本地缓存填充；未命中时展示 user_id。

use serde::{Deserialize, Serialize};

/// 用户资料：显示名称 + 头像，供消息发送者/会话最后一条发送者展示
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub user_id: String,
    /// 展示用名称（昵称或业务 display_name）
    pub nickname: String,
    /// 头像 URL
    pub avatar_url: String,
}

impl UserProfile {
    pub fn new(user_id: impl Into<String>) -> Self {
        Self {
            user_id: user_id.into(),
            nickname: String::new(),
            avatar_url: String::new(),
        }
    }

    pub fn with_nickname(mut self, nickname: impl Into<String>) -> Self {
        self.nickname = nickname.into();
        self
    }

    pub fn with_avatar(mut self, avatar_url: impl Into<String>) -> Self {
        self.avatar_url = avatar_url.into();
        self
    }

    /// 展示名：有 nickname 用 nickname，否则用 user_id（与 display_name 语义一致）
    pub fn display_name(&self) -> &str {
        if self.nickname.is_empty() {
            &self.user_id
        } else {
            &self.nickname
        }
    }
}
