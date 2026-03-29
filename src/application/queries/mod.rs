//! 读侧（Query）— 仅定义参数结构，与 commands 对齐；执行由 handlers 承担。

/// 会话列表查询
#[derive(Clone, Copy)]
pub struct GetConversationsQuery;

/// 单条会话查询
#[derive(Clone)]
pub struct GetConversationQuery {
    pub conversation_id: String,
}

/// 会话消息列表查询
#[derive(Clone)]
pub struct GetMessagesQuery {
    pub conversation_id: String,
    pub before_seq: u64,
    pub limit: u32,
}

/// 消息搜索查询
#[derive(Clone)]
pub struct SearchMessagesQuery {
    pub keyword: String,
    pub limit: u32,
}
