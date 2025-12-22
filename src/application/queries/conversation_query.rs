//! 会话查询定义
//!
//! 定义所有会话相关的读操作查询

/// 查询会话列表
#[derive(Debug, Clone)]
pub struct ListConversationsQuery {
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

/// 查询会话详情
#[derive(Debug, Clone)]
pub struct GetConversationQuery {
    pub conversation_id: String,
}

/// 查询会话未读数
#[derive(Debug, Clone)]
pub struct GetConversationUnreadCountQuery {
    pub conversation_id: String,
}

/// 查询所有会话的未读总数
#[derive(Debug, Clone)]
pub struct GetTotalUnreadCountQuery;

/// 分页获取会话列表
#[derive(Debug, Clone)]
pub struct GetConversationListSplitQuery {
    pub page: usize,
    pub page_size: usize,
}

/// 根据会话 ID 获取多个会话
#[derive(Debug, Clone)]
pub struct GetMultipleConversationQuery {
    pub conversation_ids: Vec<String>,
}

/// 根据会话类型获取会话 ID
#[derive(Debug, Clone)]
pub struct GetConversationIdBySessionTypeQuery {
    pub conversation_type: String,
    pub user_id: Option<String>,
}

/// 获取输入状态
#[derive(Debug, Clone)]
pub struct GetInputStatesQuery {
    pub conversation_id: String,
}
