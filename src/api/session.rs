//! 会话管理 API 实现

use crate::api::FlareIMClient;
use crate::api::traits::SessionApi;
use crate::application::vo::SessionVO;
use crate::domain::session::SessionSummary as DomainSessionSummary;
use crate::infrastructure::storage::SessionFilter;
use anyhow::{Context, Result};
use std::sync::Arc;

impl SessionApi for FlareIMClient {
    async fn get_sessions(&self, filter: SessionFilter) -> Result<Vec<SessionVO>> {
        let local_sessions = self
            .storage
            .get_sessions(filter.clone())
            .await
            .context("Failed to get local sessions")?;

        let should_sync = local_sessions.is_empty() && self.connection.is_connected().await;
        if should_sync {
            #[cfg(not(target_arch = "wasm32"))]
            use tokio::spawn as tokio_spawn;
            #[cfg(target_arch = "wasm32")]
            use tokio::task::spawn_local as tokio_spawn;

            let sync_handler = Arc::clone(&self.sync_command_handler);
            tokio_spawn(async move {
                use crate::application::commands::SyncMessagesCommand;
                use crate::domain::SyncType;
                if let Err(e) = sync_handler
                    .handle_sync_messages(SyncMessagesCommand {
                        session_id: None,
                        sync_type: SyncType::Incremental,
                        after_seq: None,
                    })
                    .await
                {
                    tracing::debug!(error = %e, "Auto-sync sessions failed (non-blocking)");
                }
            });
        }

        // 将 DomainSessionSummary 转换为 SessionVO
        Ok(local_sessions.into_iter().map(SessionVO::from).collect())
    }

    async fn get_sessions_paginated(
        &self,
        limit: usize,
        cursor: Option<String>,
        filter: Option<SessionFilter>,
    ) -> Result<(Vec<SessionVO>, Option<String>)> {
        let filter = filter.unwrap_or_default();
        let all_sessions = self.get_sessions(filter).await?;

        let start_index = if let Some(cursor_str) = cursor {
            if let Some(colon_pos) = cursor_str.find(':') {
                all_sessions
                    .iter()
                    .position(|s| s.session_id == cursor_str[colon_pos + 1..])
                    .unwrap_or(0)
            } else {
                0
            }
        } else {
            0
        };

        let end_index = (start_index + limit).min(all_sessions.len());
        let page_sessions = all_sessions[start_index..end_index].to_vec();

        let next_cursor = if end_index < all_sessions.len() {
            if let Some(last_session) = page_sessions.last() {
                let timestamp = last_session.updated_at.unwrap_or(0);
                Some(format!(
                    "timestamp:{}:{}",
                    timestamp, last_session.session_id
                ))
            } else {
                None
            }
        } else {
            None
        };

        Ok((page_sessions, next_cursor))
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<SessionVO>> {
        let filter = SessionFilter::default();
        let sessions = self.get_sessions(filter).await?;
        Ok(sessions.into_iter().find(|s| s.session_id == session_id))
    }

    async fn get_sessions_batch(&self, session_ids: Vec<String>) -> Result<Vec<SessionVO>> {
        let filter = SessionFilter::default();
        let all_sessions = self.get_sessions(filter).await?;
        let mut result = Vec::new();

        for session_id in session_ids {
            if let Some(session) = all_sessions.iter().find(|s| s.session_id == session_id) {
                result.push(session.clone());
            }
        }

        Ok(result)
    }

    async fn find_session_id(
        &self,
        session_type: &str,
        business_type: &str,
        target_id: &str,
    ) -> Result<Option<String>> {
        let expected_session_id = format!("{}:{}:{}", session_type, business_type, target_id);
        let filter = SessionFilter::default();
        let sessions = self.get_sessions(filter).await?;

        if sessions.iter().any(|s| s.session_id == expected_session_id) {
            Ok(Some(expected_session_id))
        } else {
            Ok(None)
        }
    }

    #[cfg(feature = "extensions")]
    async fn get_sessions_extended(
        &self,
        filter: SessionFilter,
    ) -> Result<Vec<crate::domain::session::ExtendedSessionSummary>> {
        anyhow::bail!("get_sessions_extended: Not implemented yet")
    }

    #[cfg(feature = "extensions")]
    async fn get_session_extended(
        &self,
        session_id: &str,
    ) -> Result<crate::domain::session::ExtendedSessionSummary> {
        anyhow::bail!("get_session_extended: Not implemented yet")
    }

    async fn create_session(
        &self,
        session_id: Option<String>,
        session_type: String,
        business_type: String,
        display_name: Option<String>,
        participants: Option<Vec<String>>,
    ) -> Result<String> {
        use crate::application::commands::CreateSessionCommand;
        use crate::domain::message::model::SessionId as DomainSessionId;

        let cmd = CreateSessionCommand {
            session_id: session_id.map(DomainSessionId::new),
            session_type,
            business_type,
            display_name,
            participants: participants.unwrap_or_default(),
        };

        let created_session_id = self
            .session_command_handler
            .handle_create_session(cmd)
            .await
            .context("Failed to create session")?;

        Ok(created_session_id.to_string())
    }

    async fn update_session(
        &self,
        session_id: &str,
        updates: std::collections::HashMap<String, String>,
    ) -> Result<()> {
        use crate::infrastructure::storage::SessionUpdate;

        let mut session_update = SessionUpdate::new();

        if let Some(display_name) = updates.get("display_name") {
            session_update.display_name = Some(display_name.clone());
        }

        let metadata: std::collections::HashMap<String, String> = updates
            .into_iter()
            .filter(|(k, _)| k != "display_name")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        if !metadata.is_empty() {
            session_update.metadata = Some(metadata);
        }

        self.storage
            .update_session(session_id, session_update)
            .await
            .context("Failed to update session")
    }

    async fn delete_session(&self, session_id: &str, delete_messages: bool) -> Result<usize> {
        let deleted_count = if delete_messages {
            self.storage
                .delete_all_messages(session_id)
                .await
                .context("Failed to delete messages")?
        } else {
            0
        };

        self.storage
            .delete_session(session_id)
            .await
            .context("Failed to delete session")?;

        Ok(deleted_count)
    }

    async fn hide_session(&self, session_id: &str) -> Result<()> {
        let mut updates = std::collections::HashMap::new();
        updates.insert("is_hidden".to_string(), "true".to_string());
        self.update_session(session_id, updates).await
    }

    async fn show_session(&self, session_id: &str) -> Result<()> {
        let mut updates = std::collections::HashMap::new();
        updates.insert("is_hidden".to_string(), "false".to_string());
        self.update_session(session_id, updates).await
    }

    async fn get_total_unread_count(&self) -> Result<u32> {
        let filter = SessionFilter::default();
        let sessions = self.get_sessions(filter).await?;
        let total_unread: u32 = sessions.iter().map(|s| s.unread_count).sum();
        Ok(total_unread)
    }

    async fn mark_read(&self, session_id: &str, message_seq: Option<i64>) -> Result<()> {
        use crate::infrastructure::storage::SessionUpdate;

        let mut update = SessionUpdate::new();
        if let Some(seq) = message_seq {
            // SessionUpdate 没有 last_read_seq 字段，需要通过其他方式更新
            // 这里暂时通过更新未读数来实现（实际应该更新 last_read_seq）
            update.unread_count = Some(0);
        } else {
            // SessionUpdate 没有 last_read_seq 字段，通过更新未读数来实现
            // TODO: 需要添加 last_read_seq 字段到 SessionUpdate 或使用其他方式更新
            if let Ok(Some(_max_seq)) = self.storage.get_max_seq(session_id).await {
                // 标记为已读（通过设置未读数为0）
                update.unread_count = Some(0);
            }
        }
        update.unread_count = Some(0);

        self.storage
            .update_session(session_id, update)
            .await
            .context("Failed to mark session as read")
    }

    async fn mark_read_batch(&self, session_ids: Vec<String>) -> Result<usize> {
        let mut success_count = 0;
        for session_id in session_ids {
            if self.mark_read(&session_id, None).await.is_ok() {
                success_count += 1;
            }
        }
        Ok(success_count)
    }

    async fn set_draft(&self, session_id: &str, draft: Option<String>) -> Result<()> {
        let mut updates = std::collections::HashMap::new();
        if let Some(draft_text) = draft {
            updates.insert("draft".to_string(), draft_text);
        } else {
            updates.insert("draft".to_string(), String::new());
        }
        self.update_session(session_id, updates).await
    }

    async fn get_draft(&self, session_id: &str) -> Result<Option<String>> {
        // TODO: 从存储中获取草稿（需要 StorageBackend 支持草稿存储）
        // 暂时从会话的 metadata 中获取
        let session = self.get_session(session_id).await?;
        if let Some(s) = session {
            if let Some(draft) = s.metadata.get("draft") {
                if !draft.is_empty() {
                    return Ok(Some(draft.clone()));
                }
            }
        }
        Ok(None)
    }

    async fn send_typing(&self, session_id: &str, is_typing: bool) -> Result<()> {
        use crate::application::commands::session::SetDraftCommand;
        use crate::domain::message::model::{SessionId, UserId};

        // 获取当前用户 ID
        let user_id = self.user_id.read().await.clone();
        let user_id = UserId::new(user_id);
        let session_id = SessionId::new(session_id.to_string());

        // 调用命令处理器
        self.session_command_handler
            .handle_send_typing(crate::application::commands::session::SendTypingCommand {
                session_id,
                user_id,
                is_typing,
            })
            .await
            .context("Failed to send typing")
    }

    async fn get_typing_status(&self, _session_id: &str) -> Result<bool> {
        Ok(false)
    }
}
