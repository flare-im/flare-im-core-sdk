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
                applied_seqs.push(event.seq);
                continue;
            }
            match self.apply_event(user_id, event, mode).await {
                Ok(true) => applied_seqs.push(event.seq),
                Ok(false) => {
                    self.event_deduper.forget(event).await;
                    tracing::warn!(
                        conversation_id = %event.conversation_id,
                        seq = event.seq,
                        event_type = event.r#type,
                        "关键事件目标状态暂不可用，保留后续重放机会"
                    );
                }
                Err(error) => {
                    self.event_deduper.forget(event).await;
                    tracing::warn!(
                        conversation_id = %event.conversation_id,
                        seq = event.seq,
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
            match self
                .stores
                .messages
                .apply_edit_event(
                    &edit.server_msg_id,
                    edit.new_content.clone(),
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
        if let Some(DomainEventPayload::BurnScheduled(burn_scheduled)) = &event.payload {
            let applied = self
                .stores
                .messages
                .apply_burn_scheduled_event(
                    &burn_scheduled.message_id,
                    burn_scheduled.burn_at,
                    burn_scheduled.event_time,
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
                .publish(SdkEvent::Message(MessageEvent::BurnScheduled {
                    conversation_id: event.conversation_id.clone(),
                    event: burn_scheduled.clone(),
                }));
            self.publish_extension_if_needed(event, mode, false);
            return Ok(true);
        }
        if let Some(DomainEventPayload::Burned(burned)) = &event.payload {
            let applied = self
                .stores
                .messages
                .apply_burned_event(
                    &burned.message_id,
                    burned.burn_at,
                    burned.burned_at,
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
            self.bus.publish(SdkEvent::Message(MessageEvent::Burned {
                conversation_id: event.conversation_id.clone(),
                event: burned.clone(),
            }));
            self.publish_extension_if_needed(event, mode, false);
            return Ok(true);
        }
        if let Some(DomainEventPayload::HardDeleted(hard_deleted)) = &event.payload {
            let applied = self
                .stores
                .messages
                .apply_hard_deleted_event(
                    &hard_deleted.message_id,
                    hard_deleted.burn_at,
                    hard_deleted.burned_at,
                    hard_deleted.event_time,
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
                .publish(SdkEvent::Message(MessageEvent::HardDeleted {
                    conversation_id: event.conversation_id.clone(),
                    event: hard_deleted.clone(),
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
        if let Some(DomainEventPayload::Presence(presence)) = &event.payload {
            self.bus
                .publish(SdkEvent::Message(MessageEvent::PresenceChanged {
                    conversation_id: event.conversation_id.clone(),
                    event: presence.clone(),
                }));
            self.publish_extension_if_needed(event, mode, false);
            return Ok(true);
        }
        if let Some(DomainEventPayload::CallSignal(call)) = &event.payload {
            self.bus
                .publish(SdkEvent::Message(MessageEvent::CallSignal {
                    conversation_id: event.conversation_id.clone(),
                    event: Box::new(call.clone()),
                }));
            // EVENT_CALL_SIGNAL 已由 MessageEvent::CallSignal 承载，不再走 Extension 重复下发。
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
        let target_seq = event
            .event_seq
            .or(if event.seq > 0 { Some(event.seq) } else { None })
            .unwrap_or_default();
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
        if event.r#type == flare_proto::common::EventType::EventCallSignal as i32 {
            return;
        }
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
    use crate::core::event::{EventBus, MessageEvent, SdkEvent};
    use crate::domain::{
        ConversationReader, ConversationWriter, MessageReader, MessageWriter, SyncCursorReader,
        SyncCursorVo, SyncCursorWriter,
    };
    use crate::infrastructure::persistence::StoreProvider;
    use crate::infrastructure::persistence::{SqliteMessageRepo, sqlite_init_schema};
    use crate::shared::error::Result;
    use async_trait::async_trait;
    use flare_proto::common::BurnStatus;
    use sqlx::SqlitePool;
    use std::sync::Arc;
    use tokio::time::{Duration, timeout};

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
        message.seq = 5;
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
            flare_proto::common::MessageStatus::Read as i32
        );
        assert!(updated.is_read);
    }

    #[tokio::test]
    async fn replay_burned_event_is_idempotent_and_emits_once() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite_init_schema(&pool).await.unwrap();
        let message_repo = Arc::new(SqliteMessageRepo::new(pool));
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
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
        message.server_id = "server-burn-sync-1".to_string();
        message.client_msg_id = "client-burn-sync-1".to_string();
        message.conversation_id = "conv-burn".to_string();
        message.sender_id = "u1".to_string();
        message.content_bytes = b"hello".to_vec();
        message_repo.save_batch(&[message]).await.unwrap();

        let newer = flare_proto::common::Event {
            event_id: "evt-burned-1".to_string(),
            event_seq: Some(100),
            conversation_id: "conv-burn".to_string(),
            payload: Some(flare_proto::common::event::Payload::Burned(
                flare_proto::common::MessageBurnedEvent {
                    conversation_id: "conv-burn".to_string(),
                    message_id: "server-burn-sync-1".to_string(),
                    server_id: "server-burn-sync-1".to_string(),
                    burn_at: 2_000_000_001,
                    burned_at: 2_000_000_005,
                    event_time: 2_000_000_005,
                    ..Default::default()
                },
            )),
            ..Default::default()
        };
        let stale = flare_proto::common::Event {
            event_id: "evt-burned-2".to_string(),
            event_seq: Some(99),
            conversation_id: "conv-burn".to_string(),
            payload: Some(flare_proto::common::event::Payload::Burned(
                flare_proto::common::MessageBurnedEvent {
                    conversation_id: "conv-burn".to_string(),
                    message_id: "server-burn-sync-1".to_string(),
                    server_id: "server-burn-sync-1".to_string(),
                    burn_at: 2_000_000_100,
                    burned_at: 2_000_000_200,
                    event_time: 2_000_000_200,
                    ..Default::default()
                },
            )),
            ..Default::default()
        };

        applier
            .apply_events("u1", &[newer, stale], ReplayMode::CriticalEvents)
            .await;

        let updated = message_repo
            .get("server-burn-sync-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.burn_status, BurnStatus::Burned as i32);
        assert_eq!(updated.burn_at, Some(2_000_000_001));
        assert_eq!(updated.burned_at, Some(2_000_000_005));
        assert!(updated.content_bytes.is_empty());
        assert_eq!(
            updated.extra.get("burn_placeholder").map(String::as_str),
            Some("该消息已销毁")
        );

        let mut burned_events = 0usize;
        while let Ok(Ok(event)) = timeout(Duration::from_millis(25), receiver.recv()).await {
            if let SdkEvent::Message(MessageEvent::Burned { event, .. }) = event {
                burned_events += 1;
                assert_eq!(event.message_id, "server-burn-sync-1");
            }
        }
        assert_eq!(burned_events, 1);
    }
}

fn operation_seq(event: &flare_proto::common::Event) -> Option<u64> {
    event
        .event_seq
        .or(if event.seq > 0 { Some(event.seq) } else { None })
}
