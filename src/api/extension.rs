//! 扩展功能 API 实现
//!
//! 提供扩展提供者、扩展缓存、业务扩展点注册等功能

#[cfg(feature = "extensions")]
use crate::api::FlareIMClient;
#[cfg(feature = "extensions")]
use crate::api::traits::ExtensionApi;
#[cfg(feature = "extensions")]
use crate::shared::extension::bridge::{
    ChannelExtensionBridge, GroupExtensionBridge, UserExtensionBridge,
};
#[cfg(feature = "extensions")]
use anyhow::{Context, Result};
#[cfg(feature = "extensions")]
use std::sync::Arc;
#[cfg(feature = "extensions")]
use tracing::info;

#[cfg(feature = "extensions")]
impl ExtensionApi for FlareIMClient {
    async fn register_extension_provider(
        &self,
        provider: Arc<dyn crate::domain::ExtensionProvider>,
    ) -> Result<()> {
        self.extension_manager.add_provider(provider).await;
        Ok(())
    }

    async fn set_extension_cache(
        &self,
        _cache: Arc<dyn crate::domain::ExtensionCache>,
    ) -> Result<()> {
        // ExtensionManager 需要支持运行时设置缓存
        // 这里暂时返回错误，后续可以改进
        Err(anyhow::anyhow!(
            "Setting cache at runtime not yet supported. Please set cache when creating ExtensionManager."
        ))
    }

    async fn register_user_business_extension(
        &self,
        extension: Arc<dyn crate::shared::extension::business::UserBusinessExtension>,
    ) -> Result<()> {
        info!(
            name = extension.name(),
            domain = %extension.business_domain(),
            "Registering user business extension"
        );

        // 1. 注册到业务扩展注册中心
        self.business_extension_registry
            .register(extension.clone(), self)
            .await
            .context("Failed to register user business extension")?;

        // 2. 创建桥接器并注册到扩展管理器
        let bridge = Arc::new(UserExtensionBridge::new(extension));
        self.extension_manager.add_provider(bridge).await;

        info!("User business extension registered successfully");
        Ok(())
    }

    async fn register_group_business_extension(
        &self,
        extension: Arc<dyn crate::shared::extension::business::GroupBusinessExtension>,
    ) -> Result<()> {
        info!(
            name = extension.name(),
            domain = %extension.business_domain(),
            "Registering group business extension"
        );

        // 1. 注册到业务扩展注册中心
        self.business_extension_registry
            .register(extension.clone(), self)
            .await
            .context("Failed to register group business extension")?;

        // 2. 创建桥接器并注册到扩展管理器
        let bridge = Arc::new(GroupExtensionBridge::new(extension));
        self.extension_manager.add_provider(bridge).await;

        info!("Group business extension registered successfully");
        Ok(())
    }

    async fn register_channel_business_extension(
        &self,
        extension: Arc<dyn crate::shared::extension::business::ChannelBusinessExtension>,
    ) -> Result<()> {
        info!(
            name = extension.name(),
            domain = %extension.business_domain(),
            "Registering channel business extension"
        );

        // 1. 注册到业务扩展注册中心
        self.business_extension_registry
            .register(extension.clone(), self)
            .await
            .context("Failed to register channel business extension")?;

        // 2. 创建桥接器并注册到扩展管理器
        let bridge = Arc::new(ChannelExtensionBridge::new(extension));
        self.extension_manager.add_provider(bridge).await;

        info!("Channel business extension registered successfully");
        Ok(())
    }

    fn business_extension_registry(
        &self,
    ) -> Arc<crate::shared::extension::BusinessExtensionRegistry> {
        Arc::clone(&self.business_extension_registry)
    }
}
