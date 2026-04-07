//! 应用层
//!
//! `usecases` 负责复杂业务编排；`handlers` 仅保留协议适配器/基础服务；`sync_task` 负责同步任务注册。

pub mod commands;
mod adapters;
pub(crate) mod conversation_projection_applier;
pub(crate) mod event_deduper;
pub(crate) mod incoming_message_converger;
pub(crate) mod message_deduper;
pub mod message_builder;
pub mod queries;
pub mod sync_task;
pub mod upload_progress;
pub(crate) mod usecases;

pub use commands::{RecallMessageCommand, SendMessageCommand};
pub use adapters::{MediaUploadService, SyncProtocolAdapter};
pub use message_builder::MessageBuilderService;
pub use queries::{
    GetConversationQuery, GetConversationsQuery, GetMessagesQuery, SearchMessagesQuery,
};
pub use sync_task::{ConversationsSyncTask, MessagesSyncTask, ReadStatesSyncTask};
pub use upload_progress::{UploadPhase, UploadProgress, UploadProgressCallback};
