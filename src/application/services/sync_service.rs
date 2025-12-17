//! 同步应用服务
//!
//! 编排同步相关的业务逻辑

use crate::application::handlers::{SyncCommandHandler, SyncQueryHandler};
use anyhow::Result;
use std::sync::Arc;

/// 同步应用服务
///
/// 编排同步相关的业务逻辑
pub struct SyncService {
    command_handler: Arc<SyncCommandHandler>,
    query_handler: Arc<SyncQueryHandler>,
}

impl SyncService {
    pub fn new(
        command_handler: Arc<SyncCommandHandler>,
        query_handler: Arc<SyncQueryHandler>,
    ) -> Self {
        Self {
            command_handler,
            query_handler,
        }
    }

    /// 同步消息
    pub async fn sync_messages(
        &self,
        session_id: Option<crate::domain::SessionId>,
        sync_type: crate::domain::SyncType,
        after_seq: Option<i64>,
    ) -> Result<crate::domain::sync::SyncResult> {
        use crate::application::commands::sync::SyncMessagesCommand;
        self.command_handler
            .handle_sync_messages(SyncMessagesCommand {
                session_id,
                sync_type,
                after_seq,
            })
            .await
    }

    /// 同步会话
    pub async fn sync_sessions(
        &self,
        cursor: Option<String>,
    ) -> Result<crate::application::vo::session::SessionSyncResultVO> {
        use crate::application::commands::sync::SyncSessionsCommand;
        self.command_handler
            .handle_sync_sessions(SyncSessionsCommand { cursor })
            .await
    }
}
