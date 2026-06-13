use flare_proto::common::{MessageStatus, event::Payload as DomainEventPayload};
use prost::Message;

use crate::application::services::EventDeduper;
use crate::core::event::{ConversationEvent, EventBus, ExtensionEvent, MessageEvent, SdkEvent};
use crate::domain::{EditApplyResult, OperationApplyResult, SyncPolicy, local_cleared_through_seq};
use crate::infrastructure::persistence::StoreProvider;
use crate::shared::error::Result;

use super::models::ReplayMode;

pub(crate) struct SyncEventApplier {
    stores: StoreProvider,
    bus: EventBus,
    event_deduper: EventDeduper,
}

impl SyncEventApplier {
    pub(crate) fn new(stores: StoreProvider, bus: EventBus, event_deduper: EventDeduper) -> Self {
        Self {
            stores,
            bus,
            event_deduper,
        }
    }

    pub(crate) async fn apply_events(
        &self,
        user_id: &str,
        events: &[flare_proto::common::Event],
        mode: ReplayMode,
    ) -> Vec<u64> {
        let mut applied_seqs = Vec::new();
        for event in events {
            if !self.event_deduper.record_if_new(event).await {
                applied_seqs.push(event.conversation_seq);
                continue;
            }
            match self.apply_event(user_id, event, mode).await {
                Ok(true) => applied_seqs.push(event.conversation_seq),
                Ok(false) => {
                    self.event_deduper.forget(event).await;
                    tracing::warn!(
                        conversation_id = %event.conversation_id,
                        seq = event.conversation_seq,
                        event_type = event.r#type,
                        "关键事件目标状态暂不可用，保留后续重放机会"
                    );
                }
                Err(error) => {
                    self.event_deduper.forget(event).await;
                    tracing::warn!(
                        conversation_id = %event.conversation_id,
                        seq = event.conversation_seq,
                        event_type = event.r#type,
                        error = %error,
                        "关键事件应用失败，保留后续重放机会"
                    );
                }
            }
        }
        applied_seqs
    }

    async fn apply_event(
        &self,
        user_id: &str,
        event: &flare_proto::common::Event,
        mode: ReplayMode,
    ) -> Result<bool> {
        if let Some(DomainEventPayload::Recall(recall)) = &event.payload {
            let Some(_) = self.stores.messages.get(&recall.server_msg_id).await? else {
                return self.missing_target_is_already_covered(user_id, event).await;
            };
            self.stores
                .messages
                .update_status(&recall.server_msg_id, MessageStatus::Recalled as i32)
                .await?;
            if matches!(mode, ReplayMode::SingleConversation) {
                self.bus.publish(SdkEvent::Message(MessageEvent::Recalled {
                    conversation_id: event.conversation_id.clone(),
                    event: recall.clone(),
                }));
            }
            self.publish_extension_if_needed(event, mode, false);
            return Ok(true);
        }
        if let Some(DomainEventPayload::Edit(edit)) = &event.payload {
            let Some(new_content) = edit.new_content.as_ref() else {
                tracing::warn!(
                    server_msg_id = %edit.server_msg_id,
                    "sync edit event missing new_content; keeping event replayable"
                );
                return Ok(false);
            };
            match self
                .stores
                .messages
                .apply_edit_event(
                    &edit.server_msg_id,
                    new_content.encode_to_vec(),
                    edit.edit_version,
                )
                .await
            {
                Ok(EditApplyResult::IgnoredStale) => {
                    return Ok(true);
                }
                Ok(EditApplyResult::NotFound) => {
                    return self.missing_target_is_already_covered(user_id, event).await;
                }
                Ok(EditApplyResult::Applied) => {}
                Err(error) => return Err(error),
            }
            self.bus.publish(SdkEvent::Message(MessageEvent::Edited {
                conversation_id: event.conversation_id.clone(),
                server_msg_id: edit.server_msg_id.clone(),
                edit_version: Some(edit.edit_version),
            }));
            self.publish_extension_if_needed(event, mode, false);
            return Ok(true);
        }
        if let Some(DomainEventPayload::Reaction(reaction)) = &event.payload {
            let applied = self
                .stores
                .messages
                .apply_reaction_event(
                    &event.conversation_id,
                    &reaction.server_msg_id,
                    &reaction.user_id,
                    &reaction.emoji,
                    reaction.action,
                    operation_seq(event),
                )
                .await;
            if matches!(applied, Ok(OperationApplyResult::IgnoredStale)) {
                return Ok(true);
            }
            match applied {
                Ok(OperationApplyResult::Applied) => {}
                Ok(OperationApplyResult::NotFound) => {
                    return self.missing_target_is_already_covered(user_id, event).await;
                }
                Ok(OperationApplyResult::IgnoredStale) => return Ok(true),
                Err(error) => return Err(error),
            }
            self.bus
                .publish(SdkEvent::Message(MessageEvent::ReactionChanged {
                    conversation_id: event.conversation_id.clone(),
                    server_msg_id: reaction.server_msg_id.clone(),
                    user_id: reaction.user_id.clone(),
                    emoji: reaction.emoji.clone(),
                    action: reaction.action,
                }));
            self.publish_extension_if_needed(event, mode, false);
            return Ok(true);
        }
        if let Some(DomainEventPayload::Delete(delete)) = &event.payload {
            let operation_seq = operation_seq(event);
            if SyncPolicy::evaluate_delete_visibility(
                user_id,
                delete.scope.unwrap_or(1),
                delete.target_user_id.as_deref(),
            )
            .apply_to_current_user
            {
                match self
                    .stores
                    .messages
                    .apply_delete_event(&delete.server_msg_id, operation_seq)
                    .await
                {
                    Ok(OperationApplyResult::IgnoredStale) => return Ok(true),
                    Ok(OperationApplyResult::NotFound) => {
                        return self.missing_target_is_already_covered(user_id, event).await;
                    }
                    Ok(OperationApplyResult::Applied) => {}
                    Err(error) => return Err(error),
                }
                self.bus.publish(SdkEvent::Message(MessageEvent::Deleted {
                    conversation_id: event.conversation_id.clone(),
                    event: delete.clone(),
                }));
                if matches!(mode, ReplayMode::SingleConversation) {
                    self.bus.publish(SdkEvent::Extension(ExtensionEvent {
                        source: "sync_replay".to_string(),
                        event_type: "message_delete".to_string(),
                        payload: delete.encode_to_vec(),
                    }));
                }
            }
            self.publish_extension_if_needed(event, mode, true);
            return Ok(true);
        }
        if let Some(DomainEventPayload::Read(read)) = &event.payload {
            if !user_id.is_empty()
                && !read.user_id.is_empty()
                && read.user_id != user_id
                && read.read_seq > 0
            {
                self.stores
                    .messages
                    .mark_outgoing_read_upto_seq(&event.conversation_id, user_id, read.read_seq)
                    .await?;
            }
            self.bus
                .publish(SdkEvent::Message(MessageEvent::ReadReceipt {
                    conversation_id: event.conversation_id.clone(),
                    event: read.clone(),
                }));
            self.publish_extension_if_needed(event, mode, false);
            return Ok(true);
        }
        if let Some(DomainEventPayload::RetentionScheduled(retention_scheduled)) = &event.payload {
            let (Some(policy), Some(state)) = (
                retention_scheduled.policy.as_ref(),
                retention_scheduled.state.as_ref(),
            ) else {
                tracing::warn!(
                    server_msg_id = %retention_scheduled.server_msg_id,
                    "sync retention scheduled event missing policy/state; keeping event replayable"
                );
                return Ok(false);
            };
            let applied = self
                .stores
                .messages
                .apply_retention_scheduled_event(
                    &retention_scheduled.server_msg_id,
                    policy,
                    state,
                    retention_scheduled.scheduled_at,
                    operation_seq(event),
                )
                .await;
            if matches!(applied, Ok(OperationApplyResult::IgnoredStale)) {
                return Ok(true);
            }
            match applied {
                Ok(OperationApplyResult::Applied) => {}
                Ok(OperationApplyResult::NotFound) => {
                    return self.missing_target_is_already_covered(user_id, event).await;
                }
                Ok(OperationApplyResult::IgnoredStale) => return Ok(true),
                Err(error) => return Err(error),
            }
            self.bus
                .publish(SdkEvent::Message(MessageEvent::RetentionScheduled {
                    conversation_id: event.conversation_id.clone(),
                    event: retention_scheduled.clone(),
                }));
            self.publish_extension_if_needed(event, mode, false);
            return Ok(true);
        }
        if let Some(DomainEventPayload::RetentionExpired(retention_expired)) = &event.payload {
            let Some(state) = retention_expired.state.as_ref() else {
                tracing::warn!(
                    server_msg_id = %retention_expired.server_msg_id,
                    "sync retention expired event missing state; keeping event replayable"
                );
                return Ok(false);
            };
            let applied = self
                .stores
                .messages
                .apply_retention_expired_event(
                    &retention_expired.server_msg_id,
                    state,
                    retention_expired.expired_at,
                    operation_seq(event),
                )
                .await;
            if matches!(applied, Ok(OperationApplyResult::IgnoredStale)) {
                return Ok(true);
            }
            match applied {
                Ok(OperationApplyResult::Applied) => {}
                Ok(OperationApplyResult::NotFound) => {
                    return self.missing_target_is_already_covered(user_id, event).await;
                }
                Ok(OperationApplyResult::IgnoredStale) => return Ok(true),
                Err(error) => return Err(error),
            }
            self.bus
                .publish(SdkEvent::Message(MessageEvent::RetentionExpired {
                    conversation_id: event.conversation_id.clone(),
                    event: retention_expired.clone(),
                }));
            self.publish_extension_if_needed(event, mode, false);
            return Ok(true);
        }
        if let Some(DomainEventPayload::RetentionPurged(retention_purged)) = &event.payload {
            let Some(state) = retention_purged.state.as_ref() else {
                tracing::warn!(
                    server_msg_id = %retention_purged.server_msg_id,
                    "sync retention purged event missing state; keeping event replayable"
                );
                return Ok(false);
            };
            let applied = self
                .stores
                .messages
                .apply_retention_purged_event(
                    &retention_purged.server_msg_id,
                    state,
                    retention_purged.purged_at,
                    operation_seq(event),
                )
                .await;
            if matches!(applied, Ok(OperationApplyResult::IgnoredStale)) {
                return Ok(true);
            }
            match applied {
                Ok(OperationApplyResult::Applied) => {}
                Ok(OperationApplyResult::NotFound) => {
                    return self.missing_target_is_already_covered(user_id, event).await;
                }
                Ok(OperationApplyResult::IgnoredStale) => return Ok(true),
                Err(error) => return Err(error),
            }
            self.bus
                .publish(SdkEvent::Message(MessageEvent::RetentionPurged {
                    conversation_id: event.conversation_id.clone(),
                    event: retention_purged.clone(),
                }));
            self.publish_extension_if_needed(event, mode, false);
            return Ok(true);
        }
        if let Some(DomainEventPayload::Pin(pin)) = &event.payload {
            let applied = self
                .stores
                .messages
                .apply_pin_event(&pin.server_msg_id, true, operation_seq(event))
                .await;
            if matches!(applied, Ok(OperationApplyResult::IgnoredStale)) {
                return Ok(true);
            }
            match applied {
                Ok(OperationApplyResult::Applied) => {}
                Ok(OperationApplyResult::NotFound) => {
                    return self.missing_target_is_already_covered(user_id, event).await;
                }
                Ok(OperationApplyResult::IgnoredStale) => return Ok(true),
                Err(error) => return Err(error),
            }
            self.bus.publish(SdkEvent::Message(MessageEvent::Pinned {
                conversation_id: event.conversation_id.clone(),
                event: pin.clone(),
            }));
            self.publish_extension_if_needed(event, mode, false);
            return Ok(true);
        }
        if let Some(DomainEventPayload::Unpin(unpin)) = &event.payload {
            let applied = self
                .stores
                .messages
                .apply_pin_event(&unpin.server_msg_id, false, operation_seq(event))
                .await;
            if matches!(applied, Ok(OperationApplyResult::IgnoredStale)) {
                return Ok(true);
            }
            match applied {
                Ok(OperationApplyResult::Applied) => {}
                Ok(OperationApplyResult::NotFound) => {
                    return self.missing_target_is_already_covered(user_id, event).await;
                }
                Ok(OperationApplyResult::IgnoredStale) => return Ok(true),
                Err(error) => return Err(error),
            }
            self.bus.publish(SdkEvent::Message(MessageEvent::Unpinned {
                conversation_id: event.conversation_id.clone(),
                event: unpin.clone(),
            }));
            self.publish_extension_if_needed(event, mode, false);
            return Ok(true);
        }
        if let Some(DomainEventPayload::Mark(mark)) = &event.payload {
            let applied = self
                .stores
                .messages
                .apply_mark_event(
                    &mark.server_msg_id,
                    mark.mark_type,
                    if mark.color.trim().is_empty() {
                        None
                    } else {
                        Some(mark.color.as_str())
                    },
                    true,
                    operation_seq(event),
                )
                .await;
            if matches!(applied, Ok(OperationApplyResult::IgnoredStale)) {
                return Ok(true);
            }
            match applied {
                Ok(OperationApplyResult::Applied) => {}
                Ok(OperationApplyResult::NotFound) => {
                    return self.missing_target_is_already_covered(user_id, event).await;
                }
                Ok(OperationApplyResult::IgnoredStale) => return Ok(true),
                Err(error) => return Err(error),
            }
            self.bus.publish(SdkEvent::Message(MessageEvent::Marked {
                conversation_id: event.conversation_id.clone(),
                event: mark.clone(),
            }));
            self.publish_extension_if_needed(event, mode, false);
            return Ok(true);
        }
        if let Some(DomainEventPayload::Unmark(unmark)) = &event.payload {
            let applied = self
                .stores
                .messages
                .apply_mark_event(
                    &unmark.server_msg_id,
                    unmark.mark_type,
                    None,
                    false,
                    operation_seq(event),
                )
                .await;
            if matches!(applied, Ok(OperationApplyResult::IgnoredStale)) {
                return Ok(true);
            }
            match applied {
                Ok(OperationApplyResult::Applied) => {}
                Ok(OperationApplyResult::NotFound) => {
                    return self.missing_target_is_already_covered(user_id, event).await;
                }
                Ok(OperationApplyResult::IgnoredStale) => return Ok(true),
                Err(error) => return Err(error),
            }
            self.bus.publish(SdkEvent::Message(MessageEvent::Unmarked {
                conversation_id: event.conversation_id.clone(),
                event: unmark.clone(),
            }));
            self.publish_extension_if_needed(event, mode, false);
            return Ok(true);
        }
        if let Some(DomainEventPayload::Custom(custom)) = &event.payload {
            self.bus.publish(SdkEvent::Message(MessageEvent::Custom {
                conversation_id: event.conversation_id.clone(),
                event: custom.clone(),
            }));
            self.publish_extension_if_needed(event, mode, false);
            return Ok(true);
        }
        if let Some(DomainEventPayload::ConversationDelete(_)) = &event.payload {
            if let Err(error) = self
                .stores
                .conversations
                .delete(&event.conversation_id)
                .await
            {
                tracing::warn!(
                    conversation_id = %event.conversation_id,
                    error = %error,
                    "ConversationDelete: local conversation purge failed"
                );
            }
            self.bus
                .publish(SdkEvent::Conversation(ConversationEvent::Deleted {
                    conversation_id: event.conversation_id.clone(),
                }));
            self.publish_extension_if_needed(event, mode, false);
            return Ok(true);
        }
        if let Some(DomainEventPayload::Conversation(_)) = &event.payload {
            self.bus
                .publish(SdkEvent::Conversation(ConversationEvent::Updated {
                    conversation_id: event.conversation_id.clone(),
                }));
            self.publish_extension_if_needed(event, mode, false);
            return Ok(true);
        }
        self.publish_extension_if_needed(event, mode, false);
        Ok(true)
    }

    async fn missing_target_is_already_covered(
        &self,
        user_id: &str,
        event: &flare_proto::common::Event,
    ) -> Result<bool> {
        let target_seq = if event.conversation_seq > 0 {
            event.conversation_seq
        } else {
            0
        };
        if user_id.is_empty() || event.conversation_id.trim().is_empty() || target_seq == 0 {
            return Ok(false);
        }
        let message_cursor = self
            .stores
            .cursors
            .get_conversation_cursor(user_id, &event.conversation_id)
            .await?
            .map(|cursor| cursor.last_seq)
            .unwrap_or_default();
        if message_cursor >= target_seq {
            return Ok(true);
        }

        let local_max_seq = self
            .stores
            .conversations
            .get_local_max_seq(&event.conversation_id)
            .await?;
        if local_max_seq >= target_seq {
            return Ok(true);
        }

        let cleared_floor = self
            .stores
            .conversations
            .get(&event.conversation_id)
            .await?
            .map(|conversation| {
                local_cleared_through_seq(&conversation.ext).max(conversation.visible_after_seq)
            })
            .unwrap_or_default();
        Ok(cleared_floor >= target_seq)
    }

    fn publish_extension_if_needed(
        &self,
        event: &flare_proto::common::Event,
        mode: ReplayMode,
        is_delete: bool,
    ) {
        match mode {
            ReplayMode::SingleConversation => {
                if is_delete {
                    return;
                }
                match event.r#type {
                    11..=15 | 99 => {
                        self.bus.publish(SdkEvent::Extension(ExtensionEvent {
                            source: "sync_replay".to_string(),
                            event_type: format!("event_type_{}", event.r#type),
                            payload: event.encode_to_vec(),
                        }));
                    }
                    _ => {}
                }
            }
            ReplayMode::CriticalEvents => {
                self.bus.publish(SdkEvent::Extension(ExtensionEvent {
                    source: "sync_query_events".to_string(),
                    event_type: format!("event_type_{}", event.r#type),
                    payload: event.encode_to_vec(),
                }));
            }
        }
    }
}

#[cfg(all(test, feature = "storage-sqlite"))]
mod tests {
    use super::{ReplayMode, SyncEventApplier};
    use crate::application::services::EventDeduper;
    use crate::core::event::EventBus;
    use crate::domain::{
        ConversationReader, ConversationWriter, MessageReader, MessageWriter, SyncCursorReader,
        SyncCursorVo, SyncCursorWriter,
    };
    use crate::infrastructure::persistence::StoreProvider;
    use crate::infrastructure::persistence::{SqliteMessageRepo, sqlite_init_schema};
    use crate::shared::error::Result;
    use async_trait::async_trait;
    use sqlx::SqlitePool;
    use std::sync::Arc;

    struct NoopConversationStore;
    struct NoopSyncCursorStore;

    #[async_trait]
    impl ConversationReader for NoopConversationStore {
        async fn get(&self, _conversation_id: &str) -> Result<Option<crate::model::Conversation>> {
            Ok(None)
        }

        async fn list(&self) -> Result<Vec<crate::model::Conversation>> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl ConversationWriter for NoopConversationStore {
        async fn save_batch(&self, _conversations: &[crate::model::Conversation]) -> Result<()> {
            Ok(())
        }
        async fn save_one(&self, _conversation: &crate::model::Conversation) -> Result<()> {
            Ok(())
        }
        async fn update_unread(
            &self,
            _conversation_id: &str,
            _unread_count: u32,
            _last_read_seq: u64,
        ) -> Result<()> {
            Ok(())
        }
        async fn set_pinned(&self, _conversation_id: &str, _pinned: bool) -> Result<()> {
            Ok(())
        }
        async fn set_muted(&self, _conversation_id: &str, _muted: bool) -> Result<()> {
            Ok(())
        }
        async fn set_archived(&self, _conversation_id: &str, _archived: bool) -> Result<()> {
            Ok(())
        }

        async fn mark_unread(&self, _conversation_id: &str) -> Result<u32> {
            Ok(1)
        }
        async fn update_draft(&self, _conversation_id: &str, _draft: Option<&str>) -> Result<()> {
            Ok(())
        }
        async fn delete(&self, _conversation_id: &str) -> Result<()> {
            Ok(())
        }
        async fn clear_local_chat_history(
            &self,
            _conversation_id: &str,
            _cleared_through_seq: u64,
        ) -> Result<()> {
            Ok(())
        }
        async fn update_last_message(
            &self,
            _conversation_id: &str,
            _last_message_id: &str,
            _last_sender_id: &str,
            _last_message_at: u64,
            _last_message_preview: Option<&str>,
            _max_seq: u64,
        ) -> Result<()> {
            Ok(())
        }
        async fn recompute_unread_for_user(
            &self,
            _conversation_id: &str,
            _current_user_id: &str,
        ) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl SyncCursorReader for NoopSyncCursorStore {
        async fn get_conversation_cursor(
            &self,
            _user_id: &str,
            _conversation_id: &str,
        ) -> Result<Option<SyncCursorVo>> {
            Ok(None)
        }

        async fn get_raw(&self, _key: &str) -> Result<Option<String>> {
            Ok(None)
        }
    }

    #[async_trait]
    impl SyncCursorWriter for NoopSyncCursorStore {
        async fn save_conversation_cursor(&self, _cursor: &SyncCursorVo) -> Result<()> {
            Ok(())
        }

        async fn save_raw(&self, _key: &str, _cursor: &str) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn replay_read_receipt_marks_outgoing_messages_as_read() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite_init_schema(&pool).await.unwrap();
        let message_repo = Arc::new(SqliteMessageRepo::new(pool));
        let bus = EventBus::new();
        let applier = SyncEventApplier::new(
            StoreProvider {
                messages: message_repo.clone(),
                conversations: Arc::new(NoopConversationStore),
                conversation_participants: None,
                cursors: Arc::new(NoopSyncCursorStore),
                pending_send_reader: None,
                pending_send_writer: None,
                upload_manifest_store: None,
                media_cache_store: None,
                media_cache_admin: None,
                user_file_download_store: None,
                user_profiles_reader: None,
                user_profiles_writer: None,
            },
            bus,
            EventDeduper::new(Some(32)),
        );

        let mut message = crate::model::IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-outgoing-1".to_string();
        message.client_msg_id = "client-outgoing-1".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();
        message.conversation_seq = 5;
        message.status = flare_proto::common::MessageStatus::Sent as i32;
        message_repo.save_batch(&[message]).await.unwrap();

        let event = flare_proto::common::Event {
            conversation_id: "conv-1".to_string(),
            payload: Some(flare_proto::common::event::Payload::Read(
                flare_proto::common::ReadReceiptEvent {
                    conversation_id: "conv-1".to_string(),
                    read_seq: 5,
                    user_id: "u2".to_string(),
                    ..Default::default()
                },
            )),
            ..Default::default()
        };

        applier
            .apply_events("u1", &[event], ReplayMode::CriticalEvents)
            .await;

        let updated = message_repo
            .get("server-outgoing-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated.status,
            flare_proto::common::MessageStatus::Sent as i32
        );
        assert!(updated.is_read);
    }
}

fn operation_seq(event: &flare_proto::common::Event) -> Option<u64> {
    (event.conversation_seq > 0).then_some(event.conversation_seq)
}
