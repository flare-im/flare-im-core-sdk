//! 同步查询处理器

use crate::application::queries::sync::*;
use crate::domain::sync::repository::SyncRepository;
use anyhow::Result;
use std::sync::Arc;

/// 同步查询处理器
///
/// 处理同步相关的查询（获取同步状态等）
pub struct SyncQueryHandler {
    repository: Arc<dyn SyncRepository>,
}

impl SyncQueryHandler {
    pub fn new(repository: Arc<dyn SyncRepository>) -> Self {
        Self { repository }
    }

    /// 处理获取同步状态查询
    ///
    /// 按照微信/Telegram/飞书标准：返回当前同步状态
    pub async fn handle_get_sync_status(
        &self,
        query: GetSyncStatusQuery,
    ) -> Result<crate::domain::sync::SyncStatus> {
        use crate::domain::sync::model::SyncStatus;

        // 如果指定了会话 ID，查询该会话的同步状态
        if let Some(ref session_id) = query.session_id {
            if let Ok(Some(sync)) = self.repository.find_by_session(session_id).await {
                return Ok(sync.status());
            }
        }

        // 如果没有找到同步记录，返回 Completed（表示已完成同步）
        Ok(SyncStatus::Completed)
    }
}
