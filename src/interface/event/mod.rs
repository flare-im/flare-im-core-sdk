//! Event 接口适配层
//!
//! 按照 DDD + CQRS 原则，Interface 层只负责适配器，提供便捷的 API
//!
//! ## 模块结构
//!
//! - `subscriber_builder.rs`: 订阅器构建器（链式 API，接口适配器）
//!
//! ## 架构说明
//!
//! - **Domain 层** (`domain/event/subscribers.rs`): 定义事件订阅器 trait（领域接口）
//! - **Infrastructure 层** (`infrastructure/event_bus/`): 实现事件总线、订阅管理器等
//! - **Interface 层** (`interface/event/`): 提供适配器，简化用户使用

pub mod subscriber_builder;

// 重新导出领域接口（方便用户使用）
pub use crate::domain::event::subscribers::*;

// 重新导出基础设施实现（方便用户使用）
pub use crate::infrastructure::event_bus::{
    EventBus, EventSubscriptionManager, SubscriptionStatistics,
    EventFilter, EventTypeFilter, AggregateIdFilter, CompositeFilter, NoFilter,
};

// 重新导出适配器
pub use subscriber_builder::SubscriberBuilder;
