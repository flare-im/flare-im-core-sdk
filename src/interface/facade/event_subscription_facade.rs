//! 事件订阅 Facade
//!
//! 提供便捷的事件订阅 API，封装 EventBus 的复杂操作
//!
//! ## 设计原则
//!
//! 1. **单一职责**: 只负责事件订阅相关的 API
//! 2. **便捷性**: 提供多种使用方式，满足不同场景需求
//! 3. **类型安全**: 编译期保证类型正确

use std::sync::Arc;
use crate::infrastructure::event_bus::{EventBus, SubscriptionStatistics};
use crate::domain::event::subscribers::*;
use crate::interface::event::subscriber_builder::SubscriberBuilder;

/// 事件订阅 Facade
///
/// 提供便捷的事件订阅 API，封装 EventBus 的复杂操作
pub struct EventSubscriptionFacade {
    event_bus: Arc<EventBus>,
}

impl EventSubscriptionFacade {
    /// 创建新的事件订阅 Facade
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self { event_bus }
    }

    /// 获取事件总线
    ///
    /// 用于直接访问事件总线的底层功能
    ///
    /// # 示例
    ///
    /// ```rust
    /// use flare_im_core_sdk::interface::event::subscribers::*;
    ///
    /// let event_bus = facade.event_bus();
    /// event_bus.subscribe_message(Arc::new(MyMessageSubscriber)).await;
    /// ```
    pub fn event_bus(&self) -> &Arc<EventBus> {
        &self.event_bus
    }

    /// 创建事件订阅器构建器
    ///
    /// 提供链式 API，方便一次性注册多个订阅者
    ///
    /// # 示例
    ///
    /// ```rust
    /// use flare_im_core_sdk::interface::event::SubscriberBuilder;
    ///
    /// facade.subscribe_events()
    ///     .message(Arc::new(MyMessageSubscriber))
    ///     .connection(Arc::new(MyConnectionSubscriber))
    ///     .build()
    ///     .await;
    /// ```
    pub fn subscribe_events(&self) -> SubscriberBuilder {
        SubscriberBuilder::new(self.event_bus.clone())
    }

    // ============================================================================
    // 订阅方法
    // ============================================================================

    /// 注册 Message 事件订阅者
    ///
    /// # 参数
    /// * `subscriber` - Message 事件订阅者
    ///
    /// # 返回
    /// 返回订阅者 ID，可用于后续取消订阅
    ///
    /// # 示例
    ///
    /// ```rust
    /// use flare_im_core_sdk::interface::event::subscribers::*;
    ///
    /// let subscriber_id = facade.subscribe_message(Arc::new(MyMessageSubscriber)).await;
    /// // 后续可以取消订阅
    /// facade.unsubscribe_message(&subscriber_id).await;
    /// ```
    pub async fn subscribe_message(
        &self,
        subscriber: Arc<dyn MessageEventSubscriber>,
    ) -> String {
        self.event_bus.subscribe_message(subscriber).await
    }

    /// 注册 Connection 事件订阅者
    ///
    /// # 参数
    /// * `subscriber` - Connection 事件订阅者
    ///
    /// # 返回
    /// 返回订阅者 ID，可用于后续取消订阅
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

    // ============================================================================
    // 统计方法
    // ============================================================================

    /// 获取事件订阅者统计信息
    ///
    /// # 返回
    /// 返回各种类型订阅者的数量统计
    pub async fn get_statistics(&self) -> SubscriptionStatistics {
        self.event_bus.get_statistics().await
    }
}
