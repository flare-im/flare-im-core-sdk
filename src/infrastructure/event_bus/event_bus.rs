//! 事件总线
//!
//! 用于发布领域事件，供 UI 层订阅
//!
//! ## 设计特点
//!
//! 1. **双重机制**: 既支持类型安全的订阅器（推荐），也支持原始的 broadcast channel 订阅（向后兼容）
//! 2. **自动分发**: 发布事件时自动分发到所有注册的订阅者
//! 3. **异步处理**: 事件分发是异步的，不阻塞发布者
//!
//! ## 使用方式
//!
//! ### 方式一：使用类型安全的订阅器（推荐）
//!
//! ```rust
//! use flare_im_core_sdk::domain::event::subscribers::*;
//!
//! struct MySubscriber;
//!
//! #[async_trait::async_trait]
//! impl MessageEventSubscriber for MySubscriber {
//!     async fn on_message_delivered(&self, event: &MessageDelivered) -> anyhow::Result<()> {
//!         println!("收到消息: {}", event.message_id);
//!         Ok(())
//!     }
//! }
//!
//! // 注册订阅者
//! event_bus.subscribe_message(Arc::new(MySubscriber)).await;
//! ```
//!
//! ### 方式二：使用原始的 broadcast channel（向后兼容）
//!
//! ```rust
//! let mut receiver = event_bus.subscribe();
//! tokio::spawn(async move {
//!     while let Ok(event) = receiver.recv().await {
//!         // 处理事件
//!     }
//! });
//! ```

use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::warn;
use crate::domain::event::DomainEvent;
use crate::domain::event::subscribers::*;
use super::subscription_manager::{EventSubscriptionManager, SubscriptionStatistics};

/// 事件总线
///
/// 提供事件发布和订阅功能，支持类型安全的订阅器和原始的 broadcast channel
pub struct EventBus {
    /// 原始的 broadcast channel（用于向后兼容和通用订阅）
    sender: broadcast::Sender<DomainEvent>,
    
    /// 订阅管理器（用于类型安全的订阅）
    subscription_manager: Arc<EventSubscriptionManager>,
}

impl EventBus {
    /// 创建新的事件总线
    ///
    /// # 参数
    /// * `capacity` - 事件通道容量（建议 1000-10000）
    pub fn new(capacity: usize) -> Self {
        Self {
            sender: broadcast::channel(capacity).0,
            subscription_manager: Arc::new(EventSubscriptionManager::new()),
        }
    }
    
    /// 发布事件
    ///
    /// 事件会同时：
    /// 1. 发送到 broadcast channel（供原始订阅者使用）
    /// 2. 分发到所有注册的类型安全订阅者
    ///
    /// 性能优化：使用 fire-and-forget 模式，不阻塞发布者
    pub async fn publish(&self, event: DomainEvent) -> anyhow::Result<()> {
        // 1. 发送到 broadcast channel（向后兼容，非阻塞）
        let _ = self.sender.send(event.clone());
        
        // 2. 分发到类型安全的订阅者（异步，不阻塞）
        let subscription_manager = self.subscription_manager.clone();
        tokio::spawn(async move {
            subscription_manager.dispatch(&event).await;
        });
        
        Ok(())
    }
    
    /// 订阅事件（原始方式，向后兼容）
    ///
    /// 返回一个 broadcast receiver，可以接收所有类型的事件
    pub fn subscribe(&self) -> broadcast::Receiver<DomainEvent> {
        self.sender.subscribe()
    }

    // ============================================================================
    // 类型安全的订阅方法（推荐使用）
    // ============================================================================

    /// 注册 Connection 事件订阅者
    ///
    /// # 返回
    /// 返回订阅者 ID，可用于后续取消订阅
    pub async fn subscribe_connection(
        &self,
        subscriber: Arc<dyn ConnectionEventSubscriber>,
    ) -> String {
        self.subscription_manager.subscribe_connection(subscriber).await
    }

    /// 注册 Session 事件订阅者
    ///
    /// # 返回
    /// 返回订阅者 ID，可用于后续取消订阅
    pub async fn subscribe_session(
        &self,
        subscriber: Arc<dyn SessionEventSubscriber>,
    ) -> String {
        self.subscription_manager.subscribe_session(subscriber).await
    }

    /// 注册 Message 事件订阅者
    ///
    /// # 返回
    /// 返回订阅者 ID，可用于后续取消订阅
    pub async fn subscribe_message(
        &self,
        subscriber: Arc<dyn MessageEventSubscriber>,
    ) -> String {
        self.subscription_manager.subscribe_message(subscriber).await
    }

    /// 注册 Conversation 事件订阅者
    ///
    /// # 返回
    /// 返回订阅者 ID，可用于后续取消订阅
    pub async fn subscribe_conversation(
        &self,
        subscriber: Arc<dyn ConversationEventSubscriber>,
    ) -> String {
        self.subscription_manager.subscribe_conversation(subscriber).await
    }

    /// 注册 Sync 事件订阅者
    ///
    /// # 返回
    /// 返回订阅者 ID，可用于后续取消订阅
    pub async fn subscribe_sync(
        &self,
        subscriber: Arc<dyn SyncEventSubscriber>,
    ) -> String {
        self.subscription_manager.subscribe_sync(subscriber).await
    }

    // ============================================================================
    // 取消订阅方法
    // ============================================================================

    /// 取消 Connection 事件订阅
    pub async fn unsubscribe_connection(&self, id: &str) -> bool {
        self.subscription_manager.unsubscribe_connection(id).await
    }

    /// 取消 Session 事件订阅
    pub async fn unsubscribe_session(&self, id: &str) -> bool {
        self.subscription_manager.unsubscribe_session(id).await
    }

    /// 取消 Message 事件订阅
    pub async fn unsubscribe_message(&self, id: &str) -> bool {
        self.subscription_manager.unsubscribe_message(id).await
    }

    /// 取消 Conversation 事件订阅
    pub async fn unsubscribe_conversation(&self, id: &str) -> bool {
        self.subscription_manager.unsubscribe_conversation(id).await
    }

    /// 取消 Sync 事件订阅
    pub async fn unsubscribe_sync(&self, id: &str) -> bool {
        self.subscription_manager.unsubscribe_sync(id).await
    }

    // ============================================================================
    // 统计方法
    // ============================================================================

    /// 获取订阅者统计信息
    pub async fn get_statistics(&self) -> SubscriptionStatistics {
        self.subscription_manager.get_statistics().await
    }
}
