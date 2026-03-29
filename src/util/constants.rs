//! 全局常量 — 超时、重试等，保证「可靠队列 + 状态机」行为一致。

/// 单次请求/发送超时（秒）
pub const REQUEST_TIMEOUT_SECS: u64 = 15;
/// 等待 SendAck 超时（秒，可靠队列入队后）
pub const WAIT_ACK_TIMEOUT_SECS: u64 = 30;
/// 可靠队列单条消息超时（秒）
pub const RELIABLE_QUEUE_TIMEOUT_SECS: u64 = 15;
/// 可靠队列最大重试次数
pub const RELIABLE_QUEUE_MAX_RETRIES: u32 = 3;
