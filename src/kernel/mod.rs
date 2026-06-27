//! Kernel-level contracts shared by runtime, application, extensions, and SDK facades.
//!
//! The kernel owns business-neutral event contracts, finite state machines, and
//! sync abstractions. It must not depend on application services or runtime
//! orchestration.

pub mod event;
pub mod fsm;
mod reliable_queue;
pub mod sync;

use std::sync::Arc;
use tokio::sync::RwLock;

pub use fsm::{
    ConnectionEvent, ConnectionFsm, ConnectionState, MessageState, MessageStateEvent,
    MessageStateFsm, SyncFsm, SyncState, SyncTransition,
};
pub use reliable_queue::ReliableSendQueuePort;
pub use sync::{
    ConversationSummarySync, SessionSyncRunner, SyncContext, SyncFailurePolicy, SyncManager,
    SyncMode, SyncPhase, SyncProgress, SyncReason, SyncResponseHandler, SyncResult, SyncRunContext,
    SyncScope, SyncTask, SyncTaskResult, SyncTrigger, SyncVisibility,
};

/// 对外暴露的连接状态（与 kernel FSM `ConnectionState` 对齐，便于 UI 展示）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdkState {
    Disconnected,
    Connecting,
    Connected,
    Ready,
    Reconnecting,
}

impl SdkState {
    pub(crate) fn as_u8(self) -> u8 {
        match self {
            SdkState::Disconnected => 0,
            SdkState::Connecting => 1,
            SdkState::Connected => 2,
            SdkState::Ready => 3,
            SdkState::Reconnecting => 4,
        }
    }

    pub(crate) fn from_u8(value: u8) -> Self {
        match value {
            1 => SdkState::Connecting,
            2 => SdkState::Connected,
            3 => SdkState::Ready,
            4 => SdkState::Reconnecting,
            _ => SdkState::Disconnected,
        }
    }
}

impl From<ConnectionState> for SdkState {
    fn from(s: ConnectionState) -> Self {
        use ConnectionState as S;
        match s {
            S::Disconnected => SdkState::Disconnected,
            S::Connecting => SdkState::Connecting,
            S::Connected => SdkState::Connected,
            S::Ready => SdkState::Ready,
            S::Reconnecting => SdkState::Reconnecting,
        }
    }
}

/// 当前用户 ID 存储（连接后由 runtime 写入）
pub type CurrentUserIdStore = Arc<RwLock<String>>;
