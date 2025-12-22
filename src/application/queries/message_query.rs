//! 消息查询定义
//!
//! 定义所有消息相关的读操作查询

/// 查询消息列表
#[derive(Debug, Clone)]
pub struct ListMessagesQuery {
    pub conversation_id: String,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

/// 查询消息详情
#[derive(Debug, Clone)]
pub struct GetMessageQuery {
    pub message_id: String,
}

/// 搜索消息
#[derive(Debug, Clone)]
pub struct SearchMessagesQuery {
    pub conversation_id: Option<String>,
    pub keyword: String,
    pub limit: Option<usize>,
}
