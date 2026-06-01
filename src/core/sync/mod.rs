//! 同步引擎：任务抽象 + 编排（Init/Background）；飞书级 IM 基线 — 连接就绪后自动执行，状态与阶段仅通过 [crate::event] 回调获取。
//!
//! **流程**：SyncStarted → Init 任务并行 → SyncFinished(Init) → Background 任务并行 → SyncFinished(Background)。
//! 应用层可监听 [SyncStateChanged]、[SyncFinished]（Background 结束后可认为全量同步完成，如打印会话数）。
//!
//! - [SyncTask] / [SyncContext] / [SyncTaskResult]：可注册任务与执行上下文
//! - [SyncMode]：Init（阻塞就绪）/ Background
//! - [SyncManager]：任务注册与 [Orchestrator] 编排执行（Init → Background）
//!
//! 自定义任务：实现 [SyncTask]（id、mode、weight、execute），通过 [SyncManager::register_task_arc] 或 [crate::client::IMClientBuilder::add_sync_task] 注册。会话/消息/已读等业务任务见 [crate::application::sync_task]。

mod checkpoint;
mod conversation_summary_sync;
mod error;
mod manager;
mod orchestrator;
mod progress;
mod response_handler;
mod task;

#[allow(unused_imports)]
pub use checkpoint::{CheckpointStore, SyncCheckpoint};
pub use conversation_summary_sync::ConversationSummarySync;
#[allow(unused_imports)]
pub use error::{SyncError, SyncResult};
pub use manager::SyncManager;
#[allow(unused_imports)]
pub use progress::{EventBusProgressReporter, SyncProgress, SyncProgressReporter};
pub use response_handler::SyncResponseHandler;
pub use task::{
    SessionSyncRunner, SyncContext, SyncFailurePolicy, SyncMode, SyncPhase, SyncReason,
    SyncRunContext, SyncScope, SyncTask, SyncTaskResult, SyncTrigger, SyncVisibility,
};
