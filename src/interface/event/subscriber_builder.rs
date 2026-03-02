//! Event Subscriber Builder
//!
//! Provides a fluent API for registering multiple event subscribers at once.
//!
//! ## Example
//!
//! ```no_run
//! use flare_im_core_sdk::interface::event::SubscriberBuilder;
//! use flare_im_core_sdk::interface::event::subscribers::*;
//! use std::sync::Arc;
//!
//! # async fn example(event_bus: Arc<flare_im_core_sdk::infrastructure::event_bus::EventBus>) {
//! SubscriberBuilder::new(event_bus)
//!     .message(Arc::new(MyMessageSubscriber))
//!     .connection(Arc::new(MyConnectionSubscriber))
//!     .session(Arc::new(MySessionSubscriber))
//!     .conversation(Arc::new(MyConversationSubscriber))
//!     .sync(Arc::new(MySyncSubscriber))
//!     .build()
//!     .await;
//! # }
//! ```

use std::sync::Arc;
use crate::infrastructure::event_bus::EventBus;
use crate::domain::event::subscribers::*;

/// Event subscriber builder with fluent API
///
/// Allows registering multiple subscribers in a chain before building.
///
/// # Example
///
/// ```no_run
/// use flare_im_core_sdk::interface::event::SubscriberBuilder;
/// use flare_im_core_sdk::interface::event::subscribers::*;
/// use std::sync::Arc;
///
/// # async fn example(event_bus: Arc<flare_im_core_sdk::infrastructure::event_bus::EventBus>) {
/// SubscriberBuilder::new(event_bus)
///     .message(Arc::new(MyMessageSubscriber))
///     .connection(Arc::new(MyConnectionSubscriber))
///     .build()
///     .await;
/// # }
/// ```
pub struct SubscriberBuilder {
    event_bus: Arc<EventBus>,
    message_subscribers: Vec<Arc<dyn MessageEventSubscriber>>,
    connection_subscribers: Vec<Arc<dyn ConnectionEventSubscriber>>,
    session_subscribers: Vec<Arc<dyn SessionEventSubscriber>>,
    conversation_subscribers: Vec<Arc<dyn ConversationEventSubscriber>>,
    sync_subscribers: Vec<Arc<dyn SyncEventSubscriber>>,
}

impl SubscriberBuilder {
    /// Creates a new subscriber builder
    ///
    /// # Arguments
    ///
    /// * `event_bus` - The event bus to register subscribers with
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

    /// Adds a message event subscriber
    ///
    /// # Arguments
    ///
    /// * `subscriber` - The message event subscriber
    pub fn message(mut self, subscriber: Arc<dyn MessageEventSubscriber>) -> Self {
        self.message_subscribers.push(subscriber);
        self
    }

    /// Adds a connection event subscriber
    ///
    /// # Arguments
    ///
    /// * `subscriber` - The connection event subscriber
    pub fn connection(mut self, subscriber: Arc<dyn ConnectionEventSubscriber>) -> Self {
        self.connection_subscribers.push(subscriber);
        self
    }

    /// Adds a session event subscriber
    ///
    /// # Arguments
    ///
    /// * `subscriber` - The session event subscriber
    pub fn session(mut self, subscriber: Arc<dyn SessionEventSubscriber>) -> Self {
        self.session_subscribers.push(subscriber);
        self
    }

    /// Adds a conversation event subscriber
    ///
    /// # Arguments
    ///
    /// * `subscriber` - The conversation event subscriber
    pub fn conversation(mut self, subscriber: Arc<dyn ConversationEventSubscriber>) -> Self {
        self.conversation_subscribers.push(subscriber);
        self
    }

    /// Adds a sync event subscriber
    ///
    /// # Arguments
    ///
    /// * `subscriber` - The sync event subscriber
    pub fn sync(mut self, subscriber: Arc<dyn SyncEventSubscriber>) -> Self {
        self.sync_subscribers.push(subscriber);
        self
    }

    /// Builds and registers all subscribers
    ///
    /// This method registers all added subscribers with the event bus.
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
