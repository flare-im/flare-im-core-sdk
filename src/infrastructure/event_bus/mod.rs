//! 事件总线基础设施实现
//!
//! 按照 DDD + CQRS 原则，基础设施层负责实现事件总线的具体实现
//!
//! ## 模块结构
//!
//! - `event_bus.rs`: 事件总线实现（基于 tokio broadcast）
//! - `subscription_manager.rs`: 订阅管理器实现
//! - `filter.rs`: 事件过滤器实现
//! - `subscription_entry.rs`: 订阅者条目数据结构

pub mod event_bus;
pub mod subscription_manager;
pub mod filter;
pub mod subscription_entry;

pub use event_bus::EventBus;
pub use subscription_manager::{EventSubscriptionManager, SubscriptionStatistics};
pub use filter::{EventFilter, EventTypeFilter, AggregateIdFilter, CompositeFilter, NoFilter};
pub use subscription_entry::*;
