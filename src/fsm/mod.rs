//! 有限状态机（FSM）
//!
//! 连接、消息、同步三类状态；转移显式定义、由事件触发，保证流程不乱。

mod connection;
mod message_state;
mod sync_state;

pub use connection::{ConnectionEvent, ConnectionFsm, ConnectionState};
pub use message_state::{MessageState, MessageStateEvent, MessageStateFsm};
pub use sync_state::{SyncFsm, SyncState, SyncTransition};
