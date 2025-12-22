//! 事件订阅器构建器
//!
//! 提供链式 API，方便用户注册多个订阅者
//!
//! ## 使用示例
//!
//! ```rust
//! use flare_im_core_sdk::interface::event::SubscriberBuilder;
//!
//! let builder = SubscriberBuilder::new(event_bus);
//! builder
//!     .message(Arc::new(MyMessageSubscriber))
//!     .connection(Arc::new(MyConnectionSubscriber))
//!     .session(Arc::new(MySessionSubscriber))
//!     .conversation(Arc::new(MyConversationSubscriber))
//!     .sync(Arc::new(MySyncSubscriber))
//!     .build()
//!     .await;
//! ```

use std::sync::Arc;
use crate::infrastructure::event_bus::EventBus;
use crate::domain::event::subscribers::*;

/// 事件订阅器构建器
///
/// 提供链式 API，方便用户一次性注册多个订阅者
pub struct SubscriberBuilder {
    event_bus: Arc<EventBus>,
    message_subscribers: Vec<Arc<dyn MessageEventSubscriber>>,
    connection_subscribers: Vec<Arc<dyn ConnectionEventSubscriber>>,
    session_subscribers: Vec<Arc<dyn SessionEventSubscriber>>,
    conversation_subscribers: Vec<Arc<dyn ConversationEventSubscriber>>,
    sync_subscribers: Vec<Arc<dyn SyncEventSubscriber>>,
}

impl SubscriberBuilder {
    /// 创建新的构建器
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self {
            event_bus,
            message_subscribers: Vec::new(),
            connection_subscribers: Vec::new(),
            session_subscribers: Vec::new(),
            conversation_subscribers: Vec::new(),
            sync_subscribers: Vec::new(),
        }
    }

    /// 添加 Message 事件订阅者
    pub fn message(mut self, subscriber: Arc<dyn MessageEventSubscriber>) -> Self {
        self.message_subscribers.push(subscriber);
        self
    }

    /// 添加 Connection 事件订阅者
    pub fn connection(mut self, subscriber: Arc<dyn ConnectionEventSubscriber>) -> Self {
        self.connection_subscribers.push(subscriber);
        self
    }

    /// 添加 Session 事件订阅者
    pub fn session(mut self, subscriber: Arc<dyn SessionEventSubscriber>) -> Self {
        self.session_subscribers.push(subscriber);
        self
    }

    /// 添加 Conversation 事件订阅者
    pub fn conversation(mut self, subscriber: Arc<dyn ConversationEventSubscriber>) -> Self {
        self.conversation_subscribers.push(subscriber);
        self
    }

    /// 添加 Sync 事件订阅者
    pub fn sync(mut self, subscriber: Arc<dyn SyncEventSubscriber>) -> Self {
        self.sync_subscribers.push(subscriber);
        self
    }

    /// 构建并注册所有订阅者
    pub async fn build(self) {
        // 注册所有 Message 订阅者
        for subscriber in self.message_subscribers {
            self.event_bus.subscribe_message(subscriber).await;
        }

        // 注册所有 Connection 订阅者
        for subscriber in self.connection_subscribers {
            self.event_bus.subscribe_connection(subscriber).await;
        }

        // 注册所有 Session 订阅者
        for subscriber in self.session_subscribers {
            self.event_bus.subscribe_session(subscriber).await;
        }

        // 注册所有 Conversation 订阅者
        for subscriber in self.conversation_subscribers {
            self.event_bus.subscribe_conversation(subscriber).await;
        }

        // 注册所有 Sync 订阅者
        for subscriber in self.sync_subscribers {
            self.event_bus.subscribe_sync(subscriber).await;
        }
    }
}
