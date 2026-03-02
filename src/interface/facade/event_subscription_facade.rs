//! Event Subscription Facade
//!
//! Provides convenient APIs for event subscription, encapsulating the complexity
//! of the EventBus. This facade simplifies event subscription for users.
//!
//! ## Design Principles
//!
//! 1. **Single Responsibility**: Only handles event subscription APIs
//! 2. **Convenience**: Provides multiple usage patterns for different scenarios
//! 3. **Type Safety**: Compile-time type guarantees
//!
//! ## Example
//!
//! ```no_run
//! use flare_im_core_sdk::interface::facade::EventSubscriptionFacade;
//! use flare_im_core_sdk::interface::event::subscribers::*;
//! use std::sync::Arc;
//!
//! # async fn example(facade: &EventSubscriptionFacade, subscriber: Arc<dyn MessageEventSubscriber>) {
//! let subscriber_id = facade.subscribe_message(subscriber).await;
//! # }
//! ```

use std::sync::Arc;
use crate::infrastructure::event_bus::{EventBus, SubscriptionStatistics};
use crate::domain::event::subscribers::*;
use crate::interface::event::subscriber_builder::SubscriberBuilder;

/// Event subscription facade
///
/// Provides convenient APIs for subscribing to domain events, encapsulating
/// the complexity of the EventBus.
pub struct EventSubscriptionFacade {
    event_bus: Arc<EventBus>,
}

impl EventSubscriptionFacade {
    /// Creates a new event subscription facade
    ///
    /// # Arguments
    ///
    /// * `event_bus` - The event bus to use
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self { event_bus }
    }

    /// Returns a reference to the event bus
    ///
    /// Provides direct access to the event bus for advanced use cases.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::EventSubscriptionFacade;
    /// # use flare_im_core_sdk::interface::event::subscribers::*;
    /// # use std::sync::Arc;
    /// # async fn example(facade: &EventSubscriptionFacade, subscriber: Arc<dyn MessageEventSubscriber>) {
    /// let event_bus = facade.event_bus();
    /// event_bus.subscribe_message(subscriber).await;
    /// # }
    /// ```
    pub fn event_bus(&self) -> &Arc<EventBus> {
        &self.event_bus
    }

    /// Creates an event subscriber builder
    ///
    /// Provides a fluent API for registering multiple subscribers at once.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::EventSubscriptionFacade;
    /// # use flare_im_core_sdk::interface::event::subscribers::*;
    /// # use std::sync::Arc;
    /// # async fn example(facade: &EventSubscriptionFacade, msg_sub: Arc<dyn MessageEventSubscriber>, conn_sub: Arc<dyn ConnectionEventSubscriber>) {
    /// facade.subscribe_events()
    ///     .message(msg_sub)
    ///     .connection(conn_sub)
    ///     .build()
    ///     .await;
    /// # }
    /// ```
    pub fn subscribe_events(&self) -> SubscriberBuilder {
        SubscriberBuilder::new(self.event_bus.clone())
    }

    /// Subscribes to message events
    ///
    /// # Arguments
    ///
    /// * `subscriber` - The message event subscriber
    ///
    /// # Returns
    ///
    /// Returns a subscriber ID that can be used to unsubscribe later.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::EventSubscriptionFacade;
    /// # use flare_im_core_sdk::interface::event::subscribers::*;
    /// # use std::sync::Arc;
    /// # async fn example(facade: &EventSubscriptionFacade, subscriber: Arc<dyn MessageEventSubscriber>) {
    /// let subscriber_id = facade.subscribe_message(subscriber).await;
    /// // Later, unsubscribe
    /// facade.unsubscribe_message(&subscriber_id).await;
    /// # }
    /// ```
    pub async fn subscribe_message(
        &self,
        subscriber: Arc<dyn MessageEventSubscriber>,
    ) -> String {
        self.event_bus.subscribe_message(subscriber).await
    }

    /// Subscribes to connection events
    ///
    /// # Arguments
    ///
    /// * `subscriber` - The connection event subscriber
    ///
    /// # Returns
    ///
    /// Returns a subscriber ID that can be used to unsubscribe later.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::EventSubscriptionFacade;
    /// # use flare_im_core_sdk::interface::event::subscribers::*;
    /// # use std::sync::Arc;
    /// # async fn example(facade: &EventSubscriptionFacade, subscriber: Arc<dyn ConnectionEventSubscriber>) {
    /// let subscriber_id = facade.subscribe_connection(subscriber).await;
    /// # }
    /// ```
    pub async fn subscribe_connection(
        &self,
        subscriber: Arc<dyn ConnectionEventSubscriber>,
    ) -> String {
        self.event_bus.subscribe_connection(subscriber).await
    }

    /// 注册 Session 事件订阅者
    ///
    /// # 参数
    /// * `subscriber` - Session 事件订阅者
    ///
    /// # 返回
    /// 返回订阅者 ID，可用于后续取消订阅
    pub async fn subscribe_session(
        &self,
        subscriber: Arc<dyn SessionEventSubscriber>,
    ) -> String {
        self.event_bus.subscribe_session(subscriber).await
    }

    /// 注册 Conversation 事件订阅者
    ///
    /// # 参数
    /// * `subscriber` - Conversation 事件订阅者
    ///
    /// # 返回
    /// 返回订阅者 ID，可用于后续取消订阅
    pub async fn subscribe_conversation(
        &self,
        subscriber: Arc<dyn ConversationEventSubscriber>,
    ) -> String {
        self.event_bus.subscribe_conversation(subscriber).await
    }

    /// 注册 Sync 事件订阅者
    ///
    /// # 参数
    /// * `subscriber` - Sync 事件订阅者
    ///
    /// # 返回
    /// 返回订阅者 ID，可用于后续取消订阅
    pub async fn subscribe_sync(
        &self,
        subscriber: Arc<dyn SyncEventSubscriber>,
    ) -> String {
        self.event_bus.subscribe_sync(subscriber).await
    }

    // ============================================================================
    // 取消订阅方法
    // ============================================================================

    /// 取消 Message 事件订阅
    ///
    /// # 参数
    /// * `id` - 订阅者 ID（由 subscribe_message 返回）
    ///
    /// # 返回
    /// * `true` - 成功取消订阅
    /// * `false` - 订阅者不存在
    pub async fn unsubscribe_message(&self, id: &str) -> bool {
        self.event_bus.unsubscribe_message(id).await
    }

    /// 取消 Connection 事件订阅
    pub async fn unsubscribe_connection(&self, id: &str) -> bool {
        self.event_bus.unsubscribe_connection(id).await
    }

    /// 取消 Session 事件订阅
    pub async fn unsubscribe_session(&self, id: &str) -> bool {
        self.event_bus.unsubscribe_session(id).await
    }

    /// 取消 Conversation 事件订阅
    pub async fn unsubscribe_conversation(&self, id: &str) -> bool {
        self.event_bus.unsubscribe_conversation(id).await
    }

    /// 取消 Sync 事件订阅
    pub async fn unsubscribe_sync(&self, id: &str) -> bool {
        self.event_bus.unsubscribe_sync(id).await
    }

    /// Gets subscription statistics
    ///
    /// Returns statistics about the number of subscribers for each event type.
    ///
    /// # Returns
    ///
    /// Returns a [`SubscriptionStatistics`] struct containing subscriber counts
    /// for each event type.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::EventSubscriptionFacade;
    /// # async fn example(facade: &EventSubscriptionFacade) {
    /// let stats = facade.get_statistics().await;
    /// println!("Message subscribers: {}", stats.message_count);
    /// # }
    /// ```
    pub async fn get_statistics(&self) -> SubscriptionStatistics {
        self.event_bus.get_statistics().await
    }
}
