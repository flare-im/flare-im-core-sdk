//! Infrastructure Layer
//!
//! 基础设施层实现
//!
//! 按照 DDD + CQRS 原则，基础设施层负责：
//! - 事件总线实现
//! - 存储实现
//! - 网络实现
//! - 其他外部系统适配

pub mod storage;
pub mod network;
pub mod clock;
pub mod converter;
pub mod metrics;
pub mod messaging;
pub mod event_bus;
