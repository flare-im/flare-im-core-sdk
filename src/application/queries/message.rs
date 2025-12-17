//! 消息相关查询

use crate::domain::{MessageId, SessionId};

/// 获取消息列表查询
#[derive(Debug, Clone)]
pub struct GetMessagesQuery {
    pub session_id: SessionId,
    pub limit: usize,
    pub before_message_id: Option<MessageId>,
}

/// 获取消息查询
#[derive(Debug, Clone)]
pub struct GetMessageQuery {
    pub message_id: MessageId,
}

/// 批量获取消息查询
#[derive(Debug, Clone)]
pub struct GetMessagesBatchQuery {
    pub message_ids: Vec<MessageId>,
}

/// 搜索消息查询
#[derive(Debug, Clone)]
pub struct SearchMessagesQuery {
    pub keyword: String,
    pub session_id: Option<String>,
    pub limit: Option<usize>,
}

/// 获取历史消息查询
#[derive(Debug, Clone)]
pub struct GetHistoryQuery {
    pub session_id: SessionId,
    pub limit: usize,
    pub before_message_id: Option<MessageId>,
    pub before_seq: Option<i64>,
    pub after_seq: Option<i64>,
}
