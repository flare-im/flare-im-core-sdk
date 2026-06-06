use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use flare_proto::common::ack::Payload as AckPayload;
use flare_proto::common::data_packet::Payload as DataPacketPayload;
use flare_proto::common::event::Payload as EventPayload;
use flare_proto::common::message_content::Content as MessageContentPayload;
use flare_proto::common::send_ack::Result as SendAckResult;
use flare_proto::common::{
    Ack, CapabilityPacket, ConversationSummary, DataPacket, EventEnvelope, Message,
    MessageContent, MessagePush, MessageStatus, MessageType, SendAck, SendAckDurability,
    SyncRes, TextContent,
};
use prost::Message as ProstMessage;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{Mutex, RwLock, broadcast};
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, FlareError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidArgument,
    NotConnected,
    QueueFull,
    Protocol,
    Storage,
    Transport,
    Unsupported,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlareError {
    pub code: ErrorCode,
    pub message: String,
}

impl FlareError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgument, message)
    }

    pub fn not_connected() -> Self {
        Self::new(ErrorCode::NotConnected, "client is not connected")
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unsupported, message)
    }
}

impl fmt::Display for FlareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for FlareError {}

impl From<prost::DecodeError> for FlareError {
    fn from(value: prost::DecodeError) -> Self {
        Self::new(ErrorCode::Protocol, value.to_string())
    }
}

impl From<prost::EncodeError> for FlareError {
    fn from(value: prost::EncodeError) -> Self {
        Self::new(ErrorCode::Protocol, value.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SdkState {
    Disconnected,
    Connecting,
    Connected,
    Ready,
    Reconnecting,
}

impl Default for SdkState {
    fn default() -> Self {
        Self::Disconnected
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    WebSocket,
    Quic,
    Tcp,
    Custom,
}

impl Default for TransportKind {
    fn default() -> Self {
        Self::WebSocket
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkConfig {
    pub endpoint: String,
    pub tenant_id: Option<String>,
    pub user_id: String,
    pub device_id: String,
    pub access_token: String,
    pub transport: TransportKind,
    pub outbound_queue_capacity: usize,
    pub event_buffer_capacity: usize,
}

impl SdkConfig {
    pub fn builder() -> SdkConfigBuilder {
        SdkConfigBuilder::default()
    }

    pub fn validate(&self) -> Result<()> {
        if self.user_id.trim().is_empty() {
            return Err(FlareError::invalid_argument("user_id is required"));
        }
        if self.device_id.trim().is_empty() {
            return Err(FlareError::invalid_argument("device_id is required"));
        }
        if self.access_token.trim().is_empty() {
            return Err(FlareError::invalid_argument("access_token is required"));
        }
        if self.outbound_queue_capacity == 0 {
            return Err(FlareError::invalid_argument(
                "outbound_queue_capacity must be greater than zero",
            ));
        }
        Ok(())
    }
}

impl Default for SdkConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            tenant_id: None,
            user_id: String::new(),
            device_id: default_device_id(),
            access_token: String::new(),
            transport: TransportKind::default(),
            outbound_queue_capacity: 1024,
            event_buffer_capacity: 1024,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SdkConfigBuilder {
    config: SdkConfig,
}

impl SdkConfigBuilder {
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.config.endpoint = endpoint.into();
        self
    }

    pub fn tenant_id(mut self, tenant_id: impl Into<String>) -> Self {
        self.config.tenant_id = Some(tenant_id.into());
        self
    }

    pub fn user_id(mut self, user_id: impl Into<String>) -> Self {
        self.config.user_id = user_id.into();
        self
    }

    pub fn device_id(mut self, device_id: impl Into<String>) -> Self {
        self.config.device_id = device_id.into();
        self
    }

    pub fn access_token(mut self, access_token: impl Into<String>) -> Self {
        self.config.access_token = access_token.into();
        self
    }

    pub fn transport(mut self, transport: TransportKind) -> Self {
        self.config.transport = transport;
        self
    }

    pub fn outbound_queue_capacity(mut self, capacity: usize) -> Self {
        self.config.outbound_queue_capacity = capacity;
        self
    }

    pub fn event_buffer_capacity(mut self, capacity: usize) -> Self {
        self.config.event_buffer_capacity = capacity.max(16);
        self
    }

    pub fn build(self) -> Result<SdkConfig> {
        self.config.validate()?;
        Ok(self.config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextMessageRequest {
    pub conversation_id: String,
    pub text: String,
    #[serde(default)]
    pub conversation_type: i32,
    #[serde(default)]
    pub channel_id: String,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityPacketRequest {
    pub capability_id: String,
    pub packet_type: String,
    pub version: String,
    #[serde(default)]
    pub payload: Vec<u8>,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
    #[serde(default)]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImMessage {
    pub server_id: String,
    pub conversation_id: String,
    pub client_msg_id: String,
    pub sender_id: String,
    pub conversation_seq: u64,
    pub message_type: i32,
    pub status: i32,
    pub created_at: i64,
    pub text_preview: String,
    pub attributes: HashMap<String, String>,
}

impl From<&Message> for ImMessage {
    fn from(value: &Message) -> Self {
        Self {
            server_id: value.server_id.clone(),
            conversation_id: value.conversation_id.clone(),
            client_msg_id: value.client_msg_id.clone(),
            sender_id: value.sender_id.clone(),
            conversation_seq: value.conversation_seq,
            message_type: value.message_type,
            status: value.status,
            created_at: value.created_at,
            text_preview: text_preview(value),
            attributes: value.attributes.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub conversation_id: String,
    pub conversation_type: String,
    pub display_name: String,
    pub unread_count: u32,
    pub max_conversation_seq: u64,
    pub last_read_seq: u64,
    pub updated_at: i64,
    pub attributes: HashMap<String, String>,
}

impl From<&ConversationSummary> for Conversation {
    fn from(value: &ConversationSummary) -> Self {
        Self {
            conversation_id: value.conversation_id.clone(),
            conversation_type: value.conversation_type.clone(),
            display_name: value.display_name.clone(),
            unread_count: value.unread_count,
            max_conversation_seq: value.max_conversation_seq,
            last_read_seq: value.last_read_seq,
            updated_at: value.updated_at,
            attributes: value.attributes.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundPacket {
    pub id: String,
    pub kind: String,
    pub payload: Vec<u8>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SdkEvent {
    StateChanged { state: SdkState },
    MessageStored { message: ImMessage },
    SendAck { ack: SendAckView },
    SyncApplied { messages: usize, conversations: usize },
    CapabilityPacket { packet: CapabilityPacketView },
    Error { code: ErrorCode, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendAckView {
    pub client_msg_id: String,
    pub conversation_id: String,
    pub ack_id: Option<String>,
    pub accepted: Option<SendAcceptedView>,
    pub error_code: Option<i32>,
    pub error_message: Option<String>,
}

impl From<&SendAck> for SendAckView {
    fn from(value: &SendAck) -> Self {
        let (accepted, error_code, error_message) = match &value.result {
            Some(SendAckResult::Accepted(accepted)) => (
                Some(SendAcceptedView {
                    server_msg_id: accepted.server_msg_id.clone(),
                    conversation_seq: accepted.conversation_seq,
                    server_time: accepted.server_time,
                    durability: accepted.durability,
                }),
                None,
                None,
            ),
            Some(SendAckResult::Error(error)) => {
                (None, Some(error.code), Some(error.message.clone()))
            }
            None => (None, None, None),
        };
        Self {
            client_msg_id: value.client_msg_id.clone(),
            conversation_id: value.conversation_id.clone(),
            ack_id: value.ack_id.clone(),
            accepted,
            error_code,
            error_message,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendAcceptedView {
    pub server_msg_id: String,
    pub conversation_seq: u64,
    pub server_time: i64,
    pub durability: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityPacketView {
    pub capability_id: String,
    pub packet_type: String,
    pub version: String,
    pub payload: Vec<u8>,
    pub attributes: HashMap<String, String>,
    pub correlation_id: Option<String>,
}

impl From<&CapabilityPacket> for CapabilityPacketView {
    fn from(value: &CapabilityPacket) -> Self {
        Self {
            capability_id: value.capability_id.clone(),
            packet_type: value.packet_type.clone(),
            version: value.version.clone(),
            payload: value.payload.clone(),
            attributes: value.attributes.clone(),
            correlation_id: value.correlation_id.clone(),
        }
    }
}

pub type EventReceiver = broadcast::Receiver<SdkEvent>;

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<SdkEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity.max(16));
        Self { sender }
    }

    pub fn publish(&self, event: SdkEvent) {
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> EventReceiver {
        self.sender.subscribe()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalStoreSnapshot {
    pub conversations: Vec<Conversation>,
    pub messages: Vec<ImMessage>,
    pub pending: Vec<OutboundPacket>,
}

#[derive(Debug, Default)]
pub struct InMemoryStore {
    messages: RwLock<HashMap<String, Message>>,
    conversations: RwLock<HashMap<String, Conversation>>,
    outbound: Mutex<VecDeque<OutboundPacket>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    async fn save_message(&self, current_user_id: &str, message: Message) -> ImMessage {
        let view = ImMessage::from(&message);
        self.messages
            .write()
            .await
            .insert(message_key(&message), message.clone());
        self.upsert_conversation_from_message(current_user_id, &message)
            .await;
        view
    }

    async fn upsert_conversation_from_message(&self, current_user_id: &str, message: &Message) {
        let mut guard = self.conversations.write().await;
        let entry =
            guard
                .entry(message.conversation_id.clone())
                .or_insert_with(|| Conversation {
                    conversation_id: message.conversation_id.clone(),
                    conversation_type: conversation_type_label(message.conversation_type),
                    display_name: String::new(),
                    unread_count: 0,
                    max_conversation_seq: 0,
                    last_read_seq: 0,
                    updated_at: message.created_at,
                    attributes: HashMap::new(),
                });
        entry.max_conversation_seq = entry.max_conversation_seq.max(message.conversation_seq);
        entry.updated_at = entry.updated_at.max(message.created_at);
        if !current_user_id.is_empty()
            && message.sender_id != current_user_id
            && message.conversation_seq > entry.last_read_seq
        {
            entry.unread_count = entry.unread_count.saturating_add(1);
        }
    }

    async fn apply_send_ack(&self, ack: &SendAck) {
        let Some(SendAckResult::Accepted(accepted)) = &ack.result else {
            return;
        };

        let mut guard = self.messages.write().await;
        for message in guard.values_mut() {
            if message.client_msg_id == ack.client_msg_id
                && message.conversation_id == ack.conversation_id
            {
                message.server_id = accepted.server_msg_id.clone();
                message.conversation_seq = accepted.conversation_seq;
                message.created_at = accepted.server_time;
                message.status = if accepted.durability == SendAckDurability::Persisted as i32 {
                    MessageStatus::Persisted as i32
                } else {
                    MessageStatus::Sent as i32
                };
                break;
            }
        }
    }

    async fn save_conversation_summary(&self, summary: &ConversationSummary) {
        self.conversations
            .write()
            .await
            .insert(summary.conversation_id.clone(), Conversation::from(summary));
    }

    async fn push_outbound(&self, packet: OutboundPacket, capacity: usize) -> Result<()> {
        let mut guard = self.outbound.lock().await;
        if guard.len() >= capacity {
            return Err(FlareError::new(
                ErrorCode::QueueFull,
                "outbound queue capacity exceeded",
            ));
        }
        guard.push_back(packet);
        Ok(())
    }

    pub async fn drain_outbound(&self, limit: usize) -> Vec<OutboundPacket> {
        let mut guard = self.outbound.lock().await;
        let take = if limit == 0 { guard.len() } else { limit };
        let mut out = Vec::with_capacity(take.min(guard.len()));
        for _ in 0..take {
            let Some(packet) = guard.pop_front() else {
                break;
            };
            out.push(packet);
        }
        out
    }

    pub async fn list_messages(&self, conversation_id: Option<&str>) -> Vec<ImMessage> {
        let mut messages = self
            .messages
            .read()
            .await
            .values()
            .filter(|message| {
                conversation_id
                    .map(|id| message.conversation_id == id)
                    .unwrap_or(true)
            })
            .map(ImMessage::from)
            .collect::<Vec<_>>();
        messages.sort_by_key(|message| (message.conversation_id.clone(), message.conversation_seq));
        messages
    }

    pub async fn list_conversations(&self) -> Vec<Conversation> {
        let mut conversations = self
            .conversations
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        conversations.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        conversations
    }

    pub async fn snapshot(&self) -> LocalStoreSnapshot {
        LocalStoreSnapshot {
            conversations: self.list_conversations().await,
            messages: self.list_messages(None).await,
            pending: self.outbound.lock().await.iter().cloned().collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum DownlinkPayload {
    MessagePush(MessagePush),
    Ack(Ack),
    DataPacket(DataPacket),
    SyncResponse(SyncRes),
    EventEnvelope(EventEnvelope),
}

pub struct ProtocolCodec;

impl ProtocolCodec {
    pub fn encode_data_packet(packet: &DataPacket) -> Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(packet.encoded_len());
        packet.encode(&mut bytes)?;
        Ok(bytes)
    }

    pub fn encode_ack(ack: &Ack) -> Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(ack.encoded_len());
        ack.encode(&mut bytes)?;
        Ok(bytes)
    }

    pub fn decode_downlink(bytes: &[u8]) -> Result<DownlinkPayload> {
        if let Ok(packet) = DataPacket::decode(bytes)
            && packet.payload.is_some()
        {
            if let Some(DataPacketPayload::SyncResponse(sync)) = &packet.payload {
                return Ok(DownlinkPayload::SyncResponse(sync.clone()));
            }
            return Ok(DownlinkPayload::DataPacket(packet));
        }

        if let Ok(ack) = Ack::decode(bytes)
            && ack.payload.is_some()
        {
            return Ok(DownlinkPayload::Ack(ack));
        }

        if let Ok(push) = MessagePush::decode(bytes)
            && (!push.messages.is_empty() || !push.notifications.is_empty())
        {
            return Ok(DownlinkPayload::MessagePush(push));
        }

        if let Ok(envelope) = EventEnvelope::decode(bytes)
            && (!envelope.events.is_empty() || envelope.max_conversation_seq > 0)
        {
            return Ok(DownlinkPayload::EventEnvelope(envelope));
        }

        Err(FlareError::new(
            ErrorCode::Protocol,
            "unknown downlink protobuf payload",
        ))
    }
}

#[async_trait]
pub trait TransportPort: Send + Sync {
    async fn send(&self, packet: OutboundPacket) -> Result<()>;
}

struct ClientInner {
    config: SdkConfig,
    state: RwLock<SdkState>,
    store: Arc<InMemoryStore>,
    bus: EventBus,
    event_rx: Mutex<EventReceiver>,
    transport: RwLock<Option<Arc<dyn TransportPort>>>,
}

#[derive(Clone)]
pub struct IMClient {
    inner: Arc<ClientInner>,
}

impl IMClient {
    pub fn new(config: SdkConfig) -> Result<Self> {
        config.validate()?;
        let bus = EventBus::new(config.event_buffer_capacity);
        let event_rx = bus.subscribe();
        Ok(Self {
            inner: Arc::new(ClientInner {
                config,
                state: RwLock::new(SdkState::Disconnected),
                store: Arc::new(InMemoryStore::new()),
                bus,
                event_rx: Mutex::new(event_rx),
                transport: RwLock::new(None),
            }),
        })
    }

    pub fn from_config_json(config_json: &str) -> Result<Self> {
        let config = serde_json::from_str::<SdkConfig>(config_json)
            .map_err(|e| FlareError::invalid_argument(format!("invalid config json: {e}")))?;
        Self::new(config)
    }

    pub async fn install_transport(&self, transport: Arc<dyn TransportPort>) {
        *self.inner.transport.write().await = Some(transport);
    }

    pub fn config(&self) -> &SdkConfig {
        &self.inner.config
    }

    pub fn event_bus(&self) -> EventBus {
        self.inner.bus.clone()
    }

    pub fn store(&self) -> Arc<InMemoryStore> {
        self.inner.store.clone()
    }

    pub async fn state(&self) -> SdkState {
        *self.inner.state.read().await
    }

    pub async fn is_connected(&self) -> bool {
        matches!(self.state().await, SdkState::Connected | SdkState::Ready)
    }

    pub async fn connect(&self) -> Result<()> {
        self.set_state(SdkState::Connecting).await;
        self.set_state(SdkState::Connected).await;
        self.set_state(SdkState::Ready).await;
        Ok(())
    }

    pub async fn disconnect(&self) -> Result<()> {
        self.set_state(SdkState::Disconnected).await;
        Ok(())
    }

    pub async fn send_text_message(&self, request: TextMessageRequest) -> Result<ImMessage> {
        self.ensure_ready().await?;
        if request.conversation_id.trim().is_empty() {
            return Err(FlareError::invalid_argument("conversation_id is required"));
        }
        if request.text.trim().is_empty() {
            return Err(FlareError::invalid_argument("text is required"));
        }

        let message = self.build_text_message(request);
        let view = self
            .inner
            .store
            .save_message(&self.inner.config.user_id, message.clone())
            .await;
        self.inner.bus.publish(SdkEvent::MessageStored {
            message: view.clone(),
        });

        let data_packet = DataPacket {
            payload: Some(DataPacketPayload::UserCustom(flare_proto::common::CustomData {
                r#type: "im.message.send".to_string(),
                payload: message.encode_to_vec(),
                attributes: HashMap::new(),
            })),
        };
        self.enqueue_packet("message.send", ProtocolCodec::encode_data_packet(&data_packet)?)
            .await?;
        Ok(view)
    }

    pub async fn send_capability_packet(
        &self,
        request: CapabilityPacketRequest,
    ) -> Result<CapabilityPacketView> {
        self.ensure_ready().await?;
        if request.capability_id.trim().is_empty() {
            return Err(FlareError::invalid_argument("capability_id is required"));
        }
        if request.packet_type.trim().is_empty() {
            return Err(FlareError::invalid_argument("packet_type is required"));
        }

        let packet = CapabilityPacket {
            capability_id: request.capability_id,
            packet_type: request.packet_type,
            version: request.version,
            payload: request.payload,
            attributes: request.attributes,
            correlation_id: request.correlation_id,
        };
        let data_packet = DataPacket {
            payload: Some(DataPacketPayload::Capability(packet.clone())),
        };
        self.enqueue_packet(
            "capability.packet",
            ProtocolCodec::encode_data_packet(&data_packet)?,
        )
        .await?;
        let view = CapabilityPacketView::from(&packet);
        self.inner.bus.publish(SdkEvent::CapabilityPacket {
            packet: view.clone(),
        });
        Ok(view)
    }

    pub async fn ingest_downlink_bytes(&self, bytes: &[u8]) -> Result<usize> {
        let payload = ProtocolCodec::decode_downlink(bytes)?;
        self.ingest_downlink(payload).await
    }

    pub async fn ingest_downlink(&self, payload: DownlinkPayload) -> Result<usize> {
        match payload {
            DownlinkPayload::MessagePush(push) => {
                let mut applied = 0;
                for message in push.messages.into_iter().chain(push.notifications.into_iter()) {
                    let view = self
                        .inner
                        .store
                        .save_message(&self.inner.config.user_id, message)
                        .await;
                    self.inner.bus.publish(SdkEvent::MessageStored { message: view });
                    applied += 1;
                }
                Ok(applied)
            }
            DownlinkPayload::Ack(ack) => self.apply_ack(ack).await,
            DownlinkPayload::DataPacket(packet) => self.apply_data_packet(packet).await,
            DownlinkPayload::SyncResponse(sync) => self.apply_sync_response(sync).await,
            DownlinkPayload::EventEnvelope(envelope) => self.apply_event_envelope(envelope).await,
        }
    }

    pub async fn list_messages(&self, conversation_id: Option<&str>) -> Vec<ImMessage> {
        self.inner.store.list_messages(conversation_id).await
    }

    pub async fn list_conversations(&self) -> Vec<Conversation> {
        self.inner.store.list_conversations().await
    }

    pub async fn drain_outbound(&self, limit: usize) -> Vec<OutboundPacket> {
        self.inner.store.drain_outbound(limit).await
    }

    pub async fn snapshot(&self) -> LocalStoreSnapshot {
        self.inner.store.snapshot().await
    }

    pub async fn poll_event(&self) -> Option<SdkEvent> {
        let mut rx = self.inner.event_rx.lock().await;
        match rx.try_recv() {
            Ok(event) => Some(event),
            Err(broadcast::error::TryRecvError::Lagged(_)) => Some(SdkEvent::Error {
                code: ErrorCode::Internal,
                message: "event receiver lagged".to_string(),
            }),
            Err(broadcast::error::TryRecvError::Empty)
            | Err(broadcast::error::TryRecvError::Closed) => None,
        }
    }

    pub async fn poll_event_json(&self) -> Result<Option<String>> {
        self.poll_event()
            .await
            .map(|event| {
                serde_json::to_string(&event).map_err(|e| {
                    FlareError::new(ErrorCode::Internal, format!("event json encode failed: {e}"))
                })
            })
            .transpose()
    }

    pub async fn invoke_json_value(&self, route: &str, request: Value) -> Result<Value> {
        match route {
            "sdk.connect" => {
                self.connect().await?;
                Ok(json!({ "state": self.state().await }))
            }
            "sdk.disconnect" => {
                self.disconnect().await?;
                Ok(json!({ "state": self.state().await }))
            }
            "sdk.state" => Ok(json!({ "state": self.state().await })),
            "sdk.snapshot" => Ok(json!(self.snapshot().await)),
            "events.poll" => Ok(match self.poll_event().await {
                Some(event) => json!(event),
                None => Value::Null,
            }),
            "outbox.drain" => {
                let limit = request.get("limit").and_then(Value::as_u64).unwrap_or(0) as usize;
                Ok(json!(self.drain_outbound(limit).await))
            }
            "message.send_text" => {
                let req = serde_json::from_value::<TextMessageRequest>(request).map_err(|e| {
                    FlareError::invalid_argument(format!("invalid message.send_text request: {e}"))
                })?;
                Ok(json!(self.send_text_message(req).await?))
            }
            "message.list" => {
                let conversation_id = request
                    .get("conversation_id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                Ok(json!(self.list_messages(conversation_id.as_deref()).await))
            }
            "conversation.list" => Ok(json!(self.list_conversations().await)),
            "capability.send" | "capability.dispatch" => {
                let req = serde_json::from_value::<CapabilityPacketRequest>(request).map_err(|e| {
                    FlareError::invalid_argument(format!("invalid capability request: {e}"))
                })?;
                Ok(json!(self.send_capability_packet(req).await?))
            }
            _ => Err(FlareError::unsupported(format!("unsupported route: {route}"))),
        }
    }

    pub async fn invoke_json(&self, request_json: &str) -> Result<String> {
        let envelope = serde_json::from_str::<Value>(request_json).map_err(|e| {
            FlareError::invalid_argument(format!("invalid invoke json envelope: {e}"))
        })?;
        let route = envelope
            .get("route")
            .or_else(|| envelope.get("op"))
            .and_then(Value::as_str)
            .ok_or_else(|| FlareError::invalid_argument("invoke json requires route or op"))?;
        let params = envelope
            .get("params")
            .or_else(|| envelope.get("request"))
            .cloned()
            .unwrap_or(Value::Null);
        let value = self.invoke_json_value(route, params).await?;
        serde_json::to_string(&value)
            .map_err(|e| FlareError::new(ErrorCode::Internal, format!("json encode failed: {e}")))
    }

    pub fn generate_test_token(user_id: &str) -> String {
        generate_test_token(user_id)
    }

    async fn set_state(&self, state: SdkState) {
        *self.inner.state.write().await = state;
        self.inner.bus.publish(SdkEvent::StateChanged { state });
    }

    async fn ensure_ready(&self) -> Result<()> {
        if self.state().await == SdkState::Ready {
            Ok(())
        } else {
            Err(FlareError::not_connected())
        }
    }

    fn build_text_message(&self, request: TextMessageRequest) -> Message {
        let now = now_ms();
        Message {
            conversation_id: request.conversation_id.clone(),
            client_msg_id: flare_core::common::protocol::generate_message_id(),
            sender_id: self.inner.config.user_id.clone(),
            source: flare_proto::common::MessageSource::User as i32,
            created_at: now,
            conversation_type: request.conversation_type,
            message_type: MessageType::Text as i32,
            channel_id: if request.channel_id.is_empty() {
                request.conversation_id
            } else {
                request.channel_id
            },
            content: Some(MessageContent {
                content: Some(MessageContentPayload::Text(TextContent {
                    text: request.text,
                    mentions: Vec::new(),
                })),
            }),
            status: MessageStatus::Created as i32,
            attributes: request.attributes,
            ..Default::default()
        }
    }

    async fn enqueue_packet(&self, kind: &str, payload: Vec<u8>) -> Result<()> {
        let packet = OutboundPacket {
            id: Uuid::new_v4().to_string(),
            kind: kind.to_string(),
            payload,
            created_at: now_ms(),
        };
        self.inner
            .store
            .push_outbound(packet.clone(), self.inner.config.outbound_queue_capacity)
            .await?;

        let transport = self.inner.transport.read().await.clone();
        if let Some(transport) = transport {
            transport.send(packet).await?;
        }
        Ok(())
    }

    async fn apply_ack(&self, ack: Ack) -> Result<usize> {
        match ack.payload {
            Some(AckPayload::Send(send_ack)) => {
                self.inner.store.apply_send_ack(&send_ack).await;
                self.inner.bus.publish(SdkEvent::SendAck {
                    ack: SendAckView::from(&send_ack),
                });
                Ok(1)
            }
            Some(AckPayload::Batch(batch)) => {
                let mut applied = 0;
                for send_ack in batch.send_acks {
                    self.inner.store.apply_send_ack(&send_ack).await;
                    self.inner.bus.publish(SdkEvent::SendAck {
                        ack: SendAckView::from(&send_ack),
                    });
                    applied += 1;
                }
                Ok(applied)
            }
            _ => Ok(0),
        }
    }

    async fn apply_data_packet(&self, packet: DataPacket) -> Result<usize> {
        match packet.payload {
            Some(DataPacketPayload::SyncResponse(sync)) => self.apply_sync_response(sync).await,
            Some(DataPacketPayload::Capability(capability)) => {
                self.inner.bus.publish(SdkEvent::CapabilityPacket {
                    packet: CapabilityPacketView::from(&capability),
                });
                Ok(1)
            }
            _ => Ok(0),
        }
    }

    async fn apply_sync_response(&self, sync: SyncRes) -> Result<usize> {
        use flare_proto::common::sync_res::Payload;
        let mut messages = 0;
        let mut conversations = 0;
        match sync.payload {
            Some(Payload::SingleConversation(res)) => {
                for item in res.items {
                    if let Some(flare_proto::common::sync_slice_item::Payload::Message(message)) =
                        item.payload
                    {
                        let view = self
                            .inner
                            .store
                            .save_message(&self.inner.config.user_id, message)
                            .await;
                        self.inner.bus.publish(SdkEvent::MessageStored { message: view });
                        messages += 1;
                    }
                }
            }
            Some(Payload::Conversations(res)) => {
                for conversation in &res.conversations {
                    self.inner
                        .store
                        .save_conversation_summary(conversation)
                        .await;
                    conversations += 1;
                }
            }
            Some(Payload::ConversationsAll(res)) => {
                for conversation in &res.conversations {
                    self.inner
                        .store
                        .save_conversation_summary(conversation)
                        .await;
                    conversations += 1;
                }
            }
            Some(Payload::QueryEvents(res)) => {
                if let Some(envelope) = res.envelope {
                    messages += self.apply_event_envelope(envelope).await?;
                }
            }
            _ => {}
        }
        self.inner.bus.publish(SdkEvent::SyncApplied {
            messages,
            conversations,
        });
        Ok(messages + conversations)
    }

    async fn apply_event_envelope(&self, envelope: EventEnvelope) -> Result<usize> {
        let mut applied = 0;
        for event in envelope.events {
            if let Some(EventPayload::Message(message)) = event.payload {
                let view = self
                    .inner
                    .store
                    .save_message(&self.inner.config.user_id, message)
                    .await;
                self.inner.bus.publish(SdkEvent::MessageStored { message: view });
                applied += 1;
            }
        }
        Ok(applied)
    }
}

pub fn generate_test_token(user_id: &str) -> String {
    format!("test.{}.{}", user_id, Uuid::new_v4())
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn default_device_id() -> String {
    format!("device-{}", Uuid::new_v4())
}

fn message_key(message: &Message) -> String {
    if !message.server_id.is_empty() {
        format!("server:{}", message.server_id)
    } else if !message.client_msg_id.is_empty() {
        format!("client:{}", message.client_msg_id)
    } else {
        Uuid::new_v4().to_string()
    }
}

fn text_preview(message: &Message) -> String {
    match message.content.as_ref().and_then(|content| content.content.as_ref()) {
        Some(MessageContentPayload::Text(text)) => text.text.clone(),
        Some(MessageContentPayload::Notification(notification)) => notification.body.clone(),
        Some(MessageContentPayload::System(system)) => system.body.clone(),
        Some(MessageContentPayload::Custom(custom)) => custom.description.clone(),
        Some(MessageContentPayload::Placeholder(placeholder)) => placeholder.fallback_text.clone(),
        _ => String::new(),
    }
}

fn conversation_type_label(value: i32) -> String {
    match flare_proto::common::ConversationType::try_from(value).ok() {
        Some(flare_proto::common::ConversationType::Single) => "single",
        Some(flare_proto::common::ConversationType::Group) => "group",
        Some(flare_proto::common::ConversationType::Ai) => "ai",
        Some(flare_proto::common::ConversationType::System) => "system",
        Some(flare_proto::common::ConversationType::Customer) => "customer",
        Some(flare_proto::common::ConversationType::Temp) => "temp",
        _ => "unknown",
    }
    .to_string()
}
