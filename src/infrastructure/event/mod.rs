//! 事件系统
//!
//! 提供事件总线和事件类型定义

pub mod bus;
pub mod priority_bus;
pub mod types;

pub use bus::EventBus;
pub use types::*;
