//! 事件过滤器
//!
//! 提供事件过滤功能，允许订阅者只接收感兴趣的事件

use crate::domain::event::DomainEvent;

/// 事件过滤器 Trait
///
/// 实现此 trait 以过滤事件，只有通过过滤器的事件才会被分发给订阅者
pub trait EventFilter: Send + Sync {
    /// 检查事件是否应该被处理
    ///
    /// # 参数
    /// * `event` - 要检查的事件
    ///
    /// # 返回
    /// * `true` - 事件应该被处理
    /// * `false` - 事件应该被忽略
    fn should_process(&self, event: &DomainEvent) -> bool;
}

/// 事件类型过滤器
///
/// 只允许指定类型的事件通过
pub struct EventTypeFilter {
    allowed_types: Vec<String>,
}

impl EventTypeFilter {
    /// 创建新的事件类型过滤器
    ///
    /// # 参数
    /// * `allowed_types` - 允许的事件类型列表
    pub fn new(allowed_types: Vec<String>) -> Self {
        Self { allowed_types }
    }

    /// 创建只允许单个事件类型的过滤器
    pub fn single(event_type: impl Into<String>) -> Self {
        Self {
            allowed_types: vec![event_type.into()],
        }
    }
}

impl EventFilter for EventTypeFilter {
    fn should_process(&self, event: &DomainEvent) -> bool {
        self.allowed_types.contains(&event.event_type)
    }
}

/// 聚合根 ID 过滤器
///
/// 只允许指定聚合根 ID 的事件通过
pub struct AggregateIdFilter {
    allowed_ids: Vec<String>,
}

impl AggregateIdFilter {
    /// 创建新的聚合根 ID 过滤器
    ///
    /// # 参数
    /// * `allowed_ids` - 允许的聚合根 ID 列表
    pub fn new(allowed_ids: Vec<String>) -> Self {
        Self { allowed_ids }
    }

    /// 创建只允许单个聚合根 ID 的过滤器
    pub fn single(aggregate_id: impl Into<String>) -> Self {
        Self {
            allowed_ids: vec![aggregate_id.into()],
        }
    }
}

impl EventFilter for AggregateIdFilter {
    fn should_process(&self, event: &DomainEvent) -> bool {
        self.allowed_ids.contains(&event.aggregate_id)
    }
}

/// 组合过滤器
///
/// 支持 AND、OR 逻辑组合多个过滤器
pub enum CompositeFilter {
    /// AND 逻辑：所有过滤器都必须通过
    And(Vec<Box<dyn EventFilter>>),
    /// OR 逻辑：至少一个过滤器通过即可
    Or(Vec<Box<dyn EventFilter>>),
}

impl CompositeFilter {
    /// 创建 AND 组合过滤器
    pub fn and(filters: Vec<Box<dyn EventFilter>>) -> Self {
        Self::And(filters)
    }

    /// 创建 OR 组合过滤器
    pub fn or(filters: Vec<Box<dyn EventFilter>>) -> Self {
        Self::Or(filters)
    }
}

impl EventFilter for CompositeFilter {
    fn should_process(&self, event: &DomainEvent) -> bool {
        match self {
            Self::And(filters) => filters.iter().all(|f| f.should_process(event)),
            Self::Or(filters) => filters.iter().any(|f| f.should_process(event)),
        }
    }
}

/// 无过滤器（允许所有事件通过）
pub struct NoFilter;

impl EventFilter for NoFilter {
    fn should_process(&self, _event: &DomainEvent) -> bool {
        true
    }
}
