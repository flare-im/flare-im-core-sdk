//! 同步领域模型
//!
//! 包含 Sync 聚合根、值对象等

use crate::domain::message::model::SessionId;
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::event::{SyncCompletedEvent, SyncFailedEvent, SyncStartedEvent};

/// 同步游标（用于增量同步）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncCursor {
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

impl SyncCursor {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            last_seq: None,
            last_timestamp: None,
            last_message_id: None,
            max_seq: None,
            unread_count: None,
            recent_messages_synced: false,
            recent_sync_range: None,
        }
    }

    /// 更新游标信息
    pub fn update(
        &mut self,
        last_seq: Option<i64>,
        max_seq: Option<i64>,
        unread_count: Option<i64>,
    ) {
        if let Some(seq) = last_seq {
            self.last_seq = Some(seq);
            self.last_timestamp = Some(chrono::Utc::now().timestamp_millis());
        }
        if let Some(max) = max_seq {
            self.max_seq = Some(max);
        }
        if let Some(unread) = unread_count {
            self.unread_count = Some(unread);
        }
    }

    /// 更新最近消息同步范围
    pub fn update_recent_sync_range(&mut self, start_seq: i64, end_seq: i64) {
        self.recent_sync_range = Some((start_seq, end_seq));
        self.recent_messages_synced = true;
    }

    /// 更新服务器游标信息
    pub fn update_server_cursor(&mut self, max_seq: i64, unread_count: Option<i64>) {
        self.max_seq = Some(max_seq);
        if let Some(unread) = unread_count {
            self.unread_count = Some(unread);
        }
    }
}

/// 同步结果
#[derive(Debug, Clone)]
pub struct SyncResult {
    /// 会话 ID
    pub session_id: String,
    /// 同步的消息数量
    pub message_count: usize,
    /// 是否有更多消息
    pub has_more: bool,
    /// 同步游标
    pub cursor: Option<SyncCursor>,
}

impl SyncResult {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            message_count: 0,
            has_more: false,
            cursor: None,
        }
    }
}

/// 同步错误
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("Sync validation failed: {0}")]
    ValidationFailed(String),

    #[error("Sync not found")]
    NotFound,

    #[error("Sync already in progress")]
    AlreadyInProgress,
}

/// Sync 聚合根
///
/// 封装同步的领域逻辑和行为
pub struct Sync {
    session_id: Option<SessionId>,
    sync_type: SyncType,
    status: SyncStatus,
    cursor: Option<SyncCursor>,
    started_at: chrono::DateTime<Utc>,
    completed_at: Option<chrono::DateTime<Utc>>,
}

/// 同步类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncType {
    Full,
    Incremental,
    Session,
}

/// 同步状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl Sync {
    /// 创建新同步
    pub fn new(session_id: Option<SessionId>, sync_type: SyncType) -> Self {
        Self {
            session_id,
            sync_type,
            status: SyncStatus::Pending,
            cursor: None,
            started_at: Utc::now(),
            completed_at: None,
        }
    }

    /// 开始同步（领域行为）
    pub fn start(self) -> Result<SyncStartedEvent> {
        // 验证状态
        if self.status != SyncStatus::Pending {
            return Err(SyncError::AlreadyInProgress.into());
        }

        // 创建领域事件
        let event = SyncStartedEvent {
            session_id: self.session_id.clone(),
            sync_type: self.sync_type,
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 完成同步（领域行为）
    pub fn complete(mut self, cursor: Option<SyncCursor>) -> Result<SyncCompletedEvent> {
        self.status = SyncStatus::Completed;
        self.cursor = cursor.clone();
        self.completed_at = Some(Utc::now());

        // 创建领域事件
        let event = SyncCompletedEvent {
            session_id: self.session_id.clone(),
            sync_type: self.sync_type,
            cursor,
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 创建同步结果（用于返回给调用者）
    pub fn to_result(&self, message_count: usize, has_more: bool) -> SyncResult {
        SyncResult {
            session_id: self
                .session_id
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or_default(),
            message_count,
            has_more,
            cursor: self.cursor.clone(),
        }
    }

    // Getters
    pub fn session_id(&self) -> Option<&SessionId> {
        self.session_id.as_ref()
    }

    pub fn sync_type(&self) -> SyncType {
        self.sync_type
    }

    pub fn status(&self) -> SyncStatus {
        self.status
    }

    pub fn cursor(&self) -> Option<&SyncCursor> {
        self.cursor.as_ref()
    }

    /// 失败同步（领域行为）
    pub fn fail(self, error: String) -> Result<SyncFailedEvent> {
        // 验证状态
        if self.status != SyncStatus::InProgress {
            return Err(SyncError::ValidationFailed("Sync is not in progress".to_string()).into());
        }

        // 创建领域事件
        let event = SyncFailedEvent {
            session_id: self.session_id.clone(),
            sync_type: self.sync_type,
            error,
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 更新状态（领域行为）
    pub fn update_status(mut self, status: SyncStatus) -> Self {
        self.status = status;
        self
    }
}

impl Clone for Sync {
    fn clone(&self) -> Self {
        Self {
            session_id: self.session_id.clone(),
            sync_type: self.sync_type,
            status: self.status,
            cursor: self.cursor.clone(),
            started_at: self.started_at,
            completed_at: self.completed_at,
        }
    }
}

// SyncType 和 SyncStatus 已经实现了 Copy，不需要手动实现 Clone
