use std::sync::Arc;
use tokio::sync::broadcast;
use crate::domain::event::DomainEvent;
use crate::domain::event::subscribers::*;
use super::subscription_manager::{EventSubscriptionManager, SubscriptionStatistics};

pub struct EventBus {
    sender: broadcast::Sender<DomainEvent>,
    subscription_manager: Arc<EventSubscriptionManager>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        Self {
            sender: broadcast::channel(capacity).0,
            subscription_manager: Arc::new(EventSubscriptionManager::new()),
        }
    }
    
    pub async fn publish(&self, event: DomainEvent) -> anyhow::Result<()> {
        let _ = self.sender.send(event.clone());
        
        let subscription_manager = self.subscription_manager.clone();
        tokio::spawn(async move {
            subscription_manager.dispatch(&event).await;
        });
        
        Ok(())
    }
    
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
