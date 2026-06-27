//! 上行发送：与 flare-proto 对齐 — 消息=Message，事件=Event，回执=Ack；DATA 载荷=`DataPacket`（`common/data.proto`）。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use flare_core::client::builder::flare::FlareClient;
use flare_core::common::protocol::{
    Frame, PayloadCommand, Reliability, builder,
    flare::core::commands::command::Type as CommandType,
    flare::core::commands::payload_command::Type as PayloadType,
};
use flare_proto::common::data_packet::Payload as DataPacketPayload;
use flare_proto::common::{
    Ack, CapabilityPacket, CustomData, DataPacket, Event, Message as ProtoMessage,
    RealtimeControlPacket, SyncRes,
};
use prost::Message;
use tokio::sync::Mutex;

use crate::infrastructure::protocol::Codec;
use crate::shared::error::{FlareError, Result};
use crate::shared::util::{now_millis, timeout};

const CONTROL_SEND_TIMEOUT: Duration = Duration::from_secs(10);
const BEST_EFFORT_CONTROL_SEND_TIMEOUT: Duration = Duration::from_millis(250);

/// 上行包发送器 — Message / Event / Ack / DataPacket（同步与用户扩展）
pub struct PacketSender {
    client: Arc<Mutex<Option<FlareClient>>>,
    /// 串行化 `send_frame_and_wait`：`HybridClient` 按 message_id 匹配响应，不宜并发等待。
    rpc_wait: Arc<Mutex<()>>,
}

impl PacketSender {
    pub fn new(client: Arc<Mutex<Option<FlareClient>>>, _codec: Arc<dyn Codec>) -> Self {
        Self {
            client,
            rpc_wait: Arc::new(Mutex::new(())),
        }
    }

    async fn client_snapshot(&self) -> Result<FlareClient> {
        self.client.lock().await.as_ref().cloned().ok_or_else(|| {
            FlareError::localized(flare_core::common::ErrorCode::NotConnected, "未连接")
        })
    }

    async fn send_frame_with_timeout(
        &self,
        frame: &Frame,
        timeout_duration: Duration,
        timeout_message: &'static str,
    ) -> Result<()> {
        let client = self.client_snapshot().await?;
        timeout(timeout_duration, client.send_frame(frame))
            .await
            .map_err(|_| {
                FlareError::localized(
                    flare_core::common::ErrorCode::OperationTimeout,
                    timeout_message,
                )
            })?
            .map_err(|e| FlareError::connection_failed(e.to_string()))?;
        Ok(())
    }

    /// 发送领域事件（event.proto Event）。PayloadCommand.type=Event，网关回 EventAck。
    #[tracing::instrument(skip(self, event, timeout_duration), fields(event_id = %event.event_id))]
    pub async fn send_event(&self, event: &Event, timeout_duration: Duration) -> Result<()> {
        let message_id = if event.event_id.is_empty() {
            builder::generate_message_id()
        } else {
            event.event_id.clone()
        };
        // 与服务端编排对齐：event_id/request_id 为空时补齐，避免“帧发出但事件本体缺少关联 ID”导致的静默失败。
        let mut wire_event = event.clone();
        if wire_event.event_id.is_empty() {
            wire_event.event_id = message_id.clone();
        }
        if wire_event.request_id.is_none() {
            wire_event.request_id = Some(message_id.clone());
        }
        if wire_event.created_at <= 0 {
            wire_event.created_at = now_millis() as i64;
        }
        let payload = wire_event.encode_to_vec();
        let cmd = builder::event_message(message_id, payload, None, None);
        let frame = builder::frame_with_payload_command(cmd, Reliability::AtLeastOnce);
        self.send_frame_with_timeout(&frame, timeout_duration, "event send timeout")
            .await
    }

    /// 发送消息（message.proto Message）。PayloadCommand.type=Message，网关回 SendAck。
    #[tracing::instrument(skip(self, message, timeout_duration), fields(client_msg_id = %message.client_msg_id, conversation_id = %message.conversation_id))]
    pub async fn send_message(
        &self,
        message: &ProtoMessage,
        timeout_duration: Duration,
    ) -> Result<()> {
        let message_id = message.client_msg_id.clone();
        let mut metadata = HashMap::new();
        if !message.conversation_id.is_empty() {
            metadata.insert(
                "conversation_id".to_string(),
                message.conversation_id.as_bytes().to_vec(),
            );
        }
        tracing::debug!(message_id = %message_id, "send_message");
        // 发消息必须用 SEND (0)，网关才会走 handle_message 并回 ACK (1)；用 DATA (2) 会回 DATA (2)
        let msg_cmd =
            builder::send_message(message_id, message.encode_to_vec(), Some(metadata), None);
        let frame = builder::frame_with_payload_command(msg_cmd, Reliability::AtLeastOnce);
        self.send_frame_with_timeout(&frame, timeout_duration, "message send timeout")
            .await
    }

    /// 上报 ACK（ack.proto Ack：PushAck/ConversationAck/AckBatch）。PayloadCommand.type=Ack。
    pub async fn send_ack(&self, ack: &Ack) -> Result<()> {
        let message_id = builder::generate_message_id();
        let payload = ack.encode_to_vec();
        let cmd = PayloadCommand {
            r#type: PayloadType::Ack as i32,
            message_id: message_id.clone(),
            payload,
            metadata: std::collections::HashMap::new(),
            seq: 0,
        };
        let frame = builder::frame_with_payload_command(cmd, Reliability::AtLeastOnce);
        self.send_frame_with_timeout(&frame, CONTROL_SEND_TIMEOUT, "ack send timeout")
            .await
    }

    /// 发送用户扩展（`DataPacket` + `user_custom`）。PayloadCommand.type=Data。
    pub async fn send_custom_data(&self, data: &CustomData) -> Result<()> {
        let packet = DataPacket {
            payload: Some(DataPacketPayload::UserCustom(data.clone())),
        };
        let payload = packet.encode_to_vec();
        let cmd = builder::data_message(builder::generate_message_id(), payload, None, None);
        let frame = builder::frame_with_payload_command(cmd, Reliability::AtLeastOnce);
        self.send_frame_with_timeout(&frame, CONTROL_SEND_TIMEOUT, "custom data send timeout")
            .await
    }

    /// 发送能力包（`DataPacket` + `capability`）。PayloadCommand.type=Data；不占用 conversation_seq。
    pub async fn send_capability_packet(&self, packet: &CapabilityPacket) -> Result<()> {
        let data = DataPacket {
            payload: Some(DataPacketPayload::Capability(packet.clone())),
        };
        let payload = data.encode_to_vec();
        let cmd = builder::data_message(builder::generate_message_id(), payload, None, None);
        let frame = builder::frame_with_payload_command(cmd, Reliability::AtLeastOnce);
        self.send_frame_with_timeout(
            &frame,
            CONTROL_SEND_TIMEOUT,
            "capability packet send timeout",
        )
        .await
    }

    /// 发送实时控制包（`DataPacket` + `realtime_control`）。PayloadCommand.type=Data；不占用 conversation_seq。
    pub async fn send_realtime_control(&self, control: &RealtimeControlPacket) -> Result<()> {
        self.send_realtime_control_with_timeout(control, CONTROL_SEND_TIMEOUT)
            .await
    }

    /// 发送可丢弃的实时控制包。输入状态等瞬态提示不应阻塞消息发送主链路。
    pub async fn send_realtime_control_best_effort(
        &self,
        control: &RealtimeControlPacket,
    ) -> Result<()> {
        if let Err(error) = self
            .send_realtime_control_with_timeout(control, BEST_EFFORT_CONTROL_SEND_TIMEOUT)
            .await
        {
            tracing::debug!(
                error = %error,
                control_type = %control.control_type,
                "dropped best-effort realtime control packet"
            );
        }
        Ok(())
    }

    async fn send_realtime_control_with_timeout(
        &self,
        control: &RealtimeControlPacket,
        timeout_duration: Duration,
    ) -> Result<()> {
        let data = DataPacket {
            payload: Some(DataPacketPayload::RealtimeControl(control.clone())),
        };
        let payload = data.encode_to_vec();
        let cmd = builder::data_message(builder::generate_message_id(), payload, None, None);
        let frame = builder::frame_with_payload_command(cmd, Reliability::AtLeastOnce);
        self.send_frame_with_timeout(&frame, timeout_duration, "realtime control send timeout")
            .await
    }

    /// 发送同步请求并等待 DATA 回包（网关对 DATA 为 request-response，须 `send_frame_and_wait`）。
    pub async fn send_sync_and_wait(
        &self,
        sync: &flare_proto::common::Sync,
        timeout: Duration,
    ) -> Result<SyncRes> {
        let packet = DataPacket {
            payload: Some(DataPacketPayload::SyncRequest(sync.clone())),
        };
        let payload = packet.encode_to_vec();
        let cmd = builder::data_message(builder::generate_message_id(), payload, None, None);
        let frame = builder::frame_with_payload_command(cmd, Reliability::AtLeastOnce);
        // 多会话同步会并发调用本方法；用 rpc_wait 排队，禁止 take 走 client（否则并行方得到「未连接」）。
        let _rpc = self.rpc_wait.lock().await;
        let client = self.client_snapshot().await?;
        let response = client
            .send_frame_and_wait(&frame, timeout)
            .await
            .map_err(|e| FlareError::general_error(e.to_string()))?;
        decode_sync_response_frame(&response)
    }

    /// 发送同步请求（仅发不等；遗留路径，新代码请用 [`Self::send_sync_and_wait`]）。
    pub async fn send_sync(&self, sync: &flare_proto::common::Sync) -> Result<()> {
        let packet = DataPacket {
            payload: Some(DataPacketPayload::SyncRequest(sync.clone())),
        };
        let payload = packet.encode_to_vec();
        let cmd = builder::data_message(builder::generate_message_id(), payload, None, None);
        let frame = builder::frame_with_payload_command(cmd, Reliability::AtLeastOnce);
        self.send_frame_with_timeout(&frame, CONTROL_SEND_TIMEOUT, "sync send timeout")
            .await
    }
}

fn decode_sync_response_frame(frame: &Frame) -> Result<SyncRes> {
    let payload = frame
        .command
        .as_ref()
        .and_then(|cmd| match &cmd.r#type {
            Some(CommandType::Payload(pc)) => Some(pc.payload.as_slice()),
            _ => None,
        })
        .ok_or_else(|| {
            FlareError::deserialization_error(
                "sync response frame has no payload command".to_string(),
            )
        })?;
    let packet = DataPacket::decode(payload).map_err(|e| {
        FlareError::deserialization_error(format!("decode sync DataPacket response: {e}"))
    })?;
    match packet.payload {
        Some(DataPacketPayload::SyncResponse(res)) => Ok(res),
        Some(DataPacketPayload::UserCustom(data)) if data.r#type == "error" => Err(
            FlareError::general_error(String::from_utf8_lossy(&data.payload).into_owned()),
        ),
        _ => Err(FlareError::general_error(
            "unexpected sync response payload".to_string(),
        )),
    }
}
