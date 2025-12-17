//! 同步相关查询

use crate::domain::SessionId;

/// 获取同步状态查询
#[derive(Debug, Clone)]
pub struct GetSyncStatusQuery {
    pub session_id: Option<SessionId>,
}
