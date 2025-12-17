//! 任务配置

use serde::{Deserialize, Serialize};

/// 同步配置
///
/// 用于任务执行器的同步配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    /// 消息批量大小
    pub message_batch_size: usize,
    /// 会话批量大小
    pub session_batch_size: usize,
    /// 请求超时时间（秒）
    pub request_timeout: u64,
    /// 最近消息限制
    pub recent_message_limit: usize,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            message_batch_size: 100,
            session_batch_size: 50,
            request_timeout: 120,
            recent_message_limit: 50,
        }
    }
}

impl From<crate::application::vo::sync::SyncConfigVO> for SyncConfig {
    fn from(vo: crate::application::vo::sync::SyncConfigVO) -> Self {
        Self {
            message_batch_size: vo.message_batch_size,
            session_batch_size: vo.session_batch_size,
            request_timeout: vo.request_timeout,
            recent_message_limit: vo.recent_message_limit,
        }
    }
}

impl From<SyncConfig> for crate::application::vo::sync::SyncConfigVO {
    fn from(config: SyncConfig) -> Self {
        Self {
            message_batch_size: config.message_batch_size,
            session_batch_size: config.session_batch_size,
            request_timeout: config.request_timeout,
            recent_message_limit: config.recent_message_limit,
        }
    }
}
