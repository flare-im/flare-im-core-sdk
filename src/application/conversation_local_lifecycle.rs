//! 本地会话生命周期边界。
//!
//! 该能力只处理 IM core 通用语义：本地历史清理水位、单会话同步游标、会话可见性。
//! 好友删除、重新加好友、业务重置等上层动作只负责选择目标会话 ID。

use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::{ConversationStore, SyncCursorStore, SyncCursorVo, local_cleared_through_seq};
use crate::error::Result;
use crate::store::StoreProvider;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalConversationVisibility {
    Keep,
    Visible,
    Archived,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalConversationClearResult {
    pub conversation_id: String,
    pub cleared_through_seq: u64,
}

pub struct ConversationLocalLifecycle;

impl ConversationLocalLifecycle {
    pub async fn clear_history_boundary(
        stores: &StoreProvider,
        current_user_id: &str,
        conversation_id: &str,
        visibility: LocalConversationVisibility,
    ) -> Result<Option<LocalConversationClearResult>> {
        Self::clear_history_boundary_with_ports(
            stores.conversations.as_ref(),
            Some(stores.cursors.as_ref()),
            current_user_id,
            conversation_id,
            visibility,
        )
        .await
    }

    pub async fn clear_history_boundary_with_ports(
        conversations: &dyn ConversationStore,
        cursors: Option<&dyn SyncCursorStore>,
        current_user_id: &str,
        conversation_id: &str,
        visibility: LocalConversationVisibility,
    ) -> Result<Option<LocalConversationClearResult>> {
        let Some(conversation) = conversations.get(conversation_id).await? else {
            return Ok(None);
        };

        let mut watermark = conversation.max_seq;
        watermark = watermark.max(conversations.get_local_max_seq(conversation_id).await?);
        watermark = watermark.max(local_cleared_through_seq(&conversation.ext));
        watermark = watermark.max(conversation.visible_after_seq);

        let user_id = current_user_id.trim();
        if !user_id.is_empty()
            && let Some(cursor_store) = cursors
            && let Some(cursor) = cursor_store
                .get_conversation_cursor(user_id, conversation_id)
                .await?
        {
            watermark = watermark.max(cursor.last_seq);
        }

        conversations
            .clear_local_chat_history(conversation_id, watermark)
            .await?;

        match visibility {
            LocalConversationVisibility::Keep => {}
            LocalConversationVisibility::Visible => {
                conversations.set_archived(conversation_id, false).await?;
            }
            LocalConversationVisibility::Archived => {
                conversations.set_archived(conversation_id, true).await?;
            }
        }

        if !user_id.is_empty()
            && let Some(cursor_store) = cursors
        {
            cursor_store
                .save_conversation_cursor(&SyncCursorVo {
                    user_id: user_id.to_string(),
                    conversation_id: conversation_id.to_string(),
                    last_seq: watermark,
                    synced_at: now_ms(),
                })
                .await?;
        }

        Ok(Some(LocalConversationClearResult {
            conversation_id: conversation_id.to_string(),
            cleared_through_seq: watermark,
        }))
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
