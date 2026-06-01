//! 可靠发送队列（Reliable Queue）
//!
//! 流程：enqueue → send → ack → remove；含 retry、timeout 与持久化。
//! 保证不丢消息、断线可恢复；未确认消息落 domain PendingSendReader/PendingSendWriter。

mod actor;

pub use actor::{QueueCommand, ReliableSendQueue, ReliableSendQueueConfig};
