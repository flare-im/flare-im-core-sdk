//! Router：下行载荷 → EventBus。与 flare-proto 对齐：消息=MessagePush，事件=Event/EventEnvelope，回执=SendAck；同步=`DataPacket.sync_response`→`SyncRes`，扩展=`DataPacket.user_custom`/`CustomData`。

use std::sync::Arc;

use flare_proto::common::event::Payload as EventPayload;
use flare_proto::common::MessageDeleteEvent;
use flare_proto::common::MessageStatus;
use prost::Message;
use tokio::sync::RwLock;
use tracing::warn;

use crate::application::conversation_projection_applier::ConversationProjectionApplier;
use crate::application::event_deduper::EventDeduper;
use crate::application::incoming_message_converger::IncomingMessageConverger;
use crate::application::message_deduper::MessageDeduper;
use crate::core::SyncResponseHandler;
use crate::event::{ConversationEvent, EventBus, ExtensionEvent, MessageEvent, SdkEvent};
use crate::infrastructure::protocol::DownlinkPayload;
use crate::model::IMMessage;
use crate::reliable_queue::ReliableSendQueue;
use crate::store::StoreProvider;

pub struct Dispatcher {
    bus: EventBus,
    reliable_queue: Option<Arc<ReliableSendQueue>>,
    sync_response_handler: Option<Arc<dyn SyncResponseHandler>>,
    /// 用于推送消息落库，使双端/查库与事件一致
    stores: Option<StoreProvider>,
    current_user_id: Arc<RwLock<String>>,
    event_deduper: EventDeduper,
    message_deduper: MessageDeduper,
    incoming_message_converger: Option<IncomingMessageConverger>,
    conversation_projection_applier: Option<ConversationProjectionApplier>,
}

impl Dispatcher {
    pub(crate) fn new(
        bus: EventBus,
        reliable_queue: Option<Arc<ReliableSendQueue>>,
        sync_response_handler: Option<Arc<dyn SyncResponseHandler>>,
        stores: Option<StoreProvider>,
        current_user_id: Arc<RwLock<String>>,
        event_deduper: EventDeduper,
        message_deduper: MessageDeduper,
    ) -> Self {
        let incoming_message_converger = stores.as_ref().map(|provider| {
            IncomingMessageConverger::new(
                provider.messages.clone(),
                bus.clone(),
                reliable_queue.clone(),
            )
        });
        let conversation_projection_applier = stores
            .as_ref()
            .map(|provider| ConversationProjectionApplier::new(provider.clone(), bus.clone()));
        Self {
            bus,
            reliable_queue,
            sync_response_handler,
            stores,
            current_user_id,
            event_deduper,
            message_deduper,
            incoming_message_converger,
            conversation_projection_applier,
        }
    }

    pub fn bus(&self) -> &EventBus {
        &self.bus
    }

    async fn should_apply_delete_for_current_user(&self, delete: &MessageDeleteEvent) -> bool {
        let scope = delete.scope.unwrap_or(1);
        if scope != 1 {
            return true;
        }

        let target_user_id = delete.target_user_id.as_deref().unwrap_or_default();
        if target_user_id.is_empty() {
            return true;
        }

        let current = self.current_user_id.read().await.clone();
        !current.is_empty() && current == target_user_id
    }

    pub async fn dispatch(
        &self,
        payload: DownlinkPayload,
    ) -> flare_core::common::error::Result<()> {
        match payload {
            DownlinkPayload::MessagePush(push) => {
                let mut all = Vec::new();
                all.extend(push.messages.clone());
                all.extend(push.notifications.clone());
                let mut messages = Vec::new();
                for message in all.into_iter().map(IMMessage::new) {
                    if self.message_deduper.record_if_new(&message).await {
                        messages.push(message);
                    }
                }
                let current_user_id = self.current_user_id.read().await.clone();
                if let Some(converger) = &self.incoming_message_converger {
                    messages = converger
                        .converge_messages(&current_user_id, messages)
                        .await
                        .map_err(|e| flare_core::common::error::FlareError::system(e.to_string()))?;
                }
                if !messages.is_empty() {
                    if let Some(ref stores) = self.stores {
                        if let Err(e) = stores.messages.save_batch(&messages).await {
                            warn!(error = %e, "MessagePush save_batch failed");
                        } else if let Some(applier) = &self.conversation_projection_applier {
                            if let Err(e) = applier.apply_messages(&messages, &current_user_id).await
                            {
                                warn!(error = %e, "MessagePush conversation projection failed");
                            }
                        }
                    }
                    if messages.len() == 1 {
                        self.bus.publish(SdkEvent::Message(MessageEvent::Received {
                            message: messages.into_iter().next().unwrap(),
                        }));
                    } else {
                        self.bus
                            .publish(SdkEvent::Message(MessageEvent::ReceivedBatch { messages }));
                    }
                }
            }
            DownlinkPayload::Event(ev) => {
                self.dispatch_single_event(&ev).await;
            }
            DownlinkPayload::EventEnvelope(env) => {
                tracing::info!(
                    event_count = env.events.len(),
                    "dispatch EventEnvelope (push/sync)"
                );
                for ev in &env.events {
                    self.dispatch_single_event(ev).await;
                }
            }
            DownlinkPayload::SendAck(ack) => {
                let mut ack = ack;
                if ack.conversation_id.trim().is_empty() {
                    if let Some(ref stores) = self.stores {
                        match stores.messages.get_by_client_msg_id(&ack.client_msg_id).await {
                            Ok(Some(local)) if !local.conversation_id.trim().is_empty() => {
                                ack.conversation_id = local.conversation_id.clone();
                            }
                            Ok(_) => {}
                            Err(e) => {
                                warn!(error = %e, client_msg_id = %ack.client_msg_id, "enrich send ack conversation_id failed");
                            }
                        }
                    }
                }
                if let Some(q) = &self.reliable_queue {
                    let _ = q.on_ack(ack.clone()).await;
                } else {
                    self.bus
                        .publish(SdkEvent::Message(MessageEvent::SendAck { ack }));
                }
            }
            DownlinkPayload::CustomData(data) => {
                self.bus.publish(SdkEvent::Extension(ExtensionEvent {
                    source: "custom".to_string(),
                    event_type: data.r#type.clone(),
                    payload: data.payload.clone(),
                }));
            }
            DownlinkPayload::SyncResp(resp) => {
                if let Some(h) = &self.sync_response_handler {
                    h.handle_sync_response(resp).await;
                }
            }
        }
        Ok(())
    }

    /// 分发单条 Event（从 EventEnvelope 或单条 Event 下行复用）
    async fn dispatch_single_event(&self, ev: &flare_proto::common::Event) {
        if !self.event_deduper.record_if_new(ev).await {
            return;
        }
        let mut messages: Vec<IMMessage> = Vec::new();
        if let Some(EventPayload::Message(m)) = &ev.payload {
            let message = IMMessage::new(m.clone());
            if self.message_deduper.record_if_new(&message).await {
                messages.push(message);
            }
        }
        let current_user_id = self.current_user_id.read().await.clone();
        if let Some(converger) = &self.incoming_message_converger {
            messages = converger
                .converge_messages(&current_user_id, messages)
                .await
                .unwrap_or_default();
        }
        if !messages.is_empty() {
            if let Some(ref stores) = self.stores {
                if let Err(e) = stores.messages.save_batch(&messages).await {
                    warn!(error = %e, "single event message save_batch failed");
                } else if let Some(applier) = &self.conversation_projection_applier {
                    let current_user_id = self.current_user_id.read().await.clone();
                    if let Err(e) = applier.apply_messages(&messages, &current_user_id).await {
                        warn!(error = %e, "single event conversation projection failed");
                    }
                }
            }
            for imm in messages {
                self.bus
                    .publish(SdkEvent::Message(MessageEvent::Received { message: imm }));
            }
        }
        let conv_id = ev.conversation_id.clone();
        if let Some(p) = &ev.payload {
            match p {
                EventPayload::Recall(recall) => {
                    if let Some(ref stores) = self.stores {
                        if let Err(e) = stores
                            .messages
                            .update_status(&recall.server_msg_id, MessageStatus::Recalled as i32)
                            .await
                        {
                            warn!(
                                error = %e,
                                server_msg_id = %recall.server_msg_id,
                                "Recall: update_status failed; UI refresh from DB may miss recalled state"
                            );
                        }
                    }
                    self.bus.publish(SdkEvent::Message(MessageEvent::Recalled {
                        conversation_id: conv_id,
                        event: recall.clone(),
                    }));
                }
                EventPayload::Edit(edit) => {
                    let mut should_publish = true;
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_edit_event(&edit.server_msg_id, edit.new_content.clone(), edit.edit_version)
                            .await
                        {
                            Ok(crate::domain::EditApplyResult::Applied) => {}
                            Ok(crate::domain::EditApplyResult::IgnoredStale) => {
                                should_publish = false;
                            }
                            Ok(crate::domain::EditApplyResult::NotFound) => {
                                warn!(
                                    server_msg_id = %edit.server_msg_id,
                                    "Event Edit: no local row matched; UI may refresh empty until sync"
                                );
                            }
                            Err(e) => {
                                warn!(error = %e, server_msg_id = %edit.server_msg_id, "Event Edit apply_edit_event failed");
                            }
                        }
                    }
                    if should_publish {
                        self.bus.publish(SdkEvent::Message(MessageEvent::Edited {
                            conversation_id: conv_id.clone(),
                            server_msg_id: edit.server_msg_id.clone(),
                            edit_version: Some(edit.edit_version),
                        }));
                    }
                }
                EventPayload::Reaction(reaction) => {
                    let mut should_publish = true;
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_reaction_event(
                                &conv_id,
                                &reaction.server_msg_id,
                                &reaction.user_id,
                                &reaction.emoji,
                                reaction.action,
                                operation_seq(ev),
                            )
                            .await
                        {
                            Ok(crate::domain::OperationApplyResult::IgnoredStale) => {
                                should_publish = false;
                            }
                            Ok(_) | Err(_) => {}
                        }
                    }
                    if should_publish {
                        self.bus.publish(SdkEvent::Message(MessageEvent::ReactionChanged {
                            conversation_id: conv_id,
                            server_msg_id: reaction.server_msg_id.clone(),
                            user_id: reaction.user_id.clone(),
                            emoji: reaction.emoji.clone(),
                            action: reaction.action,
                        }));
                    }
                }
                EventPayload::Delete(delete) => {
                    if self.should_apply_delete_for_current_user(delete).await {
                        let mut should_publish = true;
                        if let Some(ref stores) = self.stores {
                            match stores
                                .messages
                                .apply_delete_event(&delete.server_msg_id, operation_seq(ev))
                                .await
                            {
                                Ok(crate::domain::OperationApplyResult::IgnoredStale) => {
                                    should_publish = false;
                                }
                                Ok(_) | Err(_) => {}
                            }
                        }
                        if should_publish {
                            self.bus.publish(SdkEvent::Message(MessageEvent::Deleted {
                                conversation_id: conv_id.clone(),
                                event: delete.clone(),
                            }));
                            self.bus.publish(SdkEvent::Extension(ExtensionEvent {
                                source: "event".to_string(),
                                event_type: "message_delete".to_string(),
                                payload: delete.encode_to_vec(),
                            }));
                        }
                    }
                }
                EventPayload::Read(read) => {
                    if let Some(ref stores) = self.stores {
                        let current_user_id = self.current_user_id.read().await.clone();
                        // 对方已读回执：将「自己发送且 seq<=read_seq」的消息落库为已读，
                        // 避免仅靠前端内存态导致会话切换/重启后双对号丢失。
                        if !current_user_id.is_empty()
                            && !read.user_id.is_empty()
                            && read.user_id != current_user_id
                            && read.read_seq > 0
                        {
                            let _ = stores
                                .messages
                                .mark_outgoing_read_upto_seq(
                                    &conv_id,
                                    &current_user_id,
                                    read.read_seq,
                                )
                                .await;
                        }
                    }
                    self.bus.publish(SdkEvent::Message(MessageEvent::ReadReceipt {
                        conversation_id: conv_id,
                        event: read.clone(),
                    }));
                }
                EventPayload::Pin(pin) => {
                    let mut should_publish = true;
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_pin_event(&pin.server_msg_id, true, operation_seq(ev))
                            .await
                        {
                            Ok(crate::domain::OperationApplyResult::IgnoredStale) => {
                                should_publish = false;
                            }
                            Ok(_) | Err(_) => {}
                        }
                    }
                    if should_publish {
                        self.bus.publish(SdkEvent::Message(MessageEvent::Pinned {
                            conversation_id: conv_id,
                            event: pin.clone(),
                        }));
                    }
                }
                EventPayload::Unpin(unpin) => {
                    let mut should_publish = true;
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_pin_event(&unpin.server_msg_id, false, operation_seq(ev))
                            .await
                        {
                            Ok(crate::domain::OperationApplyResult::IgnoredStale) => {
                                should_publish = false;
                            }
                            Ok(_) | Err(_) => {}
                        }
                    }
                    if should_publish {
                        self.bus.publish(SdkEvent::Message(MessageEvent::Unpinned {
                            conversation_id: conv_id,
                            event: unpin.clone(),
                        }));
                    }
                }
                EventPayload::Mark(mark) => {
                    let mut should_publish = true;
                    if let Some(ref stores) = self.stores {
                        match stores
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
                                operation_seq(ev),
                            )
                            .await
                        {
                            Ok(crate::domain::OperationApplyResult::IgnoredStale) => {
                                should_publish = false;
                            }
                            Ok(_) | Err(_) => {}
                        }
                    }
                    if should_publish {
                        self.bus.publish(SdkEvent::Message(MessageEvent::Marked {
                            conversation_id: conv_id,
                            event: mark.clone(),
                        }));
                    }
                }
                EventPayload::Unmark(unmark) => {
                    let mut should_publish = true;
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_mark_event(
                                &unmark.server_msg_id,
                                unmark.mark_type,
                                None,
                                false,
                                operation_seq(ev),
                            )
                            .await
                        {
                            Ok(crate::domain::OperationApplyResult::IgnoredStale) => {
                                should_publish = false;
                            }
                            Ok(_) | Err(_) => {}
                        }
                    }
                    if should_publish {
                        self.bus.publish(SdkEvent::Message(MessageEvent::Unmarked {
                            conversation_id: conv_id,
                            event: unmark.clone(),
                        }));
                    }
                }
                EventPayload::Presence(presence) => {
                    self.bus
                        .publish(SdkEvent::Message(MessageEvent::PresenceChanged {
                            conversation_id: conv_id,
                            event: presence.clone(),
                        }));
                }
                EventPayload::CallSignal(call) => {
                    self.bus.publish(SdkEvent::Message(MessageEvent::CallSignal {
                        conversation_id: conv_id,
                        event: call.clone(),
                    }));
                }
                EventPayload::Custom(custom) => {
                    self.bus.publish(SdkEvent::Message(MessageEvent::Custom {
                        conversation_id: conv_id,
                        event: custom.clone(),
                    }));
                }
                EventPayload::Typing(typing) => {
                    self.bus.publish(SdkEvent::Message(MessageEvent::Typing {
                        conversation_id: conv_id,
                        event: typing.clone(),
                    }));
                }
                EventPayload::Conversation(_) => {
                    self.bus
                        .publish(SdkEvent::Conversation(ConversationEvent::Updated {
                            conversation_id: conv_id,
                        }));
                }
                EventPayload::ConversationDelete(_) => {
                    self.bus
                        .publish(SdkEvent::Conversation(ConversationEvent::Deleted {
                            conversation_id: conv_id,
                        }));
                }
                _ => {}
            }
        }
    }
}

fn operation_seq(event: &flare_proto::common::Event) -> Option<u64> {
    event.event_seq.or(if event.seq > 0 { Some(event.seq) } else { None })
}

#[cfg(test)]
mod tests {
    use super::Dispatcher;
    use crate::application::event_deduper::EventDeduper;
    use crate::application::message_deduper::MessageDeduper;
    use crate::application::usecases::SyncApplyUseCase;
    use crate::core::CurrentUserIdStore;
    use crate::domain::{
        ConversationReader, ConversationWriter, MessageReader, MessageStore, MessageWriter,
        PendingSendReader, PendingSendVo, PendingSendWriter, SyncCursorReader, SyncCursorVo,
        SyncCursorWriter,
    };
    use crate::event::{EventBus, MessageEvent, SdkEvent};
    use crate::infrastructure::protocol::DownlinkPayload;
    use crate::model::IMMessage;
    use crate::protocol::{Codec, PacketSender, ProtobufCodec};
    use crate::reliable_queue::ReliableSendQueue;
    use crate::store::StoreProvider;
    use crate::Result;
    use async_trait::async_trait;
    use flare_proto::common::{MessageDeleteEvent, SendAck};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::{Mutex, RwLock};
    use tokio::time::{Duration, timeout};

    struct MemoryPendingSendStore {
        data: Mutex<Vec<PendingSendVo>>,
    }

    impl MemoryPendingSendStore {
        fn new() -> Self {
            Self {
                data: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl PendingSendReader for MemoryPendingSendStore {
        async fn get(&self, client_msg_id: &str) -> Result<Option<PendingSendVo>> {
            let data = self.data.lock().await;
            Ok(data.iter().find(|entry| entry.client_msg_id == client_msg_id).cloned())
        }

        async fn list(&self) -> Result<Vec<PendingSendVo>> {
            Ok(self.data.lock().await.clone())
        }
    }

    #[async_trait]
    impl PendingSendWriter for MemoryPendingSendStore {
        async fn push(&self, entry: PendingSendVo) -> Result<()> {
            self.data.lock().await.push(entry);
            Ok(())
        }

        async fn pop(&self, client_msg_id: &str) -> Result<Option<PendingSendVo>> {
            let mut data = self.data.lock().await;
            let pos = data.iter().position(|entry| entry.client_msg_id == client_msg_id);
            Ok(pos.map(|index| data.remove(index)))
        }
    }

    struct MemoryMessageStore {
        data: RwLock<HashMap<String, IMMessage>>,
    }

    impl MemoryMessageStore {
        fn new() -> Self {
            Self {
                data: RwLock::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl MessageReader for MemoryMessageStore {
        async fn get(&self, message_id: &str) -> Result<Option<IMMessage>> {
            Ok(self.data.read().await.get(message_id).cloned())
        }

        async fn get_by_client_msg_id(&self, client_msg_id: &str) -> Result<Option<IMMessage>> {
            Ok(self
                .data
                .read()
                .await
                .values()
                .find(|message| message.client_msg_id == client_msg_id)
                .cloned())
        }

        async fn get_by_conversation(
            &self,
            _conversation_id: &str,
            _before_seq: u64,
            _limit: u32,
        ) -> Result<Vec<IMMessage>> {
            Ok(Vec::new())
        }

        async fn search(&self, _keyword: &str, _limit: u32) -> Result<Vec<IMMessage>> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl MessageWriter for MemoryMessageStore {
        async fn save_batch(&self, messages: &[IMMessage]) -> Result<()> {
            let mut data = self.data.write().await;
            for message in messages {
                let key = if !message.server_id.is_empty() {
                    message.server_id.clone()
                } else {
                    message.client_msg_id.clone()
                };
                data.insert(key, message.clone());
            }
            Ok(())
        }

        async fn save_one(&self, message: &IMMessage) -> Result<()> {
            self.save_batch(std::slice::from_ref(message)).await
        }

        async fn update_status(&self, message_id: &str, status: i32) -> Result<()> {
            if let Some(message) = self.data.write().await.get_mut(message_id) {
                message.status = status;
            }
            Ok(())
        }

        async fn update_content(&self, _message_id: &str, _new_content: Vec<u8>) -> Result<bool> {
            Ok(false)
        }

        async fn delete(&self, message_id: &str) -> Result<()> {
            self.data.write().await.remove(message_id);
            Ok(())
        }

        async fn update_after_ack(&self, client_msg_id: &str, message: &IMMessage) -> Result<()> {
            let mut data = self.data.write().await;
            data.remove(client_msg_id);
            data.insert(message.server_id.clone(), message.clone());
            Ok(())
        }
    }

    impl MessageStore for MemoryMessageStore {}

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
    async fn send_ack_is_published_once_when_reliable_queue_enabled() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let pending_store = Arc::new(MemoryPendingSendStore::new());
        let message_store = Arc::new(MemoryMessageStore::new());
        let sender = Arc::new(PacketSender::new(
            Arc::new(Mutex::new(None)),
            Arc::new(ProtobufCodec) as Arc<dyn Codec>,
        ));
        let reliable_queue = Arc::new(ReliableSendQueue::new(
            pending_store.clone(),
            pending_store,
            sender,
            message_store,
            current_user_id.clone(),
            bus.clone(),
            Some(60),
            Some(3),
        ));

        let dispatcher = Dispatcher::new(
            bus.clone(),
            Some(reliable_queue.clone()),
            None,
            None,
            current_user_id,
            EventDeduper::new(Some(64)),
            MessageDeduper::new(Some(64)),
        );

        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.client_msg_id = "client-1".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();
        reliable_queue.enqueue(message).await.unwrap();

        tokio::time::sleep(Duration::from_millis(20)).await;

        dispatcher
            .dispatch(DownlinkPayload::SendAck(SendAck {
                client_msg_id: "client-1".to_string(),
                server_msg_id: "server-1".to_string(),
                seq: 1,
                conversation_id: "conv-1".to_string(),
                success: true,
                ..Default::default()
            }))
            .await
            .unwrap();

        let first = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected one send ack event")
            .expect("bus closed");

        match first {
            SdkEvent::Message(MessageEvent::SendAck { ack }) => {
                assert_eq!(ack.client_msg_id, "client-1");
                assert_eq!(ack.server_msg_id, "server-1");
            }
            other => panic!("unexpected event: {other:?}"),
        }

        let second = timeout(Duration::from_millis(80), receiver.recv()).await;
        assert!(second.is_err(), "send ack should not be published twice");
    }

    #[tokio::test]
    async fn self_sent_realtime_message_converges_to_send_ack_without_received() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let pending_store = Arc::new(MemoryPendingSendStore::new());
        let message_store = Arc::new(MemoryMessageStore::new());
        let stores = StoreProvider {
            messages: message_store.clone(),
            conversations: Arc::new(NoopConversationStore),
            cursors: Arc::new(NoopSyncCursorStore),
            pending_send_reader: None,
            pending_send_writer: None,
            upload_manifest_store: None,
            media_cache_store: None,
            media_cache_admin: None,
            user_file_download_store: None,
            user_profiles_reader: None,
            user_profiles_writer: None,
        };
        let sender = Arc::new(PacketSender::new(
            Arc::new(Mutex::new(None)),
            Arc::new(ProtobufCodec) as Arc<dyn Codec>,
        ));
        let reliable_queue = Arc::new(ReliableSendQueue::new(
            pending_store.clone(),
            pending_store,
            sender,
            message_store.clone(),
            current_user_id.clone(),
            bus.clone(),
            Some(60),
            Some(3),
        ));

        let dispatcher = Dispatcher::new(
            bus.clone(),
            Some(reliable_queue.clone()),
            None,
            Some(stores),
            current_user_id,
            EventDeduper::new(Some(64)),
            MessageDeduper::new(Some(64)),
        );

        let mut optimistic = IMMessage::new(flare_proto::common::Message::default());
        optimistic.client_msg_id = "client-self-1".to_string();
        optimistic.conversation_id = "conv-1".to_string();
        optimistic.sender_id = "u1".to_string();
        reliable_queue.enqueue(optimistic).await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;

        let mut proto_message = flare_proto::common::Message::default();
        proto_message.server_id = "server-self-1".to_string();
        proto_message.client_msg_id = "client-self-1".to_string();
        proto_message.conversation_id = "conv-1".to_string();
        proto_message.sender_id = "u1".to_string();
        proto_message.seq = 11;

        dispatcher
            .dispatch(DownlinkPayload::MessagePush(flare_proto::common::MessagePush {
                messages: vec![proto_message],
                notifications: Vec::new(),
                ..Default::default()
            }))
            .await
            .unwrap();

        let first = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected convergence event")
            .expect("bus closed");
        match first {
            SdkEvent::Message(MessageEvent::SendAck { ack }) => {
                assert_eq!(ack.client_msg_id, "client-self-1");
                assert_eq!(ack.server_msg_id, "server-self-1");
            }
            other => panic!("unexpected event: {other:?}"),
        }

        let second = timeout(Duration::from_millis(80), receiver.recv()).await;
        assert!(second.is_err(), "self-sent convergence should suppress Received callback");
    }

    #[tokio::test]
    async fn out_of_order_ack_is_buffered_and_applied_once() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let pending_store = Arc::new(MemoryPendingSendStore::new());
        let message_store = Arc::new(MemoryMessageStore::new());
        let sender = Arc::new(PacketSender::new(
            Arc::new(Mutex::new(None)),
            Arc::new(ProtobufCodec) as Arc<dyn Codec>,
        ));
        let reliable_queue = Arc::new(ReliableSendQueue::new(
            pending_store.clone(),
            pending_store,
            sender,
            message_store,
            current_user_id.clone(),
            bus.clone(),
            Some(60),
            Some(3),
        ));
        let dispatcher = Dispatcher::new(
            bus.clone(),
            Some(reliable_queue.clone()),
            None,
            None,
            current_user_id,
            EventDeduper::new(Some(64)),
            MessageDeduper::new(Some(64)),
        );

        let mut message1 = IMMessage::new(flare_proto::common::Message::default());
        message1.client_msg_id = "client-1".to_string();
        message1.conversation_id = "conv-1".to_string();
        message1.sender_id = "u1".to_string();

        let mut message2 = IMMessage::new(flare_proto::common::Message::default());
        message2.client_msg_id = "client-2".to_string();
        message2.conversation_id = "conv-1".to_string();
        message2.sender_id = "u1".to_string();

        reliable_queue.enqueue(message1).await.unwrap();
        reliable_queue.enqueue(message2).await.unwrap();

        tokio::time::sleep(Duration::from_millis(20)).await;

        dispatcher
            .dispatch(DownlinkPayload::SendAck(SendAck {
                client_msg_id: "client-2".to_string(),
                server_msg_id: "server-2".to_string(),
                seq: 2,
                conversation_id: "conv-1".to_string(),
                success: true,
                ..Default::default()
            }))
            .await
            .unwrap();

        dispatcher
            .dispatch(DownlinkPayload::SendAck(SendAck {
                client_msg_id: "client-1".to_string(),
                server_msg_id: "server-1".to_string(),
                seq: 1,
                conversation_id: "conv-1".to_string(),
                success: true,
                ..Default::default()
            }))
            .await
            .unwrap();

        let first = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected first ack event")
            .expect("bus closed");
        let second = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected second ack event")
            .expect("bus closed");

        let mut ack_ids = Vec::new();
        for event in [first, second] {
            match event {
                SdkEvent::Message(MessageEvent::SendAck { ack }) => {
                    ack_ids.push(ack.client_msg_id);
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
        ack_ids.sort();
        assert_eq!(ack_ids, vec!["client-1".to_string(), "client-2".to_string()]);

        let third = timeout(Duration::from_millis(80), receiver.recv()).await;
        assert!(third.is_err(), "acks should not be published more than once");
    }

    #[tokio::test]
    async fn realtime_and_sync_replay_share_event_deduper() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let deduper = EventDeduper::new(Some(64));
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let message_store = Arc::new(MemoryMessageStore::new());
        let stores = StoreProvider {
            messages: message_store,
            conversations: Arc::new(NoopConversationStore),
            cursors: Arc::new(NoopSyncCursorStore),
            pending_send_reader: None,
            pending_send_writer: None,
            upload_manifest_store: None,
            media_cache_store: None,
            media_cache_admin: None,
            user_file_download_store: None,
            user_profiles_reader: None,
            user_profiles_writer: None,
        };
        let dispatcher = Dispatcher::new(
            bus.clone(),
            None,
            None,
            Some(stores.clone()),
            current_user_id,
            deduper.clone(),
            MessageDeduper::new(Some(64)),
        );
        let sync_apply = SyncApplyUseCase::new(
            stores,
            bus.clone(),
            deduper,
            MessageDeduper::new(Some(64)),
        );

        let mut event = flare_proto::common::Event::default();
        event.event_id = "evt-delete-1".to_string();
        event.conversation_id = "conv-1".to_string();
        event.payload = Some(flare_proto::common::event::Payload::Delete(MessageDeleteEvent {
            server_msg_id: "server-1".to_string(),
            scope: Some(2),
            ..Default::default()
        }));

        dispatcher
            .dispatch(DownlinkPayload::Event(event.clone()))
            .await
            .unwrap();
        sync_apply.apply_critical_events("u1", &[event]).await;

        let mut deleted_count = 0usize;
        let start = tokio::time::Instant::now();
        while start.elapsed() < Duration::from_millis(200) {
            match timeout(Duration::from_millis(30), receiver.recv()).await {
                Ok(Ok(SdkEvent::Message(MessageEvent::Deleted { event, .. }))) => {
                    assert_eq!(event.server_msg_id, "server-1");
                    deleted_count += 1;
                }
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert_eq!(deleted_count, 1, "duplicate replay should not emit second Deleted event");
    }

    #[tokio::test]
    async fn realtime_and_sync_message_replay_share_message_deduper() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let deduper = EventDeduper::new(Some(64));
        let message_deduper = MessageDeduper::new(Some(64));
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let stores = StoreProvider {
            messages: Arc::new(MemoryMessageStore::new()),
            conversations: Arc::new(NoopConversationStore),
            cursors: Arc::new(NoopSyncCursorStore),
            pending_send_reader: None,
            pending_send_writer: None,
            upload_manifest_store: None,
            media_cache_store: None,
            media_cache_admin: None,
            user_file_download_store: None,
            user_profiles_reader: None,
            user_profiles_writer: None,
        };
        let dispatcher = Dispatcher::new(
            bus.clone(),
            None,
            None,
            Some(stores.clone()),
            current_user_id,
            deduper.clone(),
            message_deduper.clone(),
        );
        let sync_apply = SyncApplyUseCase::new(stores, bus.clone(), deduper, message_deduper);

        let mut proto_message = flare_proto::common::Message::default();
        proto_message.server_id = "server-msg-1".to_string();
        proto_message.client_msg_id = "client-msg-1".to_string();
        proto_message.conversation_id = "conv-1".to_string();
        proto_message.sender_id = "u2".to_string();
        proto_message.seq = 10;

        dispatcher
            .dispatch(DownlinkPayload::MessagePush(flare_proto::common::MessagePush {
                messages: vec![proto_message.clone()],
                notifications: Vec::new(),
                ..Default::default()
            }))
            .await
            .unwrap();

        sync_apply
            .apply_single_conversation_page(
                "conv-1",
                "u1",
                0,
                &flare_proto::common::SingleConversationSyncRes {
                    conversation_id: "conv-1".to_string(),
                    items: vec![flare_proto::common::SyncSliceItem {
                        seq: 10,
                        created_at: None,
                        payload: prost::Message::encode_to_vec(&proto_message),
                    }],
                    max_seq: 10,
                    next_cursor: String::new(),
                    has_more: false,
                    hints: None,
                    stale: None,
                },
            )
            .await
            .unwrap();

        let mut received_count = 0usize;
        let start = tokio::time::Instant::now();
        while start.elapsed() < Duration::from_millis(200) {
            match timeout(Duration::from_millis(30), receiver.recv()).await {
                Ok(Ok(SdkEvent::Message(MessageEvent::Received { message }))) => {
                    assert_eq!(message.server_id, "server-msg-1");
                    received_count += 1;
                }
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert_eq!(received_count, 1, "duplicate replay should not emit second Received event");
    }
}
