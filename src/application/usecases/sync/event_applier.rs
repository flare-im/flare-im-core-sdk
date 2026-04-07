use flare_proto::common::{MessageStatus, event::Payload as DomainEventPayload};
use prost::Message;

use crate::application::event_deduper::EventDeduper;
use crate::domain::{EditApplyResult, OperationApplyResult, SyncPolicy};
use crate::event::{
    ConversationEvent, EventBus, ExtensionEvent, MessageEvent, SdkEvent,
};
use crate::store::StoreProvider;

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
    ) {
        for event in events {
            if !self.event_deduper.record_if_new(event).await {
                continue;
            }
            self.apply_event(user_id, event, mode).await;
        }
    }

    async fn apply_event(
        &self,
        user_id: &str,
        event: &flare_proto::common::Event,
        mode: ReplayMode,
    ) {
        if let Some(DomainEventPayload::Recall(recall)) = &event.payload {
            let _ = self
                .stores
                .messages
                .update_status(&recall.server_msg_id, MessageStatus::Recalled as i32)
                .await;
            if matches!(mode, ReplayMode::SingleConversation) {
                self.bus.publish(SdkEvent::Message(MessageEvent::Recalled {
                    conversation_id: event.conversation_id.clone(),
                    event: recall.clone(),
                }));
            }
            self.publish_extension_if_needed(event, mode, false);
            return;
        }
        if let Some(DomainEventPayload::Edit(edit)) = &event.payload {
            match self
                .stores
                .messages
                .apply_edit_event(&edit.server_msg_id, edit.new_content.clone(), edit.edit_version)
                .await
            {
                Ok(EditApplyResult::IgnoredStale) => {
                    return;
                }
                Ok(_) | Err(_) => {}
            }
            self.bus.publish(SdkEvent::Message(MessageEvent::Edited {
                conversation_id: event.conversation_id.clone(),
                server_msg_id: edit.server_msg_id.clone(),
                edit_version: Some(edit.edit_version),
            }));
            self.publish_extension_if_needed(event, mode, false);
            return;
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
                return;
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
            return;
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
                    Ok(OperationApplyResult::IgnoredStale) => return,
                    Ok(_) | Err(_) => {}
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
            return;
        }
        if let Some(DomainEventPayload::Read(read)) = &event.payload {
            if !user_id.is_empty()
                && !read.user_id.is_empty()
                && read.user_id != user_id
                && read.read_seq > 0
            {
                let _ = self
                    .stores
                    .messages
                    .mark_outgoing_read_upto_seq(
                        &event.conversation_id,
                        user_id,
                        read.read_seq,
                    )
                    .await;
            }
            self.bus.publish(SdkEvent::Message(MessageEvent::ReadReceipt {
                conversation_id: event.conversation_id.clone(),
                event: read.clone(),
            }));
            self.publish_extension_if_needed(event, mode, false);
            return;
        }
        if let Some(DomainEventPayload::Pin(pin)) = &event.payload {
            let applied = self
                .stores
                .messages
                .apply_pin_event(&pin.server_msg_id, true, operation_seq(event))
                .await;
            if matches!(applied, Ok(OperationApplyResult::IgnoredStale)) {
                return;
            }
            self.bus.publish(SdkEvent::Message(MessageEvent::Pinned {
                conversation_id: event.conversation_id.clone(),
                event: pin.clone(),
            }));
            self.publish_extension_if_needed(event, mode, false);
            return;
        }
        if let Some(DomainEventPayload::Unpin(unpin)) = &event.payload {
            let applied = self
                .stores
                .messages
                .apply_pin_event(&unpin.server_msg_id, false, operation_seq(event))
                .await;
            if matches!(applied, Ok(OperationApplyResult::IgnoredStale)) {
                return;
            }
            self.bus.publish(SdkEvent::Message(MessageEvent::Unpinned {
                conversation_id: event.conversation_id.clone(),
                event: unpin.clone(),
            }));
            self.publish_extension_if_needed(event, mode, false);
            return;
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
                return;
            }
            self.bus.publish(SdkEvent::Message(MessageEvent::Marked {
                conversation_id: event.conversation_id.clone(),
                event: mark.clone(),
            }));
            self.publish_extension_if_needed(event, mode, false);
            return;
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
                return;
            }
            self.bus.publish(SdkEvent::Message(MessageEvent::Unmarked {
                conversation_id: event.conversation_id.clone(),
                event: unmark.clone(),
            }));
            self.publish_extension_if_needed(event, mode, false);
            return;
        }
        if let Some(DomainEventPayload::Presence(presence)) = &event.payload {
            self.bus
                .publish(SdkEvent::Message(MessageEvent::PresenceChanged {
                    conversation_id: event.conversation_id.clone(),
                    event: presence.clone(),
                }));
            self.publish_extension_if_needed(event, mode, false);
            return;
        }
        if let Some(DomainEventPayload::CallSignal(call)) = &event.payload {
            self.bus.publish(SdkEvent::Message(MessageEvent::CallSignal {
                conversation_id: event.conversation_id.clone(),
                event: call.clone(),
            }));
            self.publish_extension_if_needed(event, mode, false);
            return;
        }
        if let Some(DomainEventPayload::Custom(custom)) = &event.payload {
            self.bus.publish(SdkEvent::Message(MessageEvent::Custom {
                conversation_id: event.conversation_id.clone(),
                event: custom.clone(),
            }));
            self.publish_extension_if_needed(event, mode, false);
            return;
        }
        if let Some(DomainEventPayload::ConversationDelete(_)) = &event.payload {
            self.bus.publish(SdkEvent::Conversation(ConversationEvent::Deleted {
                conversation_id: event.conversation_id.clone(),
            }));
            self.publish_extension_if_needed(event, mode, false);
            return;
        }
        if let Some(DomainEventPayload::Conversation(_)) = &event.payload {
            self.bus.publish(SdkEvent::Conversation(ConversationEvent::Updated {
                conversation_id: event.conversation_id.clone(),
            }));
            self.publish_extension_if_needed(event, mode, false);
            return;
        }
        self.publish_extension_if_needed(event, mode, false);
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
                    10..=15 | 99 => {
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
    use crate::application::event_deduper::EventDeduper;
    use crate::domain::{
        ConversationReader, ConversationWriter, MessageReader, MessageWriter, SyncCursorReader,
        SyncCursorVo, SyncCursorWriter,
    };
    use crate::event::EventBus;
    use crate::store::StoreProvider;
    use crate::Result;
    use async_trait::async_trait;
    use std::sync::Arc;
    use crate::store::{sqlite_init_schema, SqliteMessageRepo};
    use sqlx::SqlitePool;

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
        async fn update_draft(&self, _conversation_id: &str, _draft: Option<&str>) -> Result<()> {
            Ok(())
        }
        async fn delete(&self, _conversation_id: &str) -> Result<()> {
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
                cursors: Arc::new(NoopSyncCursorStore),
                pending_send_reader: None,
                pending_send_writer: None,
                upload_manifest_store: None,
                media_cache_store: None,
                media_cache_admin: None,
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

        applier.apply_events("u1", &[event], ReplayMode::CriticalEvents).await;

        let updated = message_repo.get("server-outgoing-1").await.unwrap().unwrap();
        assert_eq!(updated.status, flare_proto::common::MessageStatus::Read as i32);
        assert!(updated.is_read);
    }
}

fn operation_seq(event: &flare_proto::common::Event) -> Option<u64> {
    event.event_seq.or(if event.seq > 0 { Some(event.seq) } else { None })
}
