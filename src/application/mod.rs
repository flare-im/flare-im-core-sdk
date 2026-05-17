//! 应用层
//!
//! `usecases` 负责复杂业务编排；`handlers` 仅保留协议适配器/基础服务；`sync_task` 负责同步任务注册。

mod adapters;
pub mod commands;
pub(crate) mod conversation_projection_applier;
pub(crate) mod event_deduper;
pub(crate) mod incoming_message_converger;
pub mod message_builder;
pub(crate) mod message_deduper;
pub mod queries;
pub mod sdk_callbacks;
pub mod sync_task;
pub(crate) mod usecases;

pub use adapters::{MediaService, SyncProtocolAdapter};
pub use commands::{RecallMessageCommand, SendMessageCommand};
pub use message_builder::MessageBuilderService;
pub use queries::{
    GetConversationQuery, GetConversationsQuery, GetMessagesQuery, SearchMessagesQuery,
};
pub use sdk_callbacks::{
    FileDownloadProgress, FileDownloadProgressCallback, UploadPhase, UploadProgress,
    UploadProgressCallback,
};
pub use sync_task::{ConversationsSyncTask, MessagesSyncTask, ReadStatesSyncTask};
