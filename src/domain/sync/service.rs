//! 同步领域服务接口和实现
//!
//! 封装复杂的业务逻辑，不依赖基础设施

use crate::domain::message::model::SessionId;
use crate::domain::sync::model::{Sync, SyncCursor, SyncType};
use anyhow::{Context, Result};
use async_trait::async_trait;

/// 同步领域服务接口
///
/// 封装同步相关的复杂业务逻辑
#[async_trait]
pub trait SyncDomainService: Send + std::marker::Sync {
    /// 创建同步
    async fn create_sync(&self, session_id: Option<SessionId>, sync_type: SyncType)
    -> Result<Sync>;

    /// 验证同步
    async fn validate_sync(&self, sync: &Sync) -> Result<()>;

    /// 创建同步游标
    async fn create_cursor(&self, session_id: Option<SessionId>, seq: i64) -> Result<SyncCursor> {
        Ok(SyncCursor {
            session_id: session_id.map(|s| s.to_string()).unwrap_or_default(),
            last_seq: Some(seq),
            last_timestamp: Some(chrono::Utc::now().timestamp_millis()),
            last_message_id: None,
            max_seq: Some(seq),
            unread_count: None,
            recent_messages_synced: false,
            recent_sync_range: None,
        })
    }

    /// 更新同步游标
    ///
    /// # 参数
    /// - `cursor`: 当前游标
    /// - `last_seq`: 最后同步的序列号
    /// - `max_seq`: 服务器最大序列号
    /// - `unread_count`: 未读消息数量
    ///
    /// # 返回
    /// - `SyncCursor`: 更新后的游标
    fn update_cursor(
        &self,
        cursor: &mut SyncCursor,
        last_seq: Option<i64>,
        max_seq: Option<i64>,
        unread_count: Option<i64>,
    ) {
        if let Some(seq) = last_seq {
            cursor.last_seq = Some(seq);
            cursor.last_timestamp = Some(chrono::Utc::now().timestamp_millis());
        }
        if let Some(max) = max_seq {
            cursor.max_seq = Some(max);
        }
        if let Some(unread) = unread_count {
            cursor.unread_count = Some(unread);
        }
    }

    /// 更新最近消息同步范围
    ///
    /// # 参数
    /// - `cursor`: 当前游标
    /// - `start_seq`: 起始序列号
    /// - `end_seq`: 结束序列号
    fn update_recent_sync_range(&self, cursor: &mut SyncCursor, start_seq: i64, end_seq: i64) {
        cursor.recent_sync_range = Some((start_seq, end_seq));
        cursor.recent_messages_synced = true;
    }

    /// 判断是否需要全量同步
    ///
    /// # 参数
    /// - `offline_duration`: 离线时长（毫秒）
    /// - `threshold`: 全量同步阈值（毫秒）
    ///
    /// # 返回
    /// - `bool`: 是否需要全量同步
    fn should_full_sync(&self, offline_duration: i64, threshold: i64) -> bool {
        offline_duration > threshold
    }

    /// 计算增量同步的起始序列号
    ///
    /// # 参数
    /// - `cursor`: 当前游标
    /// - `message_count`: 消息数量
    ///
    /// # 返回
    /// - `Option<i64>`: 起始序列号
    fn calculate_incremental_start_seq(
        &self,
        cursor: &SyncCursor,
        message_count: usize,
    ) -> Option<i64> {
        cursor.last_seq.map(|last_seq| {
            let start_seq = (last_seq as i64 - message_count as i64 + 1).max(1);
            start_seq
        })
    }
}

/// 同步领域服务实现
pub struct SyncDomainServiceImpl;

impl SyncDomainServiceImpl {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SyncDomainService for SyncDomainServiceImpl {
    async fn create_sync(
        &self,
        session_id: Option<SessionId>,
        sync_type: SyncType,
    ) -> Result<Sync> {
        // 创建同步聚合根
        let sync = Sync::new(session_id, sync_type);

        // 验证同步
        self.validate_sync(&sync).await?;

        Ok(sync)
    }

    async fn validate_sync(&self, _sync: &Sync) -> Result<()> {
        Ok(())
    }

    fn update_cursor(
        &self,
        cursor: &mut SyncCursor,
        last_seq: Option<i64>,
        max_seq: Option<i64>,
        unread_count: Option<i64>,
    ) {
        if let Some(seq) = last_seq {
            cursor.last_seq = Some(seq);
            cursor.last_timestamp = Some(chrono::Utc::now().timestamp_millis());
        }
        if let Some(max) = max_seq {
            cursor.max_seq = Some(max);
        }
        if let Some(unread) = unread_count {
            cursor.unread_count = Some(unread);
        }
    }

    fn update_recent_sync_range(&self, cursor: &mut SyncCursor, start_seq: i64, end_seq: i64) {
        cursor.recent_sync_range = Some((start_seq, end_seq));
        cursor.recent_messages_synced = true;
    }

    fn should_full_sync(&self, offline_duration: i64, threshold: i64) -> bool {
        offline_duration > threshold
    }

    fn calculate_incremental_start_seq(
        &self,
        cursor: &SyncCursor,
        message_count: usize,
    ) -> Option<i64> {
        cursor.last_seq.map(|last_seq| {
            let start_seq = (last_seq as i64 - message_count as i64 + 1).max(1);
            start_seq
        })
    }
}
