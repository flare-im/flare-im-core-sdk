//! 同步视图模型

use crate::domain::sync::model::{SyncCursor as DomainSyncCursor, SyncResult as DomainSyncResult};
use serde::{Deserialize, Serialize};

/// 同步配置视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfigVO {
    /// 消息批量大小
    pub message_batch_size: usize,
    /// 会话批量大小
    pub session_batch_size: usize,
    /// 请求超时时间（秒）
    pub request_timeout: u64,
    /// 最近消息限制
    pub recent_message_limit: usize,
}

impl Default for SyncConfigVO {
    fn default() -> Self {
        Self {
            message_batch_size: 100,
            session_batch_size: 50,
            request_timeout: 120,
            recent_message_limit: 50,
        }
    }
}

/// 同步游标视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncCursorVO {
    /// 会话 ID
    pub session_id: String,
    /// 最后同步的消息序列号
    pub last_seq: Option<i64>,
    /// 最后同步的时间戳（毫秒）
    pub last_timestamp: Option<i64>,
    /// 最后同步的消息 ID
    pub last_message_id: Option<String>,
    /// 服务器最大序列号
    pub max_seq: Option<i64>,
    /// 未读消息数量
    pub unread_count: Option<i64>,
    /// 是否已同步最近消息
    pub recent_messages_synced: bool,
    /// 最近消息同步的序列号范围
    pub recent_sync_range: Option<(i64, i64)>,
}

/// 同步结果视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResultVO {
    /// 会话 ID
    pub session_id: String,
    /// 同步的消息数量
    pub message_count: usize,
    /// 是否有更多消息
    pub has_more: bool,
    /// 同步游标
    pub cursor: Option<SyncCursorVO>,
}

/// 全量同步结果视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullSyncResultVO {
    /// 同步的会话数量
    pub session_count: usize,
    /// 总消息数量
    pub total_message_count: usize,
    /// 会话同步结果列表
    pub session_results: Vec<SyncResultVO>,
}

/// 重连同步模式
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReconnectSyncModeVO {
    /// 智能模式（根据离线时长决定）
    Smart {
        /// 全量同步阈值（秒）
        full_sync_threshold: u64,
    },
    /// 增量同步
    Incremental,
    /// 全量同步
    Full,
    /// 不同步
    None,
}

/// 重连同步策略视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconnectSyncStrategyVO {
    /// 是否自动同步
    pub auto_sync: bool,
    /// 同步延迟（秒）
    pub sync_delay: u64,
    /// 同步超时（秒）
    pub sync_timeout: u64,
    /// 同步模式
    pub sync_mode: ReconnectSyncModeVO,
    /// 最大重试次数
    pub max_retries: u32,
}

impl Default for ReconnectSyncStrategyVO {
    fn default() -> Self {
        Self {
            auto_sync: true,
            sync_delay: 1,
            sync_timeout: 120,
            sync_mode: ReconnectSyncModeVO::Smart {
                full_sync_threshold: 5 * 60, // 5分钟
            },
            max_retries: 3,
        }
    }
}

impl From<DomainSyncCursor> for SyncCursorVO {
    fn from(cursor: DomainSyncCursor) -> Self {
        Self {
            session_id: cursor.session_id,
            last_seq: cursor.last_seq,
            last_timestamp: cursor.last_timestamp,
            last_message_id: cursor.last_message_id,
            max_seq: cursor.max_seq,
            unread_count: cursor.unread_count,
            recent_messages_synced: cursor.recent_messages_synced,
            recent_sync_range: cursor.recent_sync_range,
        }
    }
}

impl From<DomainSyncResult> for SyncResultVO {
    fn from(result: DomainSyncResult) -> Self {
        Self {
            session_id: result.session_id,
            message_count: result.message_count,
            has_more: result.has_more,
            cursor: result.cursor.map(SyncCursorVO::from),
        }
    }
}

impl SyncCursorVO {
    /// 从领域模型创建
    pub fn from_domain(cursor: &DomainSyncCursor) -> Self {
        Self::from(cursor.clone())
    }
}

impl SyncResultVO {
    /// 从领域模型创建
    pub fn from_domain(result: &DomainSyncResult) -> Self {
        Self::from(result.clone())
    }
}

impl FullSyncResultVO {
    /// 创建空的全量同步结果
    pub fn empty() -> Self {
        Self {
            session_count: 0,
            total_message_count: 0,
            session_results: Vec::new(),
        }
    }
}
