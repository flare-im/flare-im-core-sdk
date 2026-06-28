use super::{
    Dispatcher, first_gap_after, first_internal_gap_after, first_internal_gap_after_from,
    is_waterline_ping, max_contiguous_seq,
};
use super::{
    SEQ_REPAIR_IDLE_TTL_MS, SEQ_REPAIR_MAX_BACKOFF_MS, SEQ_REPAIR_MAX_TRACKED_CONVERSATIONS,
    SeqRepairState, WATERLINE_ATTR_CONVERSATION_ID_CAMEL, WATERLINE_ATTR_KIND,
    WATERLINE_ATTR_MAX_SEQ_CAMEL, attr_non_empty, attr_u64, prune_seq_repair_state,
    seq_repair_backoff_ms,
};
use crate::application::notification::{NotificationHandlerRegistry, NotificationInboundPipeline};
use crate::application::services::EventDeduper;
use crate::application::services::MessageDeduper;
use crate::application::usecases::SyncApplyUseCase;
use crate::domain::{
    ConversationReader, ConversationWriter, MessageReader, MessageStore, MessageWriter,
    PendingSendReader, PendingSendVo, PendingSendWriter, SyncCursorReader, SyncCursorVo,
    SyncCursorWriter,
};
use crate::infrastructure::persistence::StoreProvider;
use crate::infrastructure::protocol::DownlinkPayload;
use crate::infrastructure::protocol::{Codec, PacketSender, ProtobufCodec};
use crate::kernel::event::{EventBus, MessageEvent, SdkEvent};
use crate::kernel::{CurrentUserIdStore, SessionSyncRunner};
use crate::model::IMMessage;
use crate::runtime::{ReliableSendQueue, ReliableSendQueueConfig};
use crate::shared::error::Result;
use crate::spi::metrics::MetricsRecorder;
use async_trait::async_trait;
use flare_proto::common::event::Payload as ProtoEventPayload;
use flare_proto::common::{
    ConversationUpdateEvent, Event, EventEnvelope, MessageDeleteEvent, MessageRecallEvent,
    SendAccepted, SendAck, SendAckDurability, send_ack,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify, RwLock};
use tokio::time::{Duration, timeout};

fn test_notification_pipeline(bus: EventBus) -> NotificationInboundPipeline {
    test_notification_pipeline_with_deduper(bus, MessageDeduper::new(Some(64)))
}

fn test_notification_pipeline_with_deduper(
    bus: EventBus,
    message_deduper: MessageDeduper,
) -> NotificationInboundPipeline {
    NotificationInboundPipeline::new(
        Arc::new(NotificationHandlerRegistry::new()),
        message_deduper,
        bus,
    )
}

fn accepted_ack(
    client_msg_id: &str,
    conversation_id: &str,
    server_msg_id: &str,
    conversation_seq: u64,
) -> SendAck {
    SendAck {
        client_msg_id: client_msg_id.to_string(),
        conversation_id: conversation_id.to_string(),
        result: Some(send_ack::Result::Accepted(SendAccepted {
            server_msg_id: server_msg_id.to_string(),
            conversation_seq,
            server_time: 0,
            durability: SendAckDurability::Persisted as i32,
        })),
        ..Default::default()
    }
}

fn accepted_server_msg_id(ack: &SendAck) -> Option<&str> {
    match ack.result.as_ref() {
        Some(send_ack::Result::Accepted(accepted)) => Some(accepted.server_msg_id.as_str()),
        _ => None,
    }
}

struct RecordingSessionSyncRunner {
    message_sync_calls: Mutex<Vec<String>>,
    notify: Notify,
}

impl RecordingSessionSyncRunner {
    fn new() -> Self {
        Self {
            message_sync_calls: Mutex::new(Vec::new()),
            notify: Notify::new(),
        }
    }

    async fn message_sync_calls(&self) -> Vec<String> {
        self.message_sync_calls.lock().await.clone()
    }
}

struct BlockingSessionSyncRunner {
    message_sync_calls: Mutex<Vec<String>>,
    notify: Notify,
    release_first: Notify,
}

impl BlockingSessionSyncRunner {
    fn new() -> Self {
        Self {
            message_sync_calls: Mutex::new(Vec::new()),
            notify: Notify::new(),
            release_first: Notify::new(),
        }
    }

    async fn message_sync_calls(&self) -> Vec<String> {
        self.message_sync_calls.lock().await.clone()
    }
}

impl SessionSyncRunner for BlockingSessionSyncRunner {
    fn request_message_sync(
        &self,
        conversation_id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let conversation_id = conversation_id.to_string();
        Box::pin(async move {
            let call_index = {
                let mut calls = self.message_sync_calls.lock().await;
                calls.push(conversation_id);
                calls.len()
            };
            self.notify.notify_one();
            if call_index == 1 {
                self.release_first.notified().await;
            }
            Ok(())
        })
    }

    fn request_message_sync_from_seq(
        &self,
        conversation_id: &str,
        last_seq: u64,
        _limit: i32,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let call = format!("{conversation_id}:{last_seq}");
        Box::pin(async move {
            self.message_sync_calls.lock().await.push(call);
            self.notify.notify_one();
            Ok(())
        })
    }

    fn send_read_ack(
        &self,
        _conversation_id: &str,
        _read_seq: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }

    fn request_participants_sync(
        &self,
        _conversation_id: &str,
        _limit: i32,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Vec<crate::model::ConversationParticipant>>>
                + Send
                + '_,
        >,
    > {
        Box::pin(async { Ok(Vec::new()) })
    }
}

impl SessionSyncRunner for RecordingSessionSyncRunner {
    fn request_message_sync(
        &self,
        conversation_id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let conversation_id = conversation_id.to_string();
        Box::pin(async move {
            self.message_sync_calls.lock().await.push(conversation_id);
            self.notify.notify_one();
            Ok(())
        })
    }

    fn request_message_sync_from_seq(
        &self,
        conversation_id: &str,
        last_seq: u64,
        _limit: i32,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let call = format!("{conversation_id}:{last_seq}");
        Box::pin(async move {
            self.message_sync_calls.lock().await.push(call);
            self.notify.notify_one();
            Ok(())
        })
    }

    fn send_read_ack(
        &self,
        _conversation_id: &str,
        _read_seq: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }

    fn request_participants_sync(
        &self,
        _conversation_id: &str,
        _limit: i32,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Vec<crate::model::ConversationParticipant>>>
                + Send
                + '_,
        >,
    > {
        Box::pin(async { Ok(Vec::new()) })
    }
}

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
        Ok(data
            .iter()
            .find(|entry| entry.client_msg_id == client_msg_id)
            .cloned())
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
        let pos = data
            .iter()
            .position(|entry| entry.client_msg_id == client_msg_id);
        Ok(pos.map(|index| data.remove(index)))
    }
}

struct MemoryMessageStore {
    data: RwLock<HashMap<String, IMMessage>>,
    fail_saves: bool,
}

impl MemoryMessageStore {
    fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
            fail_saves: false,
        }
    }

    /// 模拟持久化失败的消息存储：用于验证「先投递后持久化」在 save_batch 失败时仍投递视图。
    fn failing() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
            fail_saves: true,
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

    async fn search_in_conversation(
        &self,
        _conversation_id: &str,
        _keyword: &str,
        _limit: u32,
    ) -> Result<Vec<IMMessage>> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl MessageWriter for MemoryMessageStore {
    async fn save_batch(&self, messages: &[IMMessage]) -> Result<()> {
        if self.fail_saves {
            return Err(crate::shared::error::FlareError::general_error(
                "simulated persist failure",
            ));
        }
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

    async fn rewrite_conversation_id(
        &self,
        from_conversation_id: &str,
        to_conversation_id: &str,
    ) -> Result<u64> {
        let from = from_conversation_id.trim();
        let to = to_conversation_id.trim();
        if from.is_empty() || to.is_empty() || from == to {
            return Ok(0);
        }
        let mut count = 0;
        for message in self.data.write().await.values_mut() {
            if message.conversation_id == from {
                message.conversation_id = to.to_string();
                count += 1;
            }
        }
        Ok(count)
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
struct CursorAtSeqStore {
    last_seq: u64,
}

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

    async fn merge_conversation_identity(
        &self,
        _from_conversation_id: &str,
        _to_conversation_id: &str,
    ) -> Result<()> {
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

#[async_trait]
impl SyncCursorReader for CursorAtSeqStore {
    async fn get_conversation_cursor(
        &self,
        user_id: &str,
        conversation_id: &str,
    ) -> Result<Option<SyncCursorVo>> {
        Ok(Some(SyncCursorVo {
            user_id: user_id.to_string(),
            conversation_id: conversation_id.to_string(),
            last_seq: self.last_seq,
            synced_at: 0,
        }))
    }

    async fn get_raw(&self, _key: &str) -> Result<Option<String>> {
        Ok(None)
    }
}

#[async_trait]
impl SyncCursorWriter for CursorAtSeqStore {
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
    let reliable_queue = Arc::new(ReliableSendQueue::new(ReliableSendQueueConfig {
        pending_reader: pending_store.clone(),
        pending_writer: pending_store,
        sender,
        message_store,
        conversation_store: Arc::new(NoopConversationStore),
        current_user_id: current_user_id.clone(),
        bus: bus.clone(),
        timeout_secs: Some(60),
        max_retries: Some(3),
        max_in_flight: Some(32),
        metrics: MetricsRecorder::disabled(),
    }));

    let dispatcher = Dispatcher::new(
        bus.clone(),
        Some(reliable_queue.clone()),
        None,
        None,
        None,
        current_user_id,
        EventDeduper::new(Some(64)),
        test_notification_pipeline(bus.clone()),
        MetricsRecorder::disabled(),
    );

    let mut message = IMMessage::new(flare_proto::common::Message::default());
    message.client_msg_id = "client-1".to_string();
    message.conversation_id = "conv-1".to_string();
    message.sender_id = "u1".to_string();
    reliable_queue.enqueue(message).await.unwrap();

    let optimistic = timeout(Duration::from_millis(200), receiver.recv())
        .await
        .expect("expected optimistic message event")
        .expect("bus closed");
    assert!(matches!(
        optimistic,
        SdkEvent::Message(MessageEvent::ReceivedBatch { .. })
    ));

    tokio::time::sleep(Duration::from_millis(20)).await;

    dispatcher
        .dispatch(DownlinkPayload::SendAck(accepted_ack(
            "client-1", "conv-1", "server-1", 1,
        )))
        .await
        .unwrap();

    let first = timeout(Duration::from_millis(200), receiver.recv())
        .await
        .expect("expected one send ack event")
        .expect("bus closed");

    match first {
        SdkEvent::Message(MessageEvent::SendAck { ack }) => {
            assert_eq!(ack.client_msg_id, "client-1");
            assert_eq!(accepted_server_msg_id(&ack), Some("server-1"));
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
    };
    let sender = Arc::new(PacketSender::new(
        Arc::new(Mutex::new(None)),
        Arc::new(ProtobufCodec) as Arc<dyn Codec>,
    ));
    let reliable_queue = Arc::new(ReliableSendQueue::new(ReliableSendQueueConfig {
        pending_reader: pending_store.clone(),
        pending_writer: pending_store,
        sender,
        message_store: message_store.clone(),
        conversation_store: Arc::new(NoopConversationStore),
        current_user_id: current_user_id.clone(),
        bus: bus.clone(),
        timeout_secs: Some(60),
        max_retries: Some(3),
        max_in_flight: Some(32),
        metrics: MetricsRecorder::disabled(),
    }));

    let dispatcher = Dispatcher::new(
        bus.clone(),
        Some(reliable_queue.clone()),
        None,
        None,
        Some(stores),
        current_user_id,
        EventDeduper::new(Some(64)),
        test_notification_pipeline(bus.clone()),
        MetricsRecorder::disabled(),
    );

    let mut optimistic = IMMessage::new(flare_proto::common::Message::default());
    optimistic.client_msg_id = "client-self-1".to_string();
    optimistic.conversation_id = "conv-1".to_string();
    optimistic.sender_id = "u1".to_string();
    reliable_queue.enqueue(optimistic).await.unwrap();

    // enqueue surfaces the optimistic message via ReceivedBatch (asserted in
    // reliable_queue tests); drain it so the assertions below target the
    // self-echo convergence, not the optimistic insert.
    let enqueue_optimistic = timeout(Duration::from_millis(200), receiver.recv())
        .await
        .expect("expected optimistic message event")
        .expect("bus closed");
    assert!(matches!(
        enqueue_optimistic,
        SdkEvent::Message(MessageEvent::ReceivedBatch { .. })
    ));

    tokio::time::sleep(Duration::from_millis(20)).await;

    let proto_message = flare_proto::common::Message {
        server_id: "server-self-1".to_string(),
        client_msg_id: "client-self-1".to_string(),
        conversation_id: "conv-1".to_string(),
        sender_id: "u1".to_string(),
        conversation_seq: 11,
        ..Default::default()
    };

    dispatcher
        .dispatch(DownlinkPayload::MessagePush(
            flare_proto::common::MessagePush {
                messages: vec![proto_message],
                notifications: Vec::new(),
            },
        ))
        .await
        .unwrap();

    let first = timeout(Duration::from_millis(200), receiver.recv())
        .await
        .expect("expected convergence event")
        .expect("bus closed");
    match first {
        SdkEvent::Message(MessageEvent::SendAck { ack }) => {
            assert_eq!(ack.client_msg_id, "client-self-1");
            assert_eq!(accepted_server_msg_id(&ack), Some("server-self-1"));
        }
        other => panic!("unexpected event: {other:?}"),
    }

    let second = timeout(Duration::from_millis(80), receiver.recv()).await;
    assert!(
        second.is_err(),
        "self-sent convergence should suppress Received callback"
    );
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
    let reliable_queue = Arc::new(ReliableSendQueue::new(ReliableSendQueueConfig {
        pending_reader: pending_store.clone(),
        pending_writer: pending_store,
        sender,
        message_store,
        conversation_store: Arc::new(NoopConversationStore),
        current_user_id: current_user_id.clone(),
        bus: bus.clone(),
        timeout_secs: Some(60),
        max_retries: Some(3),
        max_in_flight: Some(32),
        metrics: MetricsRecorder::disabled(),
    }));
    let dispatcher = Dispatcher::new(
        bus.clone(),
        Some(reliable_queue.clone()),
        None,
        None,
        None,
        current_user_id,
        EventDeduper::new(Some(64)),
        test_notification_pipeline(bus.clone()),
        MetricsRecorder::disabled(),
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

    // Each enqueue surfaces its optimistic message via ReceivedBatch; drain
    // both so the assertions below target the buffered out-of-order acks.
    for _ in 0..2 {
        let optimistic = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected optimistic message event")
            .expect("bus closed");
        assert!(matches!(
            optimistic,
            SdkEvent::Message(MessageEvent::ReceivedBatch { .. })
        ));
    }

    tokio::time::sleep(Duration::from_millis(20)).await;

    dispatcher
        .dispatch(DownlinkPayload::SendAck(accepted_ack(
            "client-2", "conv-1", "server-2", 2,
        )))
        .await
        .unwrap();

    dispatcher
        .dispatch(DownlinkPayload::SendAck(accepted_ack(
            "client-1", "conv-1", "server-1", 1,
        )))
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
    assert_eq!(
        ack_ids,
        vec!["client-1".to_string(), "client-2".to_string()]
    );

    let third = timeout(Duration::from_millis(80), receiver.recv()).await;
    assert!(
        third.is_err(),
        "acks should not be published more than once"
    );
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
    };
    let dispatcher = Dispatcher::new(
        bus.clone(),
        None,
        None,
        None,
        Some(stores.clone()),
        current_user_id,
        deduper.clone(),
        test_notification_pipeline(bus.clone()),
        MetricsRecorder::disabled(),
    );
    let sync_apply = SyncApplyUseCase::new(
        stores,
        bus.clone(),
        deduper,
        test_notification_pipeline(bus.clone()),
    );

    let mut event = flare_proto::common::Event::default();
    event.event_id = "evt-delete-1".to_string();
    event.conversation_id = "conv-1".to_string();
    event.payload = Some(flare_proto::common::event::Payload::Delete(
        MessageDeleteEvent {
            server_msg_id: "server-1".to_string(),
            scope: Some(2),
            ..Default::default()
        },
    ));

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

    assert_eq!(
        deleted_count, 1,
        "duplicate replay should not emit second Deleted event"
    );
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
    };
    let notification_pipeline =
        test_notification_pipeline_with_deduper(bus.clone(), message_deduper.clone());
    let dispatcher = Dispatcher::new(
        bus.clone(),
        None,
        None,
        None,
        Some(stores.clone()),
        current_user_id,
        deduper.clone(),
        notification_pipeline.clone(),
        MetricsRecorder::disabled(),
    );
    let sync_apply = SyncApplyUseCase::new(stores, bus.clone(), deduper, notification_pipeline);

    let proto_message = flare_proto::common::Message {
        server_id: "server-msg-1".to_string(),
        client_msg_id: "client-msg-1".to_string(),
        conversation_id: "conv-1".to_string(),
        sender_id: "u2".to_string(),
        conversation_seq: 10,
        ..Default::default()
    };

    dispatcher
        .dispatch(DownlinkPayload::MessagePush(
            flare_proto::common::MessagePush {
                messages: vec![proto_message.clone()],
                notifications: Vec::new(),
            },
        ))
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
                    conversation_seq: 10,
                    created_at: 0,
                    payload: Some(flare_proto::common::sync_slice_item::Payload::Message(
                        proto_message.clone(),
                    )),
                }],
                max_conversation_seq: 10,
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

    assert_eq!(
        received_count, 1,
        "duplicate replay should not emit second Received event"
    );
}

#[tokio::test]
async fn message_push_delivers_to_view_even_when_persist_fails() {
    // A2/BX-02：先投递后持久化。save_batch 失败时消息仍应投递到视图（bus），
    // 由既有 seq-gap 补拉负责重取持久化，而不是从本批次丢弃。
    let bus = EventBus::new();
    let mut receiver = bus.subscribe_raw();
    let deduper = EventDeduper::new(Some(64));
    let message_deduper = MessageDeduper::new(Some(64));
    let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
    let stores = StoreProvider {
        messages: Arc::new(MemoryMessageStore::failing()),
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
    };
    let notification_pipeline =
        test_notification_pipeline_with_deduper(bus.clone(), message_deduper.clone());
    let dispatcher = Dispatcher::new(
        bus.clone(),
        None,
        None,
        None,
        Some(stores),
        current_user_id,
        deduper,
        notification_pipeline,
        MetricsRecorder::disabled(),
    );

    dispatcher
        .dispatch(DownlinkPayload::MessagePush(
            flare_proto::common::MessagePush {
                messages: vec![flare_proto::common::Message {
                    server_id: "server-fail-1".to_string(),
                    client_msg_id: "client-fail-1".to_string(),
                    conversation_id: "conv-1".to_string(),
                    sender_id: "u2".to_string(),
                    conversation_seq: 7,
                    ..Default::default()
                }],
                notifications: Vec::new(),
            },
        ))
        .await
        .unwrap();

    let mut received = false;
    let start = tokio::time::Instant::now();
    while start.elapsed() < Duration::from_millis(200) {
        match timeout(Duration::from_millis(30), receiver.recv()).await {
            Ok(Ok(SdkEvent::Message(MessageEvent::Received { message }))) => {
                assert_eq!(message.server_id, "server-fail-1");
                received = true;
                break;
            }
            Ok(Ok(_)) => {}
            _ => break,
        }
    }

    assert!(
        received,
        "save_batch 失败时消息仍须投递到视图（deliver-then-persist），不得从本批次丢弃"
    );
}

#[tokio::test]
async fn concurrent_message_push_dispatch_is_lossless_for_slow_subscriber() {
    const TASKS: usize = 8;
    const PER_TASK: usize = 250;
    const TOTAL: usize = TASKS * PER_TASK;

    let bus = EventBus::new();
    let mut receiver = bus.subscribe_raw();
    let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
    let message_store = Arc::new(MemoryMessageStore::new());
    let stores = StoreProvider {
        messages: message_store.clone(),
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
    };
    let dispatcher = Arc::new(Dispatcher::new(
        bus.clone(),
        None,
        None,
        None,
        Some(stores.clone()),
        current_user_id,
        EventDeduper::new(Some(TOTAL * 2)),
        test_notification_pipeline(bus.clone()),
        MetricsRecorder::disabled(),
    ));

    let mut tasks = Vec::with_capacity(TASKS);
    for task_id in 0..TASKS {
        let dispatcher = dispatcher.clone();
        tasks.push(tokio::spawn(async move {
            let mut messages = Vec::with_capacity(PER_TASK);
            for index in 0..PER_TASK {
                let seq = (task_id * PER_TASK + index + 1) as u64;
                messages.push(flare_proto::common::Message {
                    server_id: format!("server-{seq}"),
                    client_msg_id: format!("client-{seq}"),
                    conversation_id: format!("conv-{}", task_id % 4),
                    sender_id: "u2".to_string(),
                    conversation_seq: seq,
                    message_type: flare_proto::common::MessageType::Text as i32,
                    ..Default::default()
                });
            }
            dispatcher
                .dispatch(DownlinkPayload::MessagePush(
                    flare_proto::common::MessagePush {
                        messages,
                        notifications: Vec::new(),
                    },
                ))
                .await
                .expect("message push dispatch");
        }));
    }

    for task in tasks {
        task.await.expect("dispatch task panicked");
    }

    let mut received_server_ids = std::collections::HashSet::with_capacity(TOTAL);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while received_server_ids.len() < TOTAL {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "timed out after receiving {} of {TOTAL} messages",
            received_server_ids.len()
        );
        match timeout(remaining.min(Duration::from_millis(100)), receiver.recv()).await {
            Ok(Ok(SdkEvent::Message(MessageEvent::Received { message }))) => {
                assert!(
                    received_server_ids.insert(message.server_id.clone()),
                    "duplicate received message {}",
                    message.server_id
                );
            }
            Ok(Ok(_)) => {}
            Ok(Err(error)) => panic!("event receiver closed: {error:?}"),
            Err(_) => {}
        }
    }

    assert_eq!(
        message_store.data.read().await.len(),
        TOTAL,
        "all dispatched messages should be persisted exactly once"
    );
}

#[tokio::test]
async fn persist_worker_persists_asynchronously_when_started() {
    // A2 win#2：装载 worker 后，持久化由后台串行 worker 完成（最终一致），dispatch 不再内联 await 落盘。
    let bus = EventBus::new();
    let deduper = EventDeduper::new(Some(64));
    let message_deduper = MessageDeduper::new(Some(64));
    let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
    let message_store = Arc::new(MemoryMessageStore::new());
    let stores = StoreProvider {
        messages: message_store.clone(),
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
    };
    let notification_pipeline =
        test_notification_pipeline_with_deduper(bus.clone(), message_deduper.clone());
    let dispatcher = Arc::new(Dispatcher::new(
        bus.clone(),
        None,
        None,
        None,
        Some(stores),
        current_user_id,
        deduper,
        notification_pipeline,
        MetricsRecorder::disabled(),
    ));
    dispatcher.start_persist_worker();

    dispatcher
        .dispatch(DownlinkPayload::MessagePush(
            flare_proto::common::MessagePush {
                messages: vec![flare_proto::common::Message {
                    server_id: "server-worker-1".to_string(),
                    client_msg_id: "client-worker-1".to_string(),
                    conversation_id: "conv-1".to_string(),
                    sender_id: "u2".to_string(),
                    conversation_seq: 5,
                    ..Default::default()
                }],
                notifications: Vec::new(),
            },
        ))
        .await
        .unwrap();

    // worker 异步落盘 → 最终一致地轮询存储
    let mut persisted = false;
    let start = tokio::time::Instant::now();
    while start.elapsed() < Duration::from_millis(500) {
        if message_store
            .data
            .read()
            .await
            .contains_key("server-worker-1")
        {
            persisted = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        persisted,
        "启动 worker 后，消息应由后台串行 worker 异步持久化"
    );
}

/// A2 win#2 压测:启动 worker 后并发投递(超过 channel 容量 → 触发背压),
/// 断言**零丢失**(全部经后台串行 worker 落盘)、无死锁。
#[tokio::test]
async fn persist_worker_under_concurrent_dispatch_loses_nothing() {
    const TASKS: usize = 8;
    const PER_TASK: usize = 40; // 8×40=320 > 持久化 channel 容量 256 → 必经背压
    let total = TASKS * PER_TASK;

    let bus = EventBus::new();
    let deduper = EventDeduper::new(Some(2048));
    let message_deduper = MessageDeduper::new(Some(2048));
    let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
    let message_store = Arc::new(MemoryMessageStore::new());
    let stores = StoreProvider {
        messages: message_store.clone(),
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
    };
    let notification_pipeline =
        test_notification_pipeline_with_deduper(bus.clone(), message_deduper.clone());
    let dispatcher = Arc::new(Dispatcher::new(
        bus.clone(),
        None,
        None,
        None,
        Some(stores),
        current_user_id,
        deduper,
        notification_pipeline,
        MetricsRecorder::disabled(),
    ));
    dispatcher.start_persist_worker();

    let mut handles = Vec::with_capacity(TASKS);
    for t in 0..TASKS {
        let d = dispatcher.clone();
        handles.push(tokio::spawn(async move {
            for i in 0..PER_TASK {
                d.dispatch(DownlinkPayload::MessagePush(
                    flare_proto::common::MessagePush {
                        messages: vec![flare_proto::common::Message {
                            server_id: format!("srv-{t}-{i}"),
                            client_msg_id: format!("cli-{t}-{i}"),
                            conversation_id: format!("conv-{t}"),
                            sender_id: "u2".to_string(),
                            conversation_seq: (i as u64) + 1,
                            ..Default::default()
                        }],
                        notifications: Vec::new(),
                    },
                ))
                .await
                .unwrap();
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    // 持久化经后台 worker 异步完成 → 最终一致轮询:全部落盘、零丢失。
    let start = tokio::time::Instant::now();
    loop {
        let n = message_store.data.read().await.len();
        if n >= total {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "persist worker lost messages under concurrency: {n}/{total}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        message_store.data.read().await.len(),
        total,
        "all concurrently dispatched messages must be persisted exactly once via the worker"
    );
}

#[test]
fn seq_helpers_ignore_duplicates_and_find_contiguous_tail() {
    assert_eq!(max_contiguous_seq(10, &[12, 11, 11, 13]), 13);
    assert_eq!(max_contiguous_seq(10, &[12, 13]), 10);
    assert_eq!(first_gap_after(10, &[11, 13, 14]), Some(11));
    assert_eq!(first_gap_after(10, &[11, 12, 13]), None);
}

// ---- TEST-1: 生成式属性测试(seq-gap 辅助函数 = 游标推进/补拉正确性的支点)----
// 用确定性 LCG 造大量乱序+重复+空洞的输入,断言结构性不变量(独立于实现,非镜像)。

struct Lcg(u64);
impl Lcg {
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next_u64() % n }
    }
}

/// 小值域 → 强制出现空洞/重复/0;含 0 以覆盖「0 非真实 seq」的过滤。
fn gen_seqs(rng: &mut Lcg) -> Vec<u64> {
    let len = rng.below(12) as usize;
    let span = 1 + rng.below(10);
    (0..len).map(|_| rng.below(span + 1)).collect()
}

#[test]
fn prop_max_contiguous_seq_is_the_maximal_present_run_from_known() {
    let mut rng = Lcg(0x1234_5678_9abc_def0);
    for _ in 0..4000 {
        let seqs = gen_seqs(&mut rng);
        let known = rng.below(8);
        let set: std::collections::BTreeSet<u64> = seqs.iter().copied().collect();

        let m = max_contiguous_seq(known, &seqs);
        assert!(
            m >= known,
            "result below known: m={m} known={known} seqs={seqs:?}"
        );
        // (known, m] 全部在集合内(运行确实连续且都存在)。
        for i in (known + 1)..=m {
            assert!(
                set.contains(&i),
                "hole inside run at {i}: m={m} known={known} seqs={seqs:?}"
            );
        }
        // 运行停在第一个缺口:m+1 不在集合。
        assert!(
            !set.contains(&(m + 1)),
            "run must stop at first hole but m+1 present: m={m} known={known} seqs={seqs:?}"
        );
    }
}

#[test]
fn prop_first_gap_after_is_contiguous_tail_iff_data_beyond_hole() {
    let mut rng = Lcg(0xdead_beef_cafe_babe);
    for _ in 0..4000 {
        let seqs = gen_seqs(&mut rng);
        let known = rng.below(8);
        let set: std::collections::BTreeSet<u64> = seqs.iter().copied().collect();
        let m = max_contiguous_seq(known, &seqs);
        let has_later = set.iter().any(|s| *s > m + 1);

        match first_gap_after(known, &seqs) {
            Some(g) => {
                assert_eq!(
                    g, m,
                    "gap-after must equal the contiguous tail; seqs={seqs:?}"
                );
                assert!(
                    has_later,
                    "Some(gap) requires data beyond the hole; seqs={seqs:?}"
                );
            }
            None => assert!(
                !has_later,
                "None requires no data beyond the hole; seqs={seqs:?}"
            ),
        }
    }
}

#[test]
fn prop_first_internal_gap_from_matches_independent_reference() {
    let mut rng = Lcg(0x0f0f_0f0f_1357_9bdf);
    for _ in 0..4000 {
        let seqs = gen_seqs(&mut rng);
        let base = rng.below(8);
        // 实现内部对 >= base 后还会再滤掉 0(0 非真实 seq),参考须对齐:>= base 且 > 0。
        let mut t: Vec<u64> = seqs
            .iter()
            .copied()
            .filter(|s| *s >= base && *s > 0)
            .collect();
        t.sort_unstable();
        t.dedup();
        let expected = t.windows(2).find(|w| w[1] > w[0] + 1).map(|w| w[0]);
        assert_eq!(
            first_internal_gap_after_from(base, &seqs),
            expected,
            "base={base} seqs={seqs:?} t={t:?}"
        );
    }
}

#[test]
fn prop_seq_repair_backoff_is_monotonic_bounded_and_formula() {
    use super::{SEQ_REPAIR_BASE_BACKOFF_MS, SEQ_REPAIR_MAX_BACKOFF_MS};
    let mut prev = 0u64;
    for attempt in 0..40u32 {
        let b = seq_repair_backoff_ms(attempt);
        assert!(
            b >= SEQ_REPAIR_BASE_BACKOFF_MS,
            "below base at {attempt}: {b}"
        );
        assert!(
            b <= SEQ_REPAIR_MAX_BACKOFF_MS,
            "above cap at {attempt}: {b}"
        );
        assert!(
            b >= prev,
            "must be non-decreasing: at {attempt} {b} < {prev}"
        );
        prev = b;
    }
    assert_eq!(seq_repair_backoff_ms(0), SEQ_REPAIR_BASE_BACKOFF_MS);
    assert_eq!(seq_repair_backoff_ms(1), SEQ_REPAIR_BASE_BACKOFF_MS);
    assert_eq!(seq_repair_backoff_ms(2), SEQ_REPAIR_BASE_BACKOFF_MS * 2);
    assert_eq!(seq_repair_backoff_ms(100), SEQ_REPAIR_MAX_BACKOFF_MS);
}

#[test]
fn waterline_ping_matches_control_type_or_kind_attribute() {
    assert!(is_waterline_ping("sync.waterline", &HashMap::new()));
    assert!(is_waterline_ping(
        "custom",
        &HashMap::from([(
            WATERLINE_ATTR_KIND.to_string(),
            "conversation.waterline".to_string()
        )])
    ));
    assert!(!is_waterline_ping("typing", &HashMap::new()));
}

#[test]
fn waterline_attributes_parse_conversation_and_seq() {
    let attrs = HashMap::from([
        (
            WATERLINE_ATTR_CONVERSATION_ID_CAMEL.to_string(),
            " c1 ".to_string(),
        ),
        (WATERLINE_ATTR_MAX_SEQ_CAMEL.to_string(), "42".to_string()),
    ]);
    assert_eq!(
        attr_non_empty(&attrs, WATERLINE_ATTR_CONVERSATION_ID_CAMEL),
        Some("c1")
    );
    assert_eq!(attr_u64(&attrs, WATERLINE_ATTR_MAX_SEQ_CAMEL), Some(42));
}

#[tokio::test]
async fn empty_event_envelope_waterline_triggers_message_sync() {
    let bus = EventBus::new();
    let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
    let sync = Arc::new(RecordingSessionSyncRunner::new());
    let dispatcher = Dispatcher::new(
        bus.clone(),
        None,
        None,
        Some(sync.clone()),
        None,
        current_user_id,
        EventDeduper::new(Some(64)),
        test_notification_pipeline(bus),
        MetricsRecorder::disabled(),
    );

    dispatcher
        .dispatch(DownlinkPayload::EventEnvelope(EventEnvelope {
            conversation_id: "conversation-1".to_string(),
            max_conversation_seq: 2,
            ..Default::default()
        }))
        .await
        .unwrap();

    timeout(Duration::from_millis(200), sync.notify.notified())
        .await
        .expect("event envelope waterline should trigger message sync");
    assert_eq!(
        sync.message_sync_calls().await,
        vec!["conversation-1".to_string()]
    );
}

#[tokio::test]
async fn standalone_event_waterline_triggers_message_sync() {
    let bus = EventBus::new();
    let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
    let sync = Arc::new(RecordingSessionSyncRunner::new());
    let dispatcher = Dispatcher::new(
        bus.clone(),
        None,
        None,
        Some(sync.clone()),
        None,
        current_user_id,
        EventDeduper::new(Some(64)),
        test_notification_pipeline(bus),
        MetricsRecorder::disabled(),
    );

    dispatcher
        .dispatch(DownlinkPayload::Event(Event {
            conversation_id: "conversation-1".to_string(),
            conversation_seq: 101,
            ..Default::default()
        }))
        .await
        .unwrap();

    timeout(Duration::from_millis(200), sync.notify.notified())
        .await
        .expect("standalone event waterline should trigger message sync");
    assert_eq!(
        sync.message_sync_calls().await,
        vec!["conversation-1".to_string()]
    );
}

#[tokio::test]
async fn in_flight_waterline_ping_runs_follow_up_pull() {
    let bus = EventBus::new();
    let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
    let sync = Arc::new(BlockingSessionSyncRunner::new());
    let dispatcher = Dispatcher::new(
        bus.clone(),
        None,
        None,
        Some(sync.clone()),
        None,
        current_user_id,
        EventDeduper::new(Some(64)),
        test_notification_pipeline(bus),
        MetricsRecorder::disabled(),
    );

    dispatcher
        .maybe_trigger_waterline_pull(
            "test",
            "conversation.waterline",
            Some("conversation-1"),
            &HashMap::new(),
        )
        .await;
    timeout(Duration::from_millis(200), sync.notify.notified())
        .await
        .expect("first waterline pull should start");

    dispatcher
        .maybe_trigger_waterline_pull(
            "test",
            "conversation.waterline",
            Some("conversation-1"),
            &HashMap::new(),
        )
        .await;
    assert_eq!(
        sync.message_sync_calls().await,
        vec!["conversation-1".to_string()]
    );

    sync.release_first.notify_one();
    timeout(Duration::from_millis(200), sync.notify.notified())
        .await
        .expect("coalesced waterline ping should run a follow-up pull");
    assert_eq!(
        sync.message_sync_calls().await,
        vec!["conversation-1".to_string(), "conversation-1".to_string()]
    );
}

#[tokio::test]
async fn unread_conversation_update_without_seq_triggers_message_sync() {
    let bus = EventBus::new();
    let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
    let sync = Arc::new(RecordingSessionSyncRunner::new());
    let dispatcher = Dispatcher::new(
        bus.clone(),
        None,
        None,
        Some(sync.clone()),
        None,
        current_user_id,
        EventDeduper::new(Some(64)),
        test_notification_pipeline(bus),
        MetricsRecorder::disabled(),
    );

    dispatcher
        .dispatch(DownlinkPayload::Event(Event {
            conversation_id: "conversation-1".to_string(),
            conversation_seq: 0,
            payload: Some(ProtoEventPayload::Conversation(ConversationUpdateEvent {
                conversation_id: "conversation-1".to_string(),
                unread_count: 2,
                ..Default::default()
            })),
            ..Default::default()
        }))
        .await
        .unwrap();

    timeout(Duration::from_millis(200), sync.notify.notified())
        .await
        .expect("unread conversation update should trigger message sync");
    assert_eq!(
        sync.message_sync_calls().await,
        vec!["conversation-1".to_string()]
    );
}

#[tokio::test]
async fn unread_conversation_update_uses_typed_payload_conversation_id() {
    let bus = EventBus::new();
    let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
    let sync = Arc::new(RecordingSessionSyncRunner::new());
    let dispatcher = Dispatcher::new(
        bus.clone(),
        None,
        None,
        Some(sync.clone()),
        None,
        current_user_id,
        EventDeduper::new(Some(64)),
        test_notification_pipeline(bus),
        MetricsRecorder::disabled(),
    );

    dispatcher
        .dispatch(DownlinkPayload::Event(Event {
            conversation_id: String::new(),
            conversation_seq: 0,
            payload: Some(ProtoEventPayload::Conversation(ConversationUpdateEvent {
                conversation_id: "conversation-1".to_string(),
                unread_count: 2,
                ..Default::default()
            })),
            ..Default::default()
        }))
        .await
        .unwrap();

    timeout(Duration::from_millis(200), sync.notify.notified())
        .await
        .expect("typed conversation update should trigger message sync");
    assert_eq!(
        sync.message_sync_calls().await,
        vec!["conversation-1".to_string()]
    );
}

#[tokio::test]
async fn recall_event_without_outer_conversation_id_publishes_local_message_conversation_id() {
    let bus = EventBus::new();
    let mut receiver = bus.subscribe_raw();
    let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
    let message_store = Arc::new(MemoryMessageStore::new());
    let mut message = IMMessage::new(flare_proto::common::Message::default());
    message.server_id = "server-1".to_string();
    message.conversation_id = "conversation-1".to_string();
    message_store.save_one(&message).await.unwrap();
    let stores = StoreProvider {
        messages: message_store,
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
    };
    let dispatcher = Dispatcher::new(
        bus.clone(),
        None,
        None,
        None,
        Some(stores),
        current_user_id,
        EventDeduper::new(Some(64)),
        test_notification_pipeline(bus),
        MetricsRecorder::disabled(),
    );

    dispatcher
        .dispatch(DownlinkPayload::Event(Event {
            event_id: "recall-1".to_string(),
            conversation_id: String::new(),
            payload: Some(ProtoEventPayload::Recall(MessageRecallEvent {
                server_msg_id: "server-1".to_string(),
                ..Default::default()
            })),
            ..Default::default()
        }))
        .await
        .unwrap();

    let event = timeout(Duration::from_millis(200), receiver.recv())
        .await
        .expect("expected recall event")
        .expect("bus closed");
    match event {
        SdkEvent::Message(MessageEvent::Recalled {
            conversation_id, ..
        }) => {
            assert_eq!(conversation_id, "conversation-1");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[tokio::test]
async fn waterline_uses_materialized_message_seq_not_cursor_seq() {
    let bus = EventBus::new();
    let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
    let sync = Arc::new(RecordingSessionSyncRunner::new());
    let stores = StoreProvider {
        messages: Arc::new(MemoryMessageStore::new()),
        conversations: Arc::new(NoopConversationStore),
        conversation_participants: None,
        cursors: Arc::new(CursorAtSeqStore { last_seq: 2 }),
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
        Some(sync.clone()),
        Some(stores),
        current_user_id,
        EventDeduper::new(Some(64)),
        test_notification_pipeline(bus),
        MetricsRecorder::disabled(),
    );

    dispatcher
        .dispatch(DownlinkPayload::EventEnvelope(EventEnvelope {
            conversation_id: "conversation-1".to_string(),
            max_conversation_seq: 2,
            ..Default::default()
        }))
        .await
        .unwrap();

    timeout(Duration::from_millis(200), sync.notify.notified())
        .await
        .expect("waterline must pull when cursor reached but message rows are missing");
    assert_eq!(
        sync.message_sync_calls().await,
        vec!["conversation-1".to_string()]
    );
}

#[test]
fn internal_gap_detection_uses_local_window() {
    assert_eq!(first_internal_gap_after(&[590, 591, 596]), Some(591));
    assert_eq!(first_internal_gap_after(&[590, 591, 592]), None);
    assert_eq!(first_internal_gap_after(&[0, 590, 590, 591]), None);
}

#[test]
fn internal_gap_detection_ignores_historical_gaps_before_base() {
    assert_eq!(
        first_internal_gap_after_from(752, &[1, 2, 752, 766]),
        Some(752)
    );
    assert_eq!(
        first_internal_gap_after_from(771, &[1, 2, 766, 771, 777]),
        Some(771)
    );
    assert_eq!(first_internal_gap_after_from(752, &[1, 2, 752, 753]), None);
}

#[test]
fn seq_repair_backoff_is_exponential_and_capped() {
    assert_eq!(seq_repair_backoff_ms(1), 1_000);
    assert_eq!(seq_repair_backoff_ms(2), 2_000);
    assert_eq!(seq_repair_backoff_ms(3), 4_000);
    assert_eq!(seq_repair_backoff_ms(30), SEQ_REPAIR_MAX_BACKOFF_MS);
}

#[test]
fn seq_repair_state_prunes_expired_idle_entries() {
    let now = 20 * 60 * 1_000;
    let mut states = HashMap::from([
        (
            "expired".to_string(),
            SeqRepairState {
                updated_at_ms: now - SEQ_REPAIR_IDLE_TTL_MS - 1,
                next_attempt_at_ms: now - 1,
                ..Default::default()
            },
        ),
        (
            "backoff".to_string(),
            SeqRepairState {
                updated_at_ms: now - SEQ_REPAIR_IDLE_TTL_MS - 1,
                next_attempt_at_ms: now + 1_000,
                ..Default::default()
            },
        ),
        (
            "in-flight".to_string(),
            SeqRepairState {
                in_flight: true,
                updated_at_ms: now - SEQ_REPAIR_IDLE_TTL_MS - 1,
                ..Default::default()
            },
        ),
    ]);

    prune_seq_repair_state(&mut states, now);

    assert!(!states.contains_key("expired"));
    assert!(states.contains_key("backoff"));
    assert!(states.contains_key("in-flight"));
}

#[test]
fn seq_repair_state_trims_oldest_idle_entries_when_over_capacity() {
    let now = 1_000;
    let mut states = HashMap::with_capacity(SEQ_REPAIR_MAX_TRACKED_CONVERSATIONS + 2);
    for index in 0..(SEQ_REPAIR_MAX_TRACKED_CONVERSATIONS + 2) {
        states.insert(
            format!("conv-{index}"),
            SeqRepairState {
                updated_at_ms: now + index as u64,
                ..Default::default()
            },
        );
    }

    prune_seq_repair_state(&mut states, now);

    assert_eq!(states.len(), SEQ_REPAIR_MAX_TRACKED_CONVERSATIONS);
    assert!(!states.contains_key("conv-0"));
    assert!(!states.contains_key("conv-1"));
    assert!(states.contains_key("conv-2"));
}
