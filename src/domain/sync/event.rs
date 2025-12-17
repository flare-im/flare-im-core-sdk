//! 同步领域事件
//!
//! 表示同步相关的业务事件

use crate::domain::message::model::SessionId;
use crate::domain::sync::model::{SyncCursor, SyncType};
use chrono::DateTime;
use chrono::Utc;

/// 同步已开始事件
#[derive(Debug, Clone)]
pub struct SyncStartedEvent {
    pub session_id: Option<SessionId>,
    pub sync_type: SyncType,
    pub timestamp: DateTime<Utc>,
}

/// 同步已完成事件
#[derive(Debug, Clone)]
pub struct SyncCompletedEvent {
    pub session_id: Option<SessionId>,
    pub sync_type: SyncType,
    pub cursor: Option<SyncCursor>,
    pub timestamp: DateTime<Utc>,
}

/// 同步失败事件
#[derive(Debug, Clone)]
pub struct SyncFailedEvent {
    pub session_id: Option<SessionId>,
    pub sync_type: SyncType,
    pub error: String,
    pub timestamp: DateTime<Utc>,
}
