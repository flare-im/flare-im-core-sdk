//! 消息服务模块
//!
//! 提供消息发送、接收、本地存储等核心功能

mod queue;
mod service;

pub use queue::{MessageQueue, MessageQueueConfig, MessagePriority, MessageBatchProcessor};
pub use service::{MessageService, SendOptions};
