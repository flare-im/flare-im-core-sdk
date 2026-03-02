//! Event Interface Adapter Layer
//!
//! Provides convenient APIs for event subscription and handling, following
//! DDD + CQRS principles. The interface layer acts as an adapter that simplifies
//! the use of domain and infrastructure components.
//!
//! ## Module Structure
//!
//! - [`subscriber_builder`]: Subscriber builder with fluent API
//!
//! ## Architecture
//!
//! - **Domain Layer** (`domain/event/subscribers.rs`): Defines event subscriber traits
//! - **Infrastructure Layer** (`infrastructure/event_bus/`): Implements event bus and subscription manager
//! - **Interface Layer** (this module): Provides adapters to simplify usage
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
//!     .build()
//!     .await;
//! # }
//! ```

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
