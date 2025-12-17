//! SDK 扩展模型
//!
//! 定义 SDK 层特有的扩展字段，用于存储客户端特有的信息
//! 如头像、显示名称、本地状态等

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// 消息扩展信息（SDK 层特有）
///
/// 用于存储消息的客户端特有信息，不包含在 flare-proto 中
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageExtension {
    /// 发送者头像 URL（客户端缓存）
    pub sender_avatar: Option<String>,

    /// 发送者显示名称（客户端缓存）
    pub sender_name: Option<String>,

    /// 消息本地状态（已读、已删除等）
    pub local_state: Option<MessageLocalState>,

    /// 是否已下载（媒体消息）
    pub is_downloaded: Option<bool>,

    /// 下载进度（0-100）
    pub download_progress: Option<u8>,

    /// 自定义扩展字段
    #[serde(default)]
    pub custom: std::collections::HashMap<String, String>,
}

/// 消息本地状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageLocalState {
    /// 发送中
    Sending,
    /// 发送成功
    Sent,
    /// 发送失败
    Failed,
    /// 已读（本地标记）
    Read,
    /// 已删除（本地标记）
    Deleted,
}

/// 会话扩展信息（SDK 层特有）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionExtension {
    /// 会话头像 URL（群聊/频道）
    pub avatar: Option<String>,

    /// 会话显示名称（客户端缓存）
    pub display_name: Option<String>,

    /// 是否置顶
    #[serde(default)]
    pub is_pinned: bool,

    /// 是否免打扰
    #[serde(default)]
    pub is_muted: bool,

    /// 最后查看时间（本地，毫秒时间戳）
    pub last_viewed_at: Option<i64>,

    /// 自定义扩展字段
    #[serde(default)]
    pub custom: std::collections::HashMap<String, String>,
}

/// 用户扩展信息
///
/// 用于存储用户的客户端特有信息（头像、名称、在线状态等）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserExtension {
    /// 用户头像 URL
    pub avatar: Option<String>,

    /// 用户显示名称
    pub name: Option<String>,

    /// 用户在线状态（online/offline/busy/away）
    pub online_status: Option<String>,

    /// 自定义字段
    #[serde(default)]
    pub custom: std::collections::HashMap<String, String>,
}

/// 扩展信息提供者
///
/// 用于从各种来源（服务端、本地缓存、用户模块等）获取扩展信息
#[async_trait]
pub trait ExtensionProvider: Send + Sync {
    /// 获取用户扩展信息
    async fn get_user_extension(&self, user_id: &str) -> Result<Option<UserExtension>>;

    /// 获取会话扩展信息
    async fn get_session_extension(&self, session_id: &str) -> Result<Option<SessionExtension>>;

    /// 批量获取用户扩展信息
    async fn batch_get_user_extensions(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<(String, UserExtension)>>;
}

/// 扩展缓存接口
#[async_trait]
pub trait ExtensionCache: Send + Sync {
    /// 获取用户扩展信息（从缓存）
    async fn get_user_extension(&self, user_id: &str) -> Result<Option<UserExtension>>;

    /// 保存用户扩展信息（到缓存）
    async fn save_user_extension(&self, user_id: &str, extension: &UserExtension) -> Result<()>;

    /// 获取会话扩展信息（从缓存）
    async fn get_session_extension(&self, session_id: &str) -> Result<Option<SessionExtension>>;

    /// 保存会话扩展信息（到缓存）
    async fn save_session_extension(
        &self,
        session_id: &str,
        extension: &SessionExtension,
    ) -> Result<()>;
}

/// 默认扩展提供者（空实现，由扩展模块填充）
pub struct DefaultExtensionProvider;

#[async_trait]
impl ExtensionProvider for DefaultExtensionProvider {
    async fn get_user_extension(&self, _user_id: &str) -> Result<Option<UserExtension>> {
        Ok(None)
    }

    async fn get_session_extension(&self, _session_id: &str) -> Result<Option<SessionExtension>> {
        Ok(None)
    }

    async fn batch_get_user_extensions(
        &self,
        _user_ids: &[String],
    ) -> Result<Vec<(String, UserExtension)>> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_extension() {
        let mut ext = MessageExtension::default();
        ext.sender_avatar = Some("https://example.com/avatar.jpg".to_string());
        ext.sender_name = Some("John Doe".to_string());
        ext.local_state = Some(MessageLocalState::Sent);

        assert_eq!(
            ext.sender_avatar.as_deref(),
            Some("https://example.com/avatar.jpg")
        );
        assert_eq!(ext.sender_name.as_deref(), Some("John Doe"));
    }

    #[test]
    fn test_session_extension() {
        let mut ext = SessionExtension::default();
        ext.display_name = Some("Group Chat".to_string());
        ext.is_pinned = true;
        ext.is_muted = false;

        assert_eq!(ext.display_name.as_deref(), Some("Group Chat"));
        assert!(ext.is_pinned);
        assert!(!ext.is_muted);
    }
}
