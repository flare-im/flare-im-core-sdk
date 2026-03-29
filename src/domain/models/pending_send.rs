use crate::model::IMMessage;

#[derive(Clone)]
pub struct PendingSendVo {
    pub client_msg_id: String,
    pub conversation_id: String,
    pub message: IMMessage,
    /// 入队时间戳（毫秒），用于排序与重试
    pub enqueued_at_ms: u64,
}
