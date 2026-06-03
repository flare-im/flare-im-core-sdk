//! 核心引擎层
//!
//! 架构：`API → Command/Query → EventBus → (Connection | Sync | Message) → FSM → Repository → Storage → Network`
//!
//! - **状态集中**：状态仅在 FSM 与 Actor 中（Connection FSM、Sync FSM、ReliableQueue 消息状态）
//! - **通信事件化**：模块间只通过 EventBus 通信，Dispatcher 仅做 Packet → Event 路由
//! - **单一职责**：SyncManager 只负责同步；消息/会话 API 直接委托 application usecases，`SyncProtocolAdapter` 仅作协议适配

mod dispatcher;
mod engine;
pub mod event;
mod fsm;
mod reliable_queue;
mod sync;

pub use dispatcher::Dispatcher;
pub(crate) use engine::SdkEngineConfig;
pub use engine::{SdkEngine, SdkState};
pub use fsm::{
    ConnectionEvent, ConnectionFsm, ConnectionState, MessageState, MessageStateEvent,
    MessageStateFsm, SyncFsm, SyncState, SyncTransition,
};
pub(crate) use reliable_queue::{ReliableSendQueue, ReliableSendQueueConfig};
pub use sync::{
    ConversationSummarySync, SessionSyncRunner, SyncContext, SyncFailurePolicy, SyncManager,
    SyncMode, SyncPhase, SyncProgress, SyncReason, SyncResponseHandler, SyncResult, SyncRunContext,
    SyncScope, SyncTask, SyncTaskResult, SyncTrigger, SyncVisibility,
};

use std::sync::Arc;
use tokio::sync::RwLock;

/// 当前用户 ID 存储（连接后由引擎写入）
pub type CurrentUserIdStore = Arc<RwLock<String>>;
