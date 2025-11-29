//! 扩展管理器
//!
//! 管理所有注册的扩展点

use crate::client::FlareIMClient;
use crate::extension::point::{
    ExtensionPoint, MessageHandlerExtension, EventListenerExtension,
    SyncStrategyExtension, StorageExtension,
};
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::Mutex;

/// 扩展管理器
/// 
/// 管理所有注册的扩展点
pub struct ExtensionManager {
    extensions: Arc<Mutex<Vec<Arc<dyn ExtensionPoint>>>>,
    message_handlers: Arc<Mutex<Vec<Arc<dyn MessageHandlerExtension>>>>,
    event_listeners: Arc<Mutex<Vec<Arc<dyn EventListenerExtension>>>>,
    sync_strategies: Arc<Mutex<Vec<Arc<dyn SyncStrategyExtension>>>>,
    storage_extensions: Arc<Mutex<Vec<Arc<dyn StorageExtension>>>>,
}

impl ExtensionManager {
    /// 创建扩展管理器
    pub fn new() -> Self {
        Self {
            extensions: Arc::new(Mutex::new(Vec::new())),
            message_handlers: Arc::new(Mutex::new(Vec::new())),
            event_listeners: Arc::new(Mutex::new(Vec::new())),
            sync_strategies: Arc::new(Mutex::new(Vec::new())),
            storage_extensions: Arc::new(Mutex::new(Vec::new())),
        }
    }
    
    /// 注册扩展点
    /// 
    /// # 参数
    /// - `extension`: 扩展点实例
    /// 
    /// # 注意
    /// - 注册后会自动调用 `initialize` 方法
    pub async fn register<T: ExtensionPoint + 'static>(
        &self,
        extension: Arc<T>,
        client: &FlareIMClient,
    ) -> Result<()> {
        // 初始化扩展点
        extension.initialize(client).await
            .context("Failed to initialize extension")?;
        
        // 添加到扩展列表
        self.extensions.lock().await.push(extension.clone());
        
        // 注意：由于Rust的类型系统限制，无法在运行时判断extension是否实现了特定trait
        // 业务层应该直接调用对应的register_xxx方法
        
        Ok(())
    }
    
    /// 注册消息处理器扩展
    pub async fn register_message_handler(
        &self,
        handler: Arc<dyn MessageHandlerExtension>,
        client: &FlareIMClient,
    ) -> Result<()> {
        handler.initialize(client).await
            .context("Failed to initialize message handler extension")?;
        
        self.message_handlers.lock().await.push(handler.clone());
        self.extensions.lock().await.push(handler);
        
        Ok(())
    }
    
    /// 注册事件监听器扩展
    pub async fn register_event_listener(
        &self,
        listener: Arc<dyn EventListenerExtension>,
        client: &FlareIMClient,
    ) -> Result<()> {
        listener.initialize(client).await
            .context("Failed to initialize event listener extension")?;
        
        self.event_listeners.lock().await.push(listener.clone());
        self.extensions.lock().await.push(listener);
        
        Ok(())
    }
    
    /// 注册同步策略扩展
    pub async fn register_sync_strategy(
        &self,
        strategy: Arc<dyn SyncStrategyExtension>,
        client: &FlareIMClient,
    ) -> Result<()> {
        strategy.initialize(client).await
            .context("Failed to initialize sync strategy extension")?;
        
        self.sync_strategies.lock().await.push(strategy.clone());
        self.extensions.lock().await.push(strategy);
        
        Ok(())
    }
    
    /// 注册存储扩展
    pub async fn register_storage_extension(
        &self,
        extension: Arc<dyn StorageExtension>,
        client: &FlareIMClient,
    ) -> Result<()> {
        extension.initialize(client).await
            .context("Failed to initialize storage extension")?;
        
        self.storage_extensions.lock().await.push(extension.clone());
        self.extensions.lock().await.push(extension);
        
        Ok(())
    }
    
    /// 获取所有扩展点
    pub async fn get_extensions(&self) -> Vec<Arc<dyn ExtensionPoint>> {
        self.extensions.lock().await.clone()
    }
    
    /// 获取消息处理器扩展
    pub async fn get_message_handlers(&self) -> Vec<Arc<dyn MessageHandlerExtension>> {
        self.message_handlers.lock().await.clone()
    }
    
    /// 获取事件监听器扩展
    pub async fn get_event_listeners(&self) -> Vec<Arc<dyn EventListenerExtension>> {
        self.event_listeners.lock().await.clone()
    }
    
    /// 获取同步策略扩展
    pub async fn get_sync_strategies(&self) -> Vec<Arc<dyn SyncStrategyExtension>> {
        self.sync_strategies.lock().await.clone()
    }
    
    /// 获取存储扩展
    pub async fn get_storage_extensions(&self) -> Vec<Arc<dyn StorageExtension>> {
        self.storage_extensions.lock().await.clone()
    }
}

impl Default for ExtensionManager {
    fn default() -> Self {
        Self::new()
    }
}


