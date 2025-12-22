//! 订阅者条目
//!
//! 管理订阅者的元数据和生命周期

use std::sync::Arc;
use uuid::Uuid;
use crate::domain::event::subscribers::*;
use super::filter::EventFilter;

/// 订阅者条目
///
/// 包含订阅者实例、唯一 ID 和可选的过滤器
pub struct SubscriptionEntry {
    /// 订阅者唯一 ID
    pub id: String,
    
    /// 可选的过滤器（如果为 None，则处理所有事件）
    pub filter: Option<Box<dyn EventFilter>>,
}

impl SubscriptionEntry {
    /// 创建新的订阅者条目
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            filter: None,
        }
    }

    /// 创建带过滤器的订阅者条目
    pub fn with_filter(filter: Box<dyn EventFilter>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            filter: Some(filter),
        }
    }

    /// 检查事件是否应该被处理
    pub fn should_process(&self, event: &crate::domain::event::DomainEvent) -> bool {
        match &self.filter {
            Some(filter) => filter.should_process(event),
            None => true,
        }
    }
}

/// Connection 订阅者条目（包含订阅者和元数据）
pub struct ConnectionSubscriptionEntry {
    pub entry: SubscriptionEntry,
    pub subscriber: Arc<dyn ConnectionEventSubscriber>,
}

impl ConnectionSubscriptionEntry {
    pub fn new(subscriber: Arc<dyn ConnectionEventSubscriber>) -> Self {
        Self {
            entry: SubscriptionEntry::new(),
            subscriber,
        }
    }
}

/// Session 订阅者条目
pub struct SessionSubscriptionEntry {
    pub entry: SubscriptionEntry,
    pub subscriber: Arc<dyn SessionEventSubscriber>,
}

impl SessionSubscriptionEntry {
    pub fn new(subscriber: Arc<dyn SessionEventSubscriber>) -> Self {
        Self {
            entry: SubscriptionEntry::new(),
            subscriber,
        }
    }
}

/// Message 订阅者条目
pub struct MessageSubscriptionEntry {
    pub entry: SubscriptionEntry,
    pub subscriber: Arc<dyn MessageEventSubscriber>,
}

impl MessageSubscriptionEntry {
    pub fn new(subscriber: Arc<dyn MessageEventSubscriber>) -> Self {
        Self {
            entry: SubscriptionEntry::new(),
            subscriber,
        }
    }
}

/// Conversation 订阅者条目
pub struct ConversationSubscriptionEntry {
    pub entry: SubscriptionEntry,
    pub subscriber: Arc<dyn ConversationEventSubscriber>,
}

impl ConversationSubscriptionEntry {
    pub fn new(subscriber: Arc<dyn ConversationEventSubscriber>) -> Self {
        Self {
            entry: SubscriptionEntry::new(),
            subscriber,
        }
    }
}

/// Sync 订阅者条目
pub struct SyncSubscriptionEntry {
    pub entry: SubscriptionEntry,
    pub subscriber: Arc<dyn SyncEventSubscriber>,
}

impl SyncSubscriptionEntry {
    pub fn new(subscriber: Arc<dyn SyncEventSubscriber>) -> Self {
        Self {
            entry: SubscriptionEntry::new(),
            subscriber,
        }
    }
}
