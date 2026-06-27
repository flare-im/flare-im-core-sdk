//! 同步任务业务实现：会话列表、消息、已读状态（结合 flare-proto / 服务端协议）。
//!
//! 同步 kernel 在 [crate::kernel::sync]；本模块提供可注册的 [SyncTask] 实现。

mod conversation_settings;
mod conversations;
mod key_events;
mod messages;
mod read_states;

pub use conversation_settings::{ConversationSettingsSyncTask, ConversationUserSettingsPatch};
pub use conversations::ConversationsSyncTask;
pub use key_events::KeyEventsSyncTask;
pub use messages::MessagesSyncTask;
pub use read_states::ReadStatesSyncTask;
