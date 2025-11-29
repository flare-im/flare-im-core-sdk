//! 扩展点接口定义
//!
//! 定义各种扩展点接口，供业务SDK实现

use crate::client::FlareIMClient;
use crate::event::Event;
use crate::model::Message;
use crate::service::sync::ReconnectSyncStrategy;
use anyhow::Result;
use async_trait::async_trait;

/// 扩展点接口
/// 
/// 供业务SDK扩展的核心接口
#[async_trait]
pub trait ExtensionPoint: Send + Sync {
    /// 扩展点名称
    fn name(&self) -> &str;
    
    /// 扩展点版本
    fn version(&self) -> &str;
    
    /// 初始化扩展点
    /// 
    /// # 参数
    /// - `client`: FlareIMClient 实例
    /// 
    /// # 注意
    /// - 此方法在扩展点注册时调用
    async fn initialize(&self, client: &FlareIMClient) -> Result<()>;
    
    /// 清理扩展点
    /// 
    /// # 注意
    /// - 此方法在扩展点卸载时调用
    async fn cleanup(&self) -> Result<()>;
}

/// 消息处理器扩展
#[async_trait]
pub trait MessageHandlerExtension: ExtensionPoint {
    /// 处理消息
    /// 
    /// # 参数
    /// - `message`: 接收到的消息
    /// 
    /// # 返回
    /// - `Result<()>`: 处理结果
    async fn handle_message(&self, message: &Message) -> Result<()>;
    
    /// 支持的消息类型
    /// 
    /// # 返回
    /// - 支持的消息类型列表（例如：["text", "image", "custom_card"]）
    fn supported_message_types(&self) -> Vec<String>;
}

/// 事件监听器扩展
#[async_trait]
pub trait EventListenerExtension: ExtensionPoint {
    /// 处理事件
    /// 
    /// # 参数
    /// - `event`: 事件
    /// 
    /// # 返回
    /// - `Result<()>`: 处理结果
    async fn handle_event(&self, event: &Event) -> Result<()>;
    
    /// 支持的事件类型
    /// 
    /// # 返回
    /// - 支持的事件类型列表
    fn supported_event_types(&self) -> Vec<EventType>;
}

/// 事件类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventType {
    /// 连接事件
    Connection,
    /// 消息事件
    Message,
    /// 会话事件
    Session,
    /// 同步事件
    Sync,
}

/// 同步策略扩展
#[async_trait]
pub trait SyncStrategyExtension: ExtensionPoint {
    /// 自定义同步策略
    /// 
    /// # 参数
    /// - `context`: 同步上下文
    /// 
    /// # 返回
    /// - 自定义的同步策略
    async fn customize_sync_strategy(
        &self,
        context: &SyncContext,
    ) -> Result<ReconnectSyncStrategy>;
}

/// 同步上下文
#[derive(Debug, Clone)]
pub struct SyncContext {
    /// 离线时间（如果重连）
    pub offline_duration: Option<std::time::Duration>,
    
    /// 当前用户ID
    pub user_id: String,
    
    /// 会话数量
    pub session_count: usize,
}

/// 存储扩展
#[async_trait]
pub trait StorageExtension: ExtensionPoint {
    /// 消息保存前钩子
    /// 
    /// # 参数
    /// - `message`: 要保存的消息
    /// 
    /// # 返回
    /// - `Result<()>`: 如果返回错误，消息将不会被保存
    async fn before_save_message(&self, message: &Message) -> Result<()>;
    
    /// 消息保存后钩子
    /// 
    /// # 参数
    /// - `message`: 已保存的消息
    async fn after_save_message(&self, message: &Message) -> Result<()>;
}

