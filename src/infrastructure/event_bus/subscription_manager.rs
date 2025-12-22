//! 事件订阅管理器
//!
//! 负责管理所有事件订阅者，并在事件发布时自动分发到对应的订阅者
//!
//! ## 设计特点
//!
//! 1. **多订阅者支持**: 每种事件类型可以注册多个订阅者
//! 2. **异步分发**: 事件分发是异步的，不阻塞发布者
//! 3. **错误隔离**: 单个订阅者的错误不会影响其他订阅者
//! 4. **类型安全**: 编译期保证类型安全

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, warn, info, debug};
use crate::domain::event::DomainEvent;
use crate::domain::event::{
    connection_events, session_events, message_events, conversation_events, sync_events,
};
use crate::domain::event::subscribers::*;
use super::subscription_entry::*;

/// 事件订阅管理器
///
/// 管理所有类型的事件订阅者，并在事件发布时自动分发
pub struct EventSubscriptionManager {
    // Connection 订阅者列表（带 ID 和过滤器）
    connection_subscribers: Arc<RwLock<Vec<ConnectionSubscriptionEntry>>>,
    
    // Session 订阅者列表
    session_subscribers: Arc<RwLock<Vec<SessionSubscriptionEntry>>>,
    
    // Message 订阅者列表
    message_subscribers: Arc<RwLock<Vec<MessageSubscriptionEntry>>>,
    
    // Conversation 订阅者列表
    conversation_subscribers: Arc<RwLock<Vec<ConversationSubscriptionEntry>>>,
    
    // Sync 订阅者列表
    sync_subscribers: Arc<RwLock<Vec<SyncSubscriptionEntry>>>,
}

impl EventSubscriptionManager {
    /// 创建新的订阅管理器
    pub fn new() -> Self {
        Self {
            connection_subscribers: Arc::new(RwLock::new(Vec::new())),
            session_subscribers: Arc::new(RwLock::new(Vec::new())),
            message_subscribers: Arc::new(RwLock::new(Vec::new())),
            conversation_subscribers: Arc::new(RwLock::new(Vec::new())),
            sync_subscribers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    // ============================================================================
    // 订阅者注册方法
    // ============================================================================

    /// 注册 Connection 事件订阅者
    ///
    /// # 返回
    /// 返回订阅者 ID，可用于后续取消订阅
    pub async fn subscribe_connection(
        &self,
        subscriber: Arc<dyn ConnectionEventSubscriber>,
    ) -> String {
        let entry = ConnectionSubscriptionEntry::new(subscriber);
        let id = entry.entry.id.clone();
        let mut subscribers = self.connection_subscribers.write().await;
        subscribers.push(entry);
        info!("Connection event subscriber registered: {}", id);
        id
    }

    /// 注册 Session 事件订阅者
    ///
    /// # 返回
    /// 返回订阅者 ID，可用于后续取消订阅
    pub async fn subscribe_session(
        &self,
        subscriber: Arc<dyn SessionEventSubscriber>,
    ) -> String {
        let entry = SessionSubscriptionEntry::new(subscriber);
        let id = entry.entry.id.clone();
        let mut subscribers = self.session_subscribers.write().await;
        subscribers.push(entry);
        info!("Session event subscriber registered: {}", id);
        id
    }

    /// 注册 Message 事件订阅者
    ///
    /// # 返回
    /// 返回订阅者 ID，可用于后续取消订阅
    pub async fn subscribe_message(
        &self,
        subscriber: Arc<dyn MessageEventSubscriber>,
    ) -> String {
        let entry = MessageSubscriptionEntry::new(subscriber);
        let id = entry.entry.id.clone();
        let mut subscribers = self.message_subscribers.write().await;
        subscribers.push(entry);
        info!("Message event subscriber registered: {}", id);
        id
    }

    /// 注册 Conversation 事件订阅者
    ///
    /// # 返回
    /// 返回订阅者 ID，可用于后续取消订阅
    pub async fn subscribe_conversation(
        &self,
        subscriber: Arc<dyn ConversationEventSubscriber>,
    ) -> String {
        let entry = ConversationSubscriptionEntry::new(subscriber);
        let id = entry.entry.id.clone();
        let mut subscribers = self.conversation_subscribers.write().await;
        subscribers.push(entry);
        info!("Conversation event subscriber registered: {}", id);
        id
    }

    /// 注册 Sync 事件订阅者
    ///
    /// # 返回
    /// 返回订阅者 ID，可用于后续取消订阅
    pub async fn subscribe_sync(
        &self,
        subscriber: Arc<dyn SyncEventSubscriber>,
    ) -> String {
        let entry = SyncSubscriptionEntry::new(subscriber);
        let id = entry.entry.id.clone();
        let mut subscribers = self.sync_subscribers.write().await;
        subscribers.push(entry);
        info!("Sync event subscriber registered: {}", id);
        id
    }

    // ============================================================================
    // 取消订阅方法
    // ============================================================================

    /// 取消 Connection 事件订阅
    ///
    /// # 参数
    /// * `id` - 订阅者 ID（由 subscribe_connection 返回）
    ///
    /// # 返回
    /// * `true` - 成功取消订阅
    /// * `false` - 订阅者不存在
    pub async fn unsubscribe_connection(&self, id: &str) -> bool {
        let mut subscribers = self.connection_subscribers.write().await;
        let len_before = subscribers.len();
        subscribers.retain(|entry| entry.entry.id != id);
        let removed = subscribers.len() < len_before;
        if removed {
            info!("Connection event subscriber unregistered: {}", id);
        } else {
            warn!("Connection event subscriber not found: {}", id);
        }
        removed
    }

    /// 取消 Session 事件订阅
    pub async fn unsubscribe_session(&self, id: &str) -> bool {
        let mut subscribers = self.session_subscribers.write().await;
        let len_before = subscribers.len();
        subscribers.retain(|entry| entry.entry.id != id);
        let removed = subscribers.len() < len_before;
        if removed {
            info!("Session event subscriber unregistered: {}", id);
        } else {
            warn!("Session event subscriber not found: {}", id);
        }
        removed
    }

    /// 取消 Message 事件订阅
    pub async fn unsubscribe_message(&self, id: &str) -> bool {
        let mut subscribers = self.message_subscribers.write().await;
        let len_before = subscribers.len();
        subscribers.retain(|entry| entry.entry.id != id);
        let removed = subscribers.len() < len_before;
        if removed {
            info!("Message event subscriber unregistered: {}", id);
        } else {
            warn!("Message event subscriber not found: {}", id);
        }
        removed
    }

    /// 取消 Conversation 事件订阅
    pub async fn unsubscribe_conversation(&self, id: &str) -> bool {
        let mut subscribers = self.conversation_subscribers.write().await;
        let len_before = subscribers.len();
        subscribers.retain(|entry| entry.entry.id != id);
        let removed = subscribers.len() < len_before;
        if removed {
            info!("Conversation event subscriber unregistered: {}", id);
        } else {
            warn!("Conversation event subscriber not found: {}", id);
        }
        removed
    }

    /// 取消 Sync 事件订阅
    pub async fn unsubscribe_sync(&self, id: &str) -> bool {
        let mut subscribers = self.sync_subscribers.write().await;
        let len_before = subscribers.len();
        subscribers.retain(|entry| entry.entry.id != id);
        let removed = subscribers.len() < len_before;
        if removed {
            info!("Sync event subscriber unregistered: {}", id);
        } else {
            warn!("Sync event subscriber not found: {}", id);
        }
        removed
    }

    // ============================================================================
    // 统计方法
    // ============================================================================

    /// 获取订阅者统计信息
    pub async fn get_statistics(&self) -> SubscriptionStatistics {
        let connection_count = self.connection_subscribers.read().await.len();
        let session_count = self.session_subscribers.read().await.len();
        let message_count = self.message_subscribers.read().await.len();
        let conversation_count = self.conversation_subscribers.read().await.len();
        let sync_count = self.sync_subscribers.read().await.len();

        SubscriptionStatistics {
            connection_subscribers: connection_count,
            session_subscribers: session_count,
            message_subscribers: message_count,
            conversation_subscribers: conversation_count,
            sync_subscribers: sync_count,
            total: connection_count + session_count + message_count + conversation_count + sync_count,
        }
    }

    // ============================================================================
    // 事件分发方法
    // ============================================================================
    
    /// 辅助函数：统一处理事件分发（Connection）
    /// 
    /// 性能优化：批量处理，减少 spawn 开销
    fn dispatch_to_connection_subscribers<F>(
        subscribers: &[ConnectionSubscriptionEntry],
        event: &DomainEvent,
        handler: F,
    ) where
        F: Fn(&Arc<dyn ConnectionEventSubscriber>) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>> + Send + Sync + Clone + 'static,
    {
        if subscribers.is_empty() {
            return;
        }
        
        // 性能优化：直接 spawn，tokio 的任务调度器已经优化过
        // 对于高频事件，fire-and-forget 模式性能最好
        for entry in subscribers.iter() {
            if !entry.entry.should_process(event) {
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let handler = handler.clone();
            let entry_id = entry.entry.id.clone();
            let event_type = event.event_type.clone();
            tokio::spawn(async move {
                if let Err(e) = handler(&subscriber).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        event_type = %event_type,
                        "ConnectionEventSubscriber handler failed"
                    );
                }
            });
        }
    }
    
    /// 辅助函数：统一处理事件分发（Session）
    fn dispatch_to_session_subscribers<F>(
        subscribers: &[SessionSubscriptionEntry],
        event: &DomainEvent,
        handler: F,
    ) where
        F: Fn(Arc<dyn SessionEventSubscriber>) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>> + Send + Sync + Clone + 'static,
    {
        if subscribers.is_empty() {
            return;
        }
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(event) {
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let handler = handler.clone();
            let entry_id = entry.entry.id.clone();
            let event_type = event.event_type.clone();
            tokio::spawn(async move {
                if let Err(e) = handler(subscriber).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        event_type = %event_type,
                        "SessionEventSubscriber handler failed"
                    );
                }
            });
        }
    }
    
    /// 辅助函数：统一处理事件分发（Message）
    /// 
    /// 性能优化：消息事件是高频事件，使用优化的分发策略
    fn dispatch_to_message_subscribers<F>(
        subscribers: &[MessageSubscriptionEntry],
        event: &DomainEvent,
        handler: F,
    ) where
        F: Fn(&Arc<dyn MessageEventSubscriber>) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>> + Send + Sync + Clone + 'static,
    {
        if subscribers.is_empty() {
            return;
        }
        
        // 性能优化：消息事件是高频事件，直接 spawn（低延迟优先）
        for entry in subscribers.iter() {
            if !entry.entry.should_process(event) {
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let handler = handler.clone();
            let entry_id = entry.entry.id.clone();
            let event_type = event.event_type.clone();
            tokio::spawn(async move {
                if let Err(e) = handler(&subscriber).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        event_type = %event_type,
                        "MessageEventSubscriber handler failed"
                    );
                }
            });
        }
    }
    
    /// 辅助函数：统一处理事件分发（Conversation）
    fn dispatch_to_conversation_subscribers<F>(
        subscribers: &[ConversationSubscriptionEntry],
        event: &DomainEvent,
        handler: F,
    ) where
        F: Fn(&Arc<dyn ConversationEventSubscriber>) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>> + Send + Sync + Clone + 'static,
    {
        if subscribers.is_empty() {
            return;
        }
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(event) {
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let handler = handler.clone();
            let entry_id = entry.entry.id.clone();
            let event_type = event.event_type.clone();
            tokio::spawn(async move {
                if let Err(e) = handler(&subscriber).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        event_type = %event_type,
                        "ConversationEventSubscriber handler failed"
                    );
                }
            });
        }
    }
    
    /// 辅助函数：统一处理事件分发（Sync）
    fn dispatch_to_sync_subscribers<F>(
        subscribers: &[SyncSubscriptionEntry],
        event: &DomainEvent,
        handler: F,
    ) where
        F: Fn(Arc<dyn SyncEventSubscriber>) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>> + Send + Sync + Clone + 'static,
    {
        if subscribers.is_empty() {
            return;
        }
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(event) {
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let handler = handler.clone();
            let entry_id = entry.entry.id.clone();
            let event_type = event.event_type.clone();
            tokio::spawn(async move {
                if let Err(e) = handler(subscriber).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        event_type = %event_type,
                        "SyncEventSubscriber handler failed"
                    );
                }
            });
        }
    }

    /// 分发事件到对应的订阅者
    ///
    /// 根据事件类型自动路由到对应的订阅者，并异步处理
    pub async fn dispatch(&self, event: &DomainEvent) {
        match event.event_type.as_str() {
            // Connection 事件
            connection_events::CONNECTED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConnectionConnected>(
                    event.data.clone()
                ) {
                    self.dispatch_connection_connected(&data).await;
                }
            }
            connection_events::DISCONNECTED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConnectionDisconnected>(
                    event.data.clone()
                ) {
                    self.dispatch_connection_disconnected(&data).await;
                }
            }
            connection_events::RECONNECTING => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConnectionReconnecting>(
                    event.data.clone()
                ) {
                    self.dispatch_connection_reconnecting(&data).await;
                }
            }
            connection_events::RECONNECTED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConnectionReconnected>(
                    event.data.clone()
                ) {
                    self.dispatch_connection_reconnected(&data).await;
                }
            }
            connection_events::CONNECT_FAILED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConnectionConnectFailed>(
                    event.data.clone()
                ) {
                    self.dispatch_connection_connect_failed(&data).await;
                }
            }

            // Session 事件
            session_events::LOGGED_IN => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::SessionLoggedIn>(
                    event.data.clone()
                ) {
                    self.dispatch_session_logged_in(&data).await;
                }
            }
            session_events::LOGGED_OUT => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::SessionLoggedOut>(
                    event.data.clone()
                ) {
                    self.dispatch_session_logged_out(&data).await;
                }
            }
            session_events::EXPIRED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::SessionExpired>(
                    event.data.clone()
                ) {
                    self.dispatch_session_expired(&data).await;
                }
            }
            session_events::TOKEN_REFRESHED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::SessionTokenRefreshed>(
                    event.data.clone()
                ) {
                    self.dispatch_session_token_refreshed(&data).await;
                }
            }

            // Message 事件
            message_events::CREATED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessageCreated>(
                    event.data.clone()
                ) {
                    self.dispatch_message_created(&data).await;
                }
            }
            message_events::SENT => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessageSent>(
                    event.data.clone()
                ) {
                    self.dispatch_message_sent(&data).await;
                }
            }
            message_events::SEND_FAILED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessageSendFailed>(
                    event.data.clone()
                ) {
                    self.dispatch_message_send_failed(&data).await;
                }
            }
            message_events::DELIVERED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessageDelivered>(
                    event.data.clone()
                ) {
                    self.dispatch_message_delivered(&data).await;
                }
            }
            message_events::READ => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessageRead>(
                    event.data.clone()
                ) {
                    self.dispatch_message_read(&data).await;
                }
            }
            message_events::RECALLED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessageRecalled>(
                    event.data.clone()
                ) {
                    self.dispatch_message_recalled(&data).await;
                }
            }
            message_events::EDITED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessageEdited>(
                    event.data.clone()
                ) {
                    self.dispatch_message_edited(&data).await;
                }
            }
            message_events::DELETED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessageDeleted>(
                    event.data.clone()
                ) {
                    self.dispatch_message_deleted(&data).await;
                }
            }
            message_events::REACTION_ADDED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessageReactionAdded>(
                    event.data.clone()
                ) {
                    self.dispatch_message_reaction_added(&data).await;
                }
            }
            message_events::REACTION_REMOVED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessageReactionRemoved>(
                    event.data.clone()
                ) {
                    self.dispatch_message_reaction_removed(&data).await;
                }
            }
            message_events::PINNED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessagePinned>(
                    event.data.clone()
                ) {
                    self.dispatch_message_pinned(&data).await;
                }
            }
            message_events::UNPINNED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessageUnpinned>(
                    event.data.clone()
                ) {
                    self.dispatch_message_unpinned(&data).await;
                }
            }
            message_events::FAVORITED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessageFavorited>(
                    event.data.clone()
                ) {
                    self.dispatch_message_favorited(&data).await;
                }
            }
            message_events::UNFAVORITED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessageUnfavorited>(
                    event.data.clone()
                ) {
                    self.dispatch_message_unfavorited(&data).await;
                }
            }
            message_events::MARKED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessageMarked>(
                    event.data.clone()
                ) {
                    self.dispatch_message_marked(&data).await;
                }
            }
            message_events::UNMARKED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessageUnmarked>(
                    event.data.clone()
                ) {
                    self.dispatch_message_unmarked(&data).await;
                }
            }
            message_events::FORWARDED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessageForwarded>(
                    event.data.clone()
                ) {
                    self.dispatch_message_forwarded(&data).await;
                }
            }
            message_events::REPLIED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::MessageReplied>(
                    event.data.clone()
                ) {
                    self.dispatch_message_replied(&data).await;
                }
            }

            // Conversation 事件
            conversation_events::CREATED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConversationCreated>(
                    event.data.clone()
                ) {
                    self.dispatch_conversation_created(&data).await;
                }
            }
            conversation_events::UNREAD_UPDATED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConversationUnreadUpdated>(
                    event.data.clone()
                ) {
                    self.dispatch_conversation_unread_updated(&data).await;
                }
            }
            conversation_events::LAST_MESSAGE_UPDATED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConversationLastMessageUpdated>(
                    event.data.clone()
                ) {
                    self.dispatch_conversation_last_message_updated(&data).await;
                }
            }
            conversation_events::MARKED_AS_READ => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConversationMarkedAsRead>(
                    event.data.clone()
                ) {
                    self.dispatch_conversation_marked_as_read(&data).await;
                }
            }
            conversation_events::DRAFT_UPDATED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConversationDraftUpdated>(
                    event.data.clone()
                ) {
                    self.dispatch_conversation_draft_updated(&data).await;
                }
            }
            conversation_events::HIDDEN => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConversationHidden>(
                    event.data.clone()
                ) {
                    self.dispatch_conversation_hidden(&data).await;
                }
            }
            conversation_events::ALL_HIDDEN => {
                self.dispatch_conversation_all_hidden().await;
            }
            conversation_events::DELETED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConversationDeleted>(
                    event.data.clone()
                ) {
                    self.dispatch_conversation_deleted(&data).await;
                }
            }
            conversation_events::MESSAGES_CLEARED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConversationMessagesCleared>(
                    event.data.clone()
                ) {
                    self.dispatch_conversation_messages_cleared(&data).await;
                }
            }
            conversation_events::UPDATED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConversationUpdated>(
                    event.data.clone()
                ) {
                    self.dispatch_conversation_updated(&data).await;
                }
            }
            conversation_events::MUTED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConversationMuted>(
                    event.data.clone()
                ) {
                    self.dispatch_conversation_muted(&data).await;
                }
            }
            conversation_events::UNMUTED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConversationUnmuted>(
                    event.data.clone()
                ) {
                    self.dispatch_conversation_unmuted(&data).await;
                }
            }
            conversation_events::PINNED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConversationPinned>(
                    event.data.clone()
                ) {
                    self.dispatch_conversation_pinned(&data).await;
                }
            }
            conversation_events::UNPINNED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConversationUnpinned>(
                    event.data.clone()
                ) {
                    self.dispatch_conversation_unpinned(&data).await;
                }
            }
            conversation_events::ARCHIVED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConversationArchived>(
                    event.data.clone()
                ) {
                    self.dispatch_conversation_archived(&data).await;
                }
            }
            conversation_events::UNARCHIVED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConversationUnarchived>(
                    event.data.clone()
                ) {
                    self.dispatch_conversation_unarchived(&data).await;
                }
            }
            conversation_events::INPUT_STATE_UPDATED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConversationInputStateUpdated>(
                    event.data.clone()
                ) {
                    self.dispatch_conversation_input_state_updated(&data).await;
                }
            }
            conversation_events::INPUT_STATE_CLEARED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::ConversationInputStateCleared>(
                    event.data.clone()
                ) {
                    self.dispatch_conversation_input_state_cleared(&data).await;
                }
            }

            // Sync 事件
            sync_events::BOOTSTRAP_STARTED => {
                self.dispatch_sync_bootstrap_started().await;
            }
            sync_events::BOOTSTRAP_COMPLETED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::SyncBootstrapCompleted>(
                    event.data.clone()
                ) {
                    self.dispatch_sync_bootstrap_completed(&data).await;
                }
            }
            sync_events::BOOTSTRAP_FAILED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::SyncBootstrapFailed>(
                    event.data.clone()
                ) {
                    self.dispatch_sync_bootstrap_failed(&data).await;
                }
            }
            sync_events::ASYNC_STARTED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::SyncAsyncStarted>(
                    event.data.clone()
                ) {
                    self.dispatch_sync_async_started(&data).await;
                }
            }
            sync_events::ASYNC_COMPLETED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::SyncAsyncCompleted>(
                    event.data.clone()
                ) {
                    self.dispatch_sync_async_completed(&data).await;
                }
            }
            sync_events::ASYNC_FAILED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::SyncAsyncFailed>(
                    event.data.clone()
                ) {
                    self.dispatch_sync_async_failed(&data).await;
                }
            }
            sync_events::PROGRESS_UPDATED => {
                if let Ok(data) = serde_json::from_value::<crate::domain::event::SyncProgressUpdated>(
                    event.data.clone()
                ) {
                    self.dispatch_sync_progress_updated(&data).await;
                }
            }

            // 未知事件类型，记录警告但不影响其他事件
            unknown => {
                warn!(
                    event_type = %unknown,
                    "Unknown event type, skipping dispatch"
                );
            }
        }
    }

    // ============================================================================
    // Connection 事件分发方法
    // ============================================================================

    async fn dispatch_connection_connected(&self, event: &crate::domain::event::ConnectionConnected) {
        let subscribers = self.connection_subscribers.read().await;
        let domain_event = DomainEvent::new(
            connection_events::CONNECTED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_connected(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConnectionEventSubscriber::on_connected failed"
                    );
                }
            });
        }
    }

    async fn dispatch_connection_disconnected(&self, event: &crate::domain::event::ConnectionDisconnected) {
        let subscribers = self.connection_subscribers.read().await;
        let domain_event = DomainEvent::new(
            connection_events::DISCONNECTED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_disconnected(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConnectionEventSubscriber::on_disconnected failed"
                    );
                }
            });
        }
    }

    async fn dispatch_connection_reconnecting(&self, event: &crate::domain::event::ConnectionReconnecting) {
        let subscribers = self.connection_subscribers.read().await;
        let domain_event = DomainEvent::new(
            connection_events::RECONNECTING,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_reconnecting(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConnectionEventSubscriber::on_reconnecting failed"
                    );
                }
            });
        }
    }

    async fn dispatch_connection_reconnected(&self, event: &crate::domain::event::ConnectionReconnected) {
        let subscribers = self.connection_subscribers.read().await;
        let domain_event = DomainEvent::new(
            connection_events::RECONNECTED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_reconnected(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConnectionEventSubscriber::on_reconnected failed"
                    );
                }
            });
        }
    }

    async fn dispatch_connection_connect_failed(&self, event: &crate::domain::event::ConnectionConnectFailed) {
        let subscribers = self.connection_subscribers.read().await;
        let domain_event = DomainEvent::new(
            connection_events::CONNECT_FAILED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_connect_failed(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConnectionEventSubscriber::on_connect_failed failed"
                    );
                }
            });
        }
    }

    // ============================================================================
    // Session 事件分发方法
    // ============================================================================

    async fn dispatch_session_logged_in(&self, event: &crate::domain::event::SessionLoggedIn) {
        let subscribers = self.session_subscribers.read().await;
        let domain_event = DomainEvent::new(
            session_events::LOGGED_IN,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        let event_owned = event.clone();
        Self::dispatch_to_session_subscribers(
            &subscribers,
            &domain_event,
            {
                let event_owned = event_owned.clone();
                move |sub: Arc<dyn SessionEventSubscriber>| {
                    let event = event_owned.clone();
                    Box::pin(async move { sub.on_logged_in(&event).await })
                }
            },
        );
    }

    async fn dispatch_session_logged_out(&self, event: &crate::domain::event::SessionLoggedOut) {
        let subscribers = self.session_subscribers.read().await;
        let domain_event = DomainEvent::new(
            session_events::LOGGED_OUT,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        let event_owned = event.clone();
        Self::dispatch_to_session_subscribers(
            &subscribers,
            &domain_event,
            {
                let event_owned = event_owned.clone();
                move |sub: Arc<dyn SessionEventSubscriber>| {
                    let event = event_owned.clone();
                    Box::pin(async move { sub.on_logged_out(&event).await })
                }
            },
        );
    }

    async fn dispatch_session_expired(&self, event: &crate::domain::event::SessionExpired) {
        let subscribers = self.session_subscribers.read().await;
        let domain_event = DomainEvent::new(
            session_events::EXPIRED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        let event_owned = event.clone();
        Self::dispatch_to_session_subscribers(
            &subscribers,
            &domain_event,
            {
                let event_owned = event_owned.clone();
                move |sub: Arc<dyn SessionEventSubscriber>| {
                    let event = event_owned.clone();
                    Box::pin(async move { sub.on_expired(&event).await })
                }
            },
        );
    }

    async fn dispatch_session_token_refreshed(&self, event: &crate::domain::event::SessionTokenRefreshed) {
        let subscribers = self.session_subscribers.read().await;
        let domain_event = DomainEvent::new(
            session_events::TOKEN_REFRESHED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        let event_owned = event.clone();
        Self::dispatch_to_session_subscribers(
            &subscribers,
            &domain_event,
            {
                let event_owned = event_owned.clone();
                move |sub: Arc<dyn SessionEventSubscriber>| {
                    let event = event_owned.clone();
                    Box::pin(async move { sub.on_token_refreshed(&event).await })
                }
            },
        );
    }

    // ============================================================================
    // Message 事件分发方法（部分关键方法，其他类似）
    // ============================================================================

    async fn dispatch_message_created(&self, event: &crate::domain::event::MessageCreated) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::CREATED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_created(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_created failed"
                    );
                }
            });
        }
    }

    async fn dispatch_message_sent(&self, event: &crate::domain::event::MessageSent) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::SENT,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_sent(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_sent failed"
                    );
                }
            });
        }
    }

    async fn dispatch_message_send_failed(&self, event: &crate::domain::event::MessageSendFailed) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::SEND_FAILED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_send_failed(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_send_failed failed"
                    );
                }
            });
        }
    }

    async fn dispatch_message_delivered(&self, event: &crate::domain::event::MessageDelivered) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::DELIVERED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_delivered(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_delivered failed"
                    );
                }
            });
        }
    }

    async fn dispatch_message_read(&self, event: &crate::domain::event::MessageRead) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::READ,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_read(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_read failed"
                    );
                }
            });
        }
    }

    async fn dispatch_message_recalled(&self, event: &crate::domain::event::MessageRecalled) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::RECALLED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_recalled(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_recalled failed"
                    );
                }
            });
        }
    }

    async fn dispatch_message_edited(&self, event: &crate::domain::event::MessageEdited) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::EDITED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_edited(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_edited failed"
                    );
                }
            });
        }
    }

    async fn dispatch_message_deleted(&self, event: &crate::domain::event::MessageDeleted) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::DELETED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_deleted(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_deleted failed"
                    );
                }
            });
        }
    }

    async fn dispatch_message_reaction_added(&self, event: &crate::domain::event::MessageReactionAdded) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::REACTION_ADDED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_reaction_added(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_reaction_added failed"
                    );
                }
            });
        }
    }

    async fn dispatch_message_reaction_removed(&self, event: &crate::domain::event::MessageReactionRemoved) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::REACTION_REMOVED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_reaction_removed(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_reaction_removed failed"
                    );
                }
            });
        }
    }

    async fn dispatch_message_pinned(&self, event: &crate::domain::event::MessagePinned) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::PINNED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_pinned(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_pinned failed"
                    );
                }
            });
        }
    }

    async fn dispatch_message_unpinned(&self, event: &crate::domain::event::MessageUnpinned) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::UNPINNED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_unpinned(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_unpinned failed"
                    );
                }
            });
        }
    }

    async fn dispatch_message_favorited(&self, event: &crate::domain::event::MessageFavorited) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::FAVORITED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_favorited(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_favorited failed"
                    );
                }
            });
        }
    }

    async fn dispatch_message_unfavorited(&self, event: &crate::domain::event::MessageUnfavorited) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::UNFAVORITED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_unfavorited(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_unfavorited failed"
                    );
                }
            });
        }
    }

    async fn dispatch_message_marked(&self, event: &crate::domain::event::MessageMarked) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::MARKED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_marked(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_marked failed"
                    );
                }
            });
        }
    }

    async fn dispatch_message_unmarked(&self, event: &crate::domain::event::MessageUnmarked) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::UNMARKED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_unmarked(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_unmarked failed"
                    );
                }
            });
        }
    }

    async fn dispatch_message_forwarded(&self, event: &crate::domain::event::MessageForwarded) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::FORWARDED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_forwarded(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_forwarded failed"
                    );
                }
            });
        }
    }

    async fn dispatch_message_replied(&self, event: &crate::domain::event::MessageReplied) {
        let subscribers = self.message_subscribers.read().await;
        let domain_event = DomainEvent::new(
            message_events::REPLIED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_message_replied(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "MessageEventSubscriber::on_message_replied failed"
                    );
                }
            });
        }
    }

    // ============================================================================
    // Conversation 事件分发方法（部分关键方法）
    // ============================================================================

    async fn dispatch_conversation_created(&self, event: &crate::domain::event::ConversationCreated) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::CREATED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_conversation_created(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_conversation_created failed"
                    );
                }
            });
        }
    }

    async fn dispatch_conversation_unread_updated(&self, event: &crate::domain::event::ConversationUnreadUpdated) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::UNREAD_UPDATED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_unread_updated(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_unread_updated failed"
                    );
                }
            });
        }
    }

    async fn dispatch_conversation_last_message_updated(&self, event: &crate::domain::event::ConversationLastMessageUpdated) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::LAST_MESSAGE_UPDATED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_last_message_updated(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_last_message_updated failed"
                    );
                }
            });
        }
    }

    async fn dispatch_conversation_marked_as_read(&self, event: &crate::domain::event::ConversationMarkedAsRead) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::MARKED_AS_READ,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_marked_as_read(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_marked_as_read failed"
                    );
                }
            });
        }
    }

    async fn dispatch_conversation_draft_updated(&self, event: &crate::domain::event::ConversationDraftUpdated) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::DRAFT_UPDATED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_draft_updated(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_draft_updated failed"
                    );
                }
            });
        }
    }

    async fn dispatch_conversation_hidden(&self, event: &crate::domain::event::ConversationHidden) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::HIDDEN,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_hidden(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_hidden failed"
                    );
                }
            });
        }
    }

    async fn dispatch_conversation_all_hidden(&self) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::ALL_HIDDEN,
            "",
            0,
            serde_json::json!({}),
        );
                let event = crate::domain::event::ConversationAllHidden;
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_all_hidden(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_all_hidden failed"
                    );
                }
            });
        }
    }

    async fn dispatch_conversation_deleted(&self, event: &crate::domain::event::ConversationDeleted) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::DELETED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_deleted(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_deleted failed"
                    );
                }
            });
        }
    }

    async fn dispatch_conversation_messages_cleared(&self, event: &crate::domain::event::ConversationMessagesCleared) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::MESSAGES_CLEARED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_messages_cleared(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_messages_cleared failed"
                    );
                }
            });
        }
    }

    async fn dispatch_conversation_updated(&self, event: &crate::domain::event::ConversationUpdated) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::UPDATED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_updated(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_updated failed"
                    );
                }
            });
        }
    }

    async fn dispatch_conversation_muted(&self, event: &crate::domain::event::ConversationMuted) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::MUTED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_muted(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_muted failed"
                    );
                }
            });
        }
    }

    async fn dispatch_conversation_unmuted(&self, event: &crate::domain::event::ConversationUnmuted) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::UNMUTED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_unmuted(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_unmuted failed"
                    );
                }
            });
        }
    }

    async fn dispatch_conversation_pinned(&self, event: &crate::domain::event::ConversationPinned) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::PINNED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_pinned(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_pinned failed"
                    );
                }
            });
        }
    }

    async fn dispatch_conversation_unpinned(&self, event: &crate::domain::event::ConversationUnpinned) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::UNPINNED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_unpinned(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_unpinned failed"
                    );
                }
            });
        }
    }

    async fn dispatch_conversation_archived(&self, event: &crate::domain::event::ConversationArchived) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::ARCHIVED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_archived(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_archived failed"
                    );
                }
            });
        }
    }

    async fn dispatch_conversation_unarchived(&self, event: &crate::domain::event::ConversationUnarchived) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::UNARCHIVED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_unarchived(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_unarchived failed"
                    );
                }
            });
        }
    }

    async fn dispatch_conversation_input_state_updated(&self, event: &crate::domain::event::ConversationInputStateUpdated) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::INPUT_STATE_UPDATED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_input_state_updated(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_input_state_updated failed"
                    );
                }
            });
        }
    }

    async fn dispatch_conversation_input_state_cleared(&self, event: &crate::domain::event::ConversationInputStateCleared) {
        let subscribers = self.conversation_subscribers.read().await;
        let domain_event = DomainEvent::new(
            conversation_events::INPUT_STATE_CLEARED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        
        for entry in subscribers.iter() {
            if !entry.entry.should_process(&domain_event) {
                debug!("Event filtered out for subscriber: {}", entry.entry.id);
                continue;
            }
            
            let subscriber = entry.subscriber.clone();
            let event = event.clone();
            let entry_id = entry.entry.id.clone();
            tokio::spawn(async move {
                if let Err(e) = subscriber.on_input_state_cleared(&event).await {
                    error!(
                        subscriber_id = %entry_id,
                        error = %e,
                        "ConversationEventSubscriber::on_input_state_cleared failed"
                    );
                }
            });
        }
    }

    // ============================================================================
    // Sync 事件分发方法
    // ============================================================================

    async fn dispatch_sync_bootstrap_started(&self) {
        let subscribers = self.sync_subscribers.read().await;
        let domain_event = DomainEvent::new(
            sync_events::BOOTSTRAP_STARTED,
            "",
            0,
            serde_json::json!({}),
        );
        let event_owned = crate::domain::event::SyncBootstrapStarted;
        Self::dispatch_to_sync_subscribers(
            &subscribers,
            &domain_event,
            {
                let event_owned = event_owned.clone();
                move |sub: Arc<dyn SyncEventSubscriber>| {
                    let event = event_owned.clone();
                    Box::pin(async move { sub.on_bootstrap_started(&event).await })
                }
            },
        );
    }

    async fn dispatch_sync_bootstrap_completed(&self, event: &crate::domain::event::SyncBootstrapCompleted) {
        let subscribers = self.sync_subscribers.read().await;
        let domain_event = DomainEvent::new(
            sync_events::BOOTSTRAP_COMPLETED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        let event_owned = event.clone();
        Self::dispatch_to_sync_subscribers(
            &subscribers,
            &domain_event,
            {
                let event_owned = event_owned.clone();
                move |sub: Arc<dyn SyncEventSubscriber>| {
                    let event = event_owned.clone();
                    Box::pin(async move { sub.on_bootstrap_completed(&event).await })
                }
            },
        );
    }

    async fn dispatch_sync_bootstrap_failed(&self, event: &crate::domain::event::SyncBootstrapFailed) {
        let subscribers = self.sync_subscribers.read().await;
        let domain_event = DomainEvent::new(
            sync_events::BOOTSTRAP_FAILED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        let event_owned = event.clone();
        Self::dispatch_to_sync_subscribers(
            &subscribers,
            &domain_event,
            {
                let event_owned = event_owned.clone();
                move |sub: Arc<dyn SyncEventSubscriber>| {
                    let event = event_owned.clone();
                    Box::pin(async move { sub.on_bootstrap_failed(&event).await })
                }
            },
        );
    }

    async fn dispatch_sync_async_started(&self, event: &crate::domain::event::SyncAsyncStarted) {
        let subscribers = self.sync_subscribers.read().await;
        let domain_event = DomainEvent::new(
            sync_events::ASYNC_STARTED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        let event_owned = event.clone();
        Self::dispatch_to_sync_subscribers(
            &subscribers,
            &domain_event,
            {
                let event_owned = event_owned.clone();
                move |sub: Arc<dyn SyncEventSubscriber>| {
                    let event = event_owned.clone();
                    Box::pin(async move { sub.on_async_started(&event).await })
                }
            },
        );
    }

    async fn dispatch_sync_async_completed(&self, event: &crate::domain::event::SyncAsyncCompleted) {
        let subscribers = self.sync_subscribers.read().await;
        let domain_event = DomainEvent::new(
            sync_events::ASYNC_COMPLETED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        let event_owned = event.clone();
        Self::dispatch_to_sync_subscribers(
            &subscribers,
            &domain_event,
            {
                let event_owned = event_owned.clone();
                move |sub: Arc<dyn SyncEventSubscriber>| {
                    let event = event_owned.clone();
                    Box::pin(async move { sub.on_async_completed(&event).await })
                }
            },
        );
    }

    async fn dispatch_sync_async_failed(&self, event: &crate::domain::event::SyncAsyncFailed) {
        let subscribers = self.sync_subscribers.read().await;
        let domain_event = DomainEvent::new(
            sync_events::ASYNC_FAILED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        let event_owned = event.clone();
        Self::dispatch_to_sync_subscribers(
            &subscribers,
            &domain_event,
            {
                let event_owned = event_owned.clone();
                move |sub: Arc<dyn SyncEventSubscriber>| {
                    let event = event_owned.clone();
                    Box::pin(async move { sub.on_async_failed(&event).await })
                }
            },
        );
    }

    async fn dispatch_sync_progress_updated(&self, event: &crate::domain::event::SyncProgressUpdated) {
        let subscribers = self.sync_subscribers.read().await;
        let domain_event = DomainEvent::new(
            sync_events::PROGRESS_UPDATED,
            "",
            0,
            serde_json::to_value(event).unwrap_or_default(),
        );
        let event_owned = event.clone();
        Self::dispatch_to_sync_subscribers(
            &subscribers,
            &domain_event,
            {
                let event_owned = event_owned.clone();
                move |sub: Arc<dyn SyncEventSubscriber>| {
                    let event = event_owned.clone();
                    Box::pin(async move { sub.on_progress_updated(&event).await })
                }
            },
        );
    }
}

/// 订阅者统计信息
#[derive(Debug, Clone)]
pub struct SubscriptionStatistics {
    pub connection_subscribers: usize,
    pub session_subscribers: usize,
    pub message_subscribers: usize,
    pub conversation_subscribers: usize,
    pub sync_subscribers: usize,
    pub total: usize,
}

impl Default for EventSubscriptionManager {
    fn default() -> Self {
        Self::new()
    }
}
