//! 会话（Conversation）事件转发
//!
//! 将 SDK 会话事件（创建、更新、已读等）自动转发到 Tauri 前端

use tauri::{AppHandle, Emitter};
use flare_im_core_sdk::{
    interface::event::ConversationEventSubscriber,
    domain::event::*,
};
use anyhow::Result as AnyhowResult;

/// 会话事件订阅器（转发到前端）
pub struct ConversationEventForwarder {
    app: AppHandle,
}

impl ConversationEventForwarder {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

#[async_trait::async_trait]
impl ConversationEventSubscriber for ConversationEventForwarder {
    async fn on_conversation_created(&self, event: &ConversationCreated) -> AnyhowResult<()> {
        let _ = self.app.emit("im://conversation_created", serde_json::json!({
            "conversation_id": event.conversation_id,
            "conversation_type": event.conversation_type,
        }));
        Ok(())
    }

    async fn on_unread_updated(&self, event: &ConversationUnreadUpdated) -> AnyhowResult<()> {
        let _ = self.app.emit("im://unread", serde_json::json!({
            "conversation_id": event.conversation_id,
            "count": event.unread_count,
        }));
        Ok(())
    }

    async fn on_last_message_updated(&self, event: &ConversationLastMessageUpdated) -> AnyhowResult<()> {
        let _ = self.app.emit("im://conversation_last_message_updated", serde_json::json!({
            "conversation_id": event.conversation_id,
            "message_id": event.message_id,
            "seq": event.seq,
        }));
        Ok(())
    }

    async fn on_marked_as_read(&self, event: &ConversationMarkedAsRead) -> AnyhowResult<()> {
        let _ = self.app.emit("im://conversation_marked_as_read", serde_json::json!({
            "conversation_id": event.conversation_id,
            "user_id": event.user_id,
            "unread_count": event.unread_count,
        }));
        Ok(())
    }

    async fn on_draft_updated(&self, event: &ConversationDraftUpdated) -> AnyhowResult<()> {
        let _ = self.app.emit("im://conversation_draft_updated", serde_json::json!({
            "conversation_id": event.conversation_id,
            "draft": event.draft,
        }));
        Ok(())
    }

    async fn on_hidden(&self, event: &ConversationHidden) -> AnyhowResult<()> {
        let _ = self.app.emit("im://conversation_hidden", serde_json::json!({
            "conversation_id": event.conversation_id,
        }));
        Ok(())
    }

    async fn on_all_hidden(&self, _event: &ConversationAllHidden) -> AnyhowResult<()> {
        let _ = self.app.emit("im://conversation_all_hidden", serde_json::json!({}));
        Ok(())
    }

    async fn on_deleted(&self, event: &ConversationDeleted) -> AnyhowResult<()> {
        let _ = self.app.emit("im://conversation_deleted", serde_json::json!({
            "conversation_id": event.conversation_id,
        }));
        Ok(())
    }

    async fn on_messages_cleared(&self, event: &ConversationMessagesCleared) -> AnyhowResult<()> {
        let _ = self.app.emit("im://conversation_messages_cleared", serde_json::json!({
            "conversation_id": event.conversation_id,
        }));
        Ok(())
    }

    async fn on_updated(&self, event: &ConversationUpdated) -> AnyhowResult<()> {
        let _ = self.app.emit("im://conversation_updated", serde_json::json!({
            "conversation_id": event.conversation_id,
        }));
        Ok(())
    }

    async fn on_muted(&self, event: &ConversationMuted) -> AnyhowResult<()> {
        let _ = self.app.emit("im://conversation_muted", serde_json::json!({
            "conversation_id": event.conversation_id,
        }));
        Ok(())
    }

    async fn on_unmuted(&self, event: &ConversationUnmuted) -> AnyhowResult<()> {
        let _ = self.app.emit("im://conversation_unmuted", serde_json::json!({
            "conversation_id": event.conversation_id,
        }));
        Ok(())
    }

    async fn on_pinned(&self, event: &ConversationPinned) -> AnyhowResult<()> {
        let _ = self.app.emit("im://conversation_pinned", serde_json::json!({
            "conversation_id": event.conversation_id,
        }));
        Ok(())
    }

    async fn on_unpinned(&self, event: &ConversationUnpinned) -> AnyhowResult<()> {
        let _ = self.app.emit("im://conversation_unpinned", serde_json::json!({
            "conversation_id": event.conversation_id,
        }));
        Ok(())
    }

    async fn on_archived(&self, event: &ConversationArchived) -> AnyhowResult<()> {
        let _ = self.app.emit("im://conversation_archived", serde_json::json!({
            "conversation_id": event.conversation_id,
        }));
        Ok(())
    }

    async fn on_unarchived(&self, event: &ConversationUnarchived) -> AnyhowResult<()> {
        let _ = self.app.emit("im://conversation_unarchived", serde_json::json!({
            "conversation_id": event.conversation_id,
        }));
        Ok(())
    }

    async fn on_input_state_updated(&self, event: &ConversationInputStateUpdated) -> AnyhowResult<()> {
        let _ = self.app.emit("im://conversation_input_state_updated", serde_json::json!({
            "conversation_id": event.conversation_id,
            "user_id": event.user_id,
            "state_type": event.state_type,
        }));
        Ok(())
    }

    async fn on_input_state_cleared(&self, event: &ConversationInputStateCleared) -> AnyhowResult<()> {
        let _ = self.app.emit("im://conversation_input_state_cleared", serde_json::json!({
            "conversation_id": event.conversation_id,
        }));
        Ok(())
    }
}
