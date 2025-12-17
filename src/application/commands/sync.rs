//! 同步相关命令

use crate::domain::{SessionId, SyncType};

/// 同步消息命令
#[derive(Debug, Clone)]
pub struct SyncMessagesCommand {
    pub session_id: Option<SessionId>,
    pub sync_type: SyncType,
    pub after_seq: Option<i64>,
}

/// 同步会话命令
#[derive(Debug, Clone)]
pub struct SyncSessionsCommand {
    pub cursor: Option<String>,
}
