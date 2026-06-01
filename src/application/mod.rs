//! 应用层
//!
//! `usecases` 负责复杂业务编排；`handlers` 仅保留协议适配器/基础服务；`sync_task` 负责同步任务注册。

mod adapters;
pub mod commands;
pub mod conversation_display_projection;
pub mod conversation_local_lifecycle;
pub(crate) mod conversation_projection_applier;
pub(crate) mod event_deduper;
pub(crate) mod incoming_message_converger;
pub mod message_builder;
pub(crate) mod message_deduper;
pub mod queries;
pub mod sdk_callbacks;
pub mod sync_task;
pub(crate) mod usecases;
pub mod user_profile_projection;

pub use adapters::{MediaService, SyncProtocolAdapter};
pub use commands::{RecallMessageCommand, SendMessageCommand};
pub use conversation_display_projection::{
    ConversationDisplayProjectionApplier, ConversationDisplaySnapshot, resolve_display_name,
};
pub use conversation_local_lifecycle::{
    ConversationLocalLifecycle, LocalConversationClearResult, LocalConversationVisibility,
};
pub use message_builder::{
    BuildCardRequest, BuildLinkCardRequest, BuildLocationRequest, BuildMiniProgramRequest,
    BuildRichDocRequest, BuildScheduleRequest, BuildStickerRequest, MessageBuilderService,
};
pub use queries::{
    GetConversationQuery, GetConversationsQuery, GetMessagesQuery, SearchMessagesQuery,
};
pub use sdk_callbacks::{
    FileDownloadProgress, FileDownloadProgressCallback, UploadPhase, UploadProgress,
    UploadProgressCallback, UserFileDownloadRequest,
};
pub use sync_task::{
    ConversationSettingsSyncTask, ConversationsSyncTask, KeyEventsSyncTask, MessagesSyncTask,
    ReadStatesSyncTask,
};
pub use user_profile_projection::UserProfileProjectionApplier;
