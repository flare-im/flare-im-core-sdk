//! 应用层 — 与 orchestrator application 对齐
//!
//! commands（写侧）、queries（读侧）、handlers（消息/会话业务处理）、sync_task（会话/消息/已读同步任务）。

pub mod commands;
pub mod handlers;
pub mod queries;
pub mod sync_task;

pub use commands::{EditMessageCommand, RecallMessageCommand, SendMessageCommand};
pub use handlers::{
    ConversationFlow, ConversationQueryHandler, MessageBuilderHandler, MessageEngine,
    MessageQueryHandler,
};
pub use queries::{
    GetConversationQuery, GetConversationsQuery, GetMessagesQuery, SearchMessagesQuery,
};
pub use sync_task::{ConversationsSyncTask, MessagesSyncTask, ReadStatesSyncTask};
