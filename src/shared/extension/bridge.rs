//! 业务扩展点桥接层
//!
//! 将业务扩展点（UserBusinessExtension、GroupBusinessExtension 等）桥接到 ExtensionProvider
//! 实现业务扩展点与扩展信息提供者的无缝集成
//!
//! ## 设计理念
//!
//! 参考 Spring Framework 的 Adapter 模式和 Telegram SDK 的扩展机制：
//! - **桥接模式**: 业务扩展点通过桥接层转换为 ExtensionProvider
//! - **自动填充**: 桥接层自动将业务扩展点的数据转换为扩展信息
//! - **优先级支持**: 支持多个扩展点，按优先级选择

use crate::domain::extension::{ExtensionProvider, SessionExtension, UserExtension};
use crate::shared::extension::business::{
    ChannelBusinessExtension, GroupBusinessExtension, GroupInfo, UserBusinessExtension, UserInfo,
};
use crate::shared::extension::point::ExtensionPoint;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 用户业务扩展点桥接器
///
/// 将 `UserBusinessExtension` 转换为 `ExtensionProvider`
pub struct UserExtensionBridge {
    /// 用户业务扩展点
    user_extension: Arc<dyn UserBusinessExtension>,
}

impl UserExtensionBridge {
    /// 创建新的用户扩展桥接器
    pub fn new(user_extension: Arc<dyn UserBusinessExtension>) -> Self {
        Self { user_extension }
    }
}

#[async_trait]
impl ExtensionProvider for UserExtensionBridge {
    async fn get_user_extension(&self, user_id: &str) -> Result<Option<UserExtension>> {
        // 从业务扩展点获取用户信息
        let user_info = self
            .user_extension
            .get_user_info(user_id)
            .await
            .context("Failed to get user info from business extension")?;

        // 转换为 UserExtension
        let extension = user_info.map(|info| UserExtension {
            avatar: info.avatar,
            name: Some(info.name),
            online_status: Some(format!("{:?}", info.online_status)),
            custom: info.custom,
        });

        Ok(extension)
    }

    async fn get_session_extension(&self, _session_id: &str) -> Result<Option<SessionExtension>> {
        // 用户扩展点不提供会话扩展信息
        Ok(None)
    }

    async fn batch_get_user_extensions(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<(String, UserExtension)>> {
        // 批量获取用户信息
        let user_infos = self
            .user_extension
            .batch_get_user_info(user_ids)
            .await
            .context("Failed to batch get user info from business extension")?;

        // 转换为 UserExtension 列表
        let extensions: Vec<(String, UserExtension)> = user_infos
            .into_iter()
            .map(|info| {
                let extension = UserExtension {
                    avatar: info.avatar.clone(),
                    name: Some(info.name.clone()),
                    online_status: Some(format!("{:?}", info.online_status)),
                    custom: info.custom.clone(),
                };
                (info.user_id, extension)
            })
            .collect();

        Ok(extensions)
    }
}

/// 群组业务扩展点桥接器
///
/// 将 `GroupBusinessExtension` 转换为 `ExtensionProvider`
pub struct GroupExtensionBridge {
    /// 群组业务扩展点
    group_extension: Arc<dyn GroupBusinessExtension>,
}

impl GroupExtensionBridge {
    /// 创建新的群组扩展桥接器
    pub fn new(group_extension: Arc<dyn GroupBusinessExtension>) -> Self {
        Self { group_extension }
    }

    /// 根据会话 ID 提取群组 ID
    ///
    /// 会话 ID 格式：`group:{business_type}:{group_id}`
    fn extract_group_id(&self, session_id: &str) -> Option<String> {
        // 解析会话 ID，提取群组 ID
        // 格式：group:{business_type}:{group_id}
        if session_id.starts_with("group:") {
            let parts: Vec<&str> = session_id.splitn(3, ':').collect();
            if parts.len() >= 3 {
                return Some(parts[2].to_string());
            }
        }
        None
    }
}

#[async_trait]
impl ExtensionProvider for GroupExtensionBridge {
    async fn get_user_extension(&self, _user_id: &str) -> Result<Option<UserExtension>> {
        // 群组扩展点不提供用户扩展信息
        Ok(None)
    }

    async fn get_session_extension(&self, session_id: &str) -> Result<Option<SessionExtension>> {
        // 从会话 ID 提取群组 ID
        let group_id = match self.extract_group_id(session_id) {
            Some(id) => id,
            None => return Ok(None), // 不是群组会话
        };

        // 从业务扩展点获取群组信息
        let group_info = self
            .group_extension
            .get_group_info(&group_id)
            .await
            .context("Failed to get group info from business extension")?;

        // 转换为 SessionExtension
        let extension = group_info.map(|info| SessionExtension {
            avatar: info.avatar,
            display_name: Some(info.name),
            is_pinned: false, // 从会话本身获取
            is_muted: false,  // 从会话本身获取
            last_viewed_at: None,
            custom: info.custom,
        });

        Ok(extension)
    }

    async fn batch_get_user_extensions(
        &self,
        _user_ids: &[String],
    ) -> Result<Vec<(String, UserExtension)>> {
        // 群组扩展点不提供用户扩展信息
        Ok(vec![])
    }
}

/// 频道业务扩展点桥接器
///
/// 将 `ChannelBusinessExtension` 转换为 `ExtensionProvider`
pub struct ChannelExtensionBridge {
    /// 频道业务扩展点
    channel_extension: Arc<dyn ChannelBusinessExtension>,
}

impl ChannelExtensionBridge {
    /// 创建新的频道扩展桥接器
    pub fn new(channel_extension: Arc<dyn ChannelBusinessExtension>) -> Self {
        Self { channel_extension }
    }

    /// 根据会话 ID 提取频道 ID
    ///
    /// 会话 ID 格式：`channel:{business_type}:{channel_id}`
    fn extract_channel_id(&self, session_id: &str) -> Option<String> {
        // 解析会话 ID，提取频道 ID
        // 格式：channel:{business_type}:{channel_id}
        if session_id.starts_with("channel:") {
            let parts: Vec<&str> = session_id.splitn(3, ':').collect();
            if parts.len() >= 3 {
                return Some(parts[2].to_string());
            }
        }
        None
    }
}

#[async_trait]
impl ExtensionProvider for ChannelExtensionBridge {
    async fn get_user_extension(&self, _user_id: &str) -> Result<Option<UserExtension>> {
        // 频道扩展点不提供用户扩展信息
        Ok(None)
    }

    async fn get_session_extension(&self, session_id: &str) -> Result<Option<SessionExtension>> {
        // 从会话 ID 提取频道 ID
        let channel_id = match self.extract_channel_id(session_id) {
            Some(id) => id,
            None => return Ok(None), // 不是频道会话
        };

        // 从业务扩展点获取频道信息
        let channel_info = self
            .channel_extension
            .get_channel_info(&channel_id)
            .await
            .context("Failed to get channel info from business extension")?;

        // 转换为 SessionExtension
        let extension = channel_info.map(|info| SessionExtension {
            avatar: None, // 频道通常没有头像
            display_name: Some(info.name),
            is_pinned: false,
            is_muted: false,
            last_viewed_at: None,
            custom: info.custom,
        });

        Ok(extension)
    }

    async fn batch_get_user_extensions(
        &self,
        _user_ids: &[String],
    ) -> Result<Vec<(String, UserExtension)>> {
        // 频道扩展点不提供用户扩展信息
        Ok(vec![])
    }
}

/// 组合扩展提供者
///
/// 组合多个 ExtensionProvider，按优先级顺序查询
pub struct CompositeExtensionProvider {
    /// 扩展提供者列表（按优先级排序）
    providers: Arc<RwLock<Vec<Arc<dyn ExtensionProvider>>>>,
}

impl CompositeExtensionProvider {
    /// 创建新的组合扩展提供者
    pub fn new() -> Self {
        Self {
            providers: Arc::new(RwLock::new(vec![])),
        }
    }

    /// 添加扩展提供者（按优先级顺序添加）
    pub async fn add_provider(&self, provider: Arc<dyn ExtensionProvider>) {
        let mut providers = self.providers.write().await;
        providers.push(provider);
    }

    /// 清除所有提供者
    pub async fn clear(&self) {
        let mut providers = self.providers.write().await;
        providers.clear();
    }
}

impl Default for CompositeExtensionProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExtensionProvider for CompositeExtensionProvider {
    async fn get_user_extension(&self, user_id: &str) -> Result<Option<UserExtension>> {
        // 按顺序查询所有提供者，返回第一个非空结果
        let providers = self.providers.read().await;
        for provider in providers.iter() {
            if let Ok(Some(ext)) = provider.get_user_extension(user_id).await {
                return Ok(Some(ext));
            }
        }
        Ok(None)
    }

    async fn get_session_extension(&self, session_id: &str) -> Result<Option<SessionExtension>> {
        // 按顺序查询所有提供者，返回第一个非空结果
        let providers = self.providers.read().await;
        for provider in providers.iter() {
            if let Ok(Some(ext)) = provider.get_session_extension(session_id).await {
                return Ok(Some(ext));
            }
        }
        Ok(None)
    }

    async fn batch_get_user_extensions(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<(String, UserExtension)>> {
        // 按顺序查询所有提供者，合并结果
        let providers = self.providers.read().await;
        let mut results = HashMap::new();

        for provider in providers.iter() {
            if let Ok(extensions) = provider.batch_get_user_extensions(user_ids).await {
                for (user_id, ext) in extensions {
                    // 只保留第一个提供者的结果（优先级）
                    results.entry(user_id).or_insert(ext);
                }
            }
        }

        Ok(results.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::extension::business::{OnlineStatus, UserInfo};

    struct MockUserExtension;

    #[async_trait]
    impl ExtensionPoint for MockUserExtension {
        fn name(&self) -> &str {
            "mock_user"
        }

        fn version(&self) -> &str {
            "1.0.0"
        }

        async fn initialize(&self, _client: &crate::api::FlareIMClient) -> Result<()> {
            Ok(())
        }

        async fn cleanup(&self) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl crate::shared::extension::business::BusinessExtensionPoint for MockUserExtension {
        fn business_domain(&self) -> crate::shared::extension::business::BusinessDomain {
            crate::shared::extension::business::BusinessDomain::User
        }
    }

    #[async_trait]
    impl UserBusinessExtension for MockUserExtension {
        fn business_domain(&self) -> crate::shared::extension::business::BusinessDomain {
            crate::shared::extension::business::BusinessDomain::User
        }

        async fn get_user_info(&self, user_id: &str) -> Result<Option<UserInfo>> {
            Ok(Some(UserInfo {
                user_id: user_id.to_string(),
                name: format!("User {}", user_id),
                avatar: Some(format!("https://example.com/avatar/{}.jpg", user_id)),
                online_status: OnlineStatus::Online,
                bio: None,
                custom: HashMap::new(),
            }))
        }
    }

    #[tokio::test]
    async fn test_user_extension_bridge() {
        let user_ext = Arc::new(MockUserExtension);
        let bridge = UserExtensionBridge::new(user_ext);

        let result = bridge.get_user_extension("user_123").await.unwrap();
        assert!(result.is_some());
        let ext = result.unwrap();
        assert_eq!(ext.name, Some("User user_123".to_string()));
        assert!(ext.avatar.is_some());
    }
}
