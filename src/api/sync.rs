//! 数据同步 API 实现

use crate::api::FlareIMClient;
use crate::api::traits::SyncApi;
use crate::application::commands::SyncMessagesCommand;
use crate::domain::{SessionId, SyncType};
use anyhow::{Context, Result};

impl SyncApi for FlareIMClient {
    async fn sync_messages(
        &self,
        session_id: &str,
        after_seq: Option<i64>,
    ) -> Result<crate::domain::sync::SyncResult> {
        self.sync_command_handler
            .handle_sync_messages(SyncMessagesCommand {
                session_id: Some(SessionId::new(session_id.to_string())),
                sync_type: SyncType::Incremental,
                after_seq,
            })
            .await
            .context("Failed to sync messages")
    }

    async fn sync_sessions(
        &self,
        cursor: Option<String>,
    ) -> Result<crate::application::vo::session::SessionSyncResultVO> {
        anyhow::bail!("sync_sessions: Not implemented yet")
    }
}
