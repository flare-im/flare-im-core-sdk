//! 消息发送服务
//!
//! 基础设施层：负责消息的网络发送和 ACK 处理

mod message_sender;

pub use message_sender::MessageSender;
