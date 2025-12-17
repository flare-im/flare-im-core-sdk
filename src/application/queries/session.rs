//! 会话相关查询

use crate::domain::message::model::SessionId;
use crate::infrastructure::storage::SessionFilter;

/// 获取会话列表查询
#[derive(Debug, Clone)]
pub struct GetSessionsQuery {
    pub filter: SessionFilter,
}

/// 分页获取会话列表查询
#[derive(Debug, Clone)]
pub struct GetSessionsPaginatedQuery {
    pub limit: usize,
    pub cursor: Option<String>,
    pub filter: Option<SessionFilter>,
}

/// 获取会话查询
#[derive(Debug, Clone)]
pub struct GetSessionQuery {
    pub session_id: SessionId,
}

/// 批量获取会话查询
#[derive(Debug, Clone)]
pub struct GetSessionsBatchQuery {
    pub session_ids: Vec<SessionId>,
}

/// 查找会话 ID 查询
#[derive(Debug, Clone)]
pub struct FindSessionIdQuery {
    pub session_type: String,
    pub business_type: String,
    pub target_id: String,
}

/// 获取总未读数查询
#[derive(Debug, Clone)]
pub struct GetTotalUnreadCountQuery;

/// 获取草稿查询
#[derive(Debug, Clone)]
pub struct GetDraftQuery {
    pub session_id: SessionId,
}

/// 获取输入状态查询
#[derive(Debug, Clone)]
pub struct GetTypingStatusQuery {
    pub session_id: SessionId,
}
