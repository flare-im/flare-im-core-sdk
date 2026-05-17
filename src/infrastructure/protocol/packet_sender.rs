//! 上行发送：与 flare-proto 对齐 — 消息=Message，事件=Event，回执=Ack；DATA 载荷=`DataPacket`（`common/data.proto`）。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use flare_core::client::builder::flare::FlareClient;
use flare_core::common::protocol::{
    PayloadCommand, Reliability, builder, flare::core::commands::command::Type as CommandType,
    flare::core::commands::payload_command::Type as PayloadType,
};
use flare_proto::common::data_packet::Payload as DataPacketPayload;
use flare_proto::common::{
    Ack, CustomData, DataKind, DataPacket, Event, Message as ProtoMessage, SyncRes,
};
use prost::Message;
use tokio::sync::Mutex;

use crate::error::{FlareError, Result};
use crate::infrastructure::protocol::Codec;
use crate::util::system_time_to_prost_timestamp;

/// 上行包发送器 — Message / Event / Ack / DataPacket（同步与用户扩展）
pub struct PacketSender {
    client: Arc<Mutex<Option<FlareClient>>>,
}

impl PacketSender {
    pub fn new(client: Arc<Mutex<Option<FlareClient>>>, _codec: Arc<dyn Codec>) -> Self {
        Self { client }
    }

    /// 发送领域事件（event.proto Event）。PayloadCommand.type=Event，网关回 EventAck。
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
        if wire_event.created_at.is_none() {
            wire_event.created_at = Some(system_time_to_prost_timestamp());
        }
        let payload = wire_event.encode_to_vec();
        let mut guard = self.client.lock().await;
        let client = guard.as_mut().ok_or_else(|| {
            FlareError::localized(flare_core::common::ErrorCode::NotConnected, "未连接")
        })?;
        let cmd = builder::event_message(message_id, payload, None, None);
        let frame = builder::frame_with_payload_command(cmd, Reliability::AtLeastOnce);
        tokio::time::timeout(timeout_duration, client.send_frame(&frame))
            .await
            .map_err(|_| {
                FlareError::localized(
                    flare_core::common::ErrorCode::OperationTimeout,
                    "event send timeout",
                )
            })?
            .map_err(|e| FlareError::connection_failed(e.to_string()))?;
        Ok(())
    }

    /// 发送消息（message.proto Message）。PayloadCommand.type=Message，网关回 SendAck。
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
        let mut guard = self.client.lock().await;
        let client = guard.as_mut().ok_or_else(|| {
            FlareError::localized(flare_core::common::ErrorCode::NotConnected, "未连接")
        })?;
        // 发消息必须用 SEND (0)，网关才会走 handle_message 并回 ACK (1)；用 DATA (2) 会回 DATA (2)
        let msg_cmd =
            builder::send_message(message_id, message.encode_to_vec(), Some(metadata), None);
        let frame = builder::frame_with_payload_command(msg_cmd, Reliability::AtLeastOnce);
        tokio::time::timeout(timeout_duration, client.send_frame(&frame))
            .await
            .map_err(|_| {
                FlareError::localized(
                    flare_core::common::ErrorCode::OperationTimeout,
                    "message send timeout",
                )
            })?
            .map_err(|e| FlareError::connection_failed(e.to_string()))?;
        Ok(())
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
        let mut guard = self.client.lock().await;
        let client = guard.as_mut().ok_or_else(|| {
            FlareError::localized(flare_core::common::ErrorCode::NotConnected, "未连接")
        })?;
        client
            .send_frame(&frame)
            .await
            .map_err(|e| FlareError::connection_failed(e.to_string()))?;
        Ok(())
    }

    /// 发送用户扩展（`DataPacket` + `user_custom`）。PayloadCommand.type=Data。
    pub async fn send_custom_data(&self, data: &CustomData) -> Result<()> {
        let packet = DataPacket {
            kind: DataKind::UserCustom as i32,
            payload: Some(DataPacketPayload::UserCustom(data.clone())),
        };
        let payload = packet.encode_to_vec();
        let mut guard = self.client.lock().await;
        let client = guard.as_mut().ok_or_else(|| {
            FlareError::localized(flare_core::common::ErrorCode::NotConnected, "未连接")
        })?;
        let cmd = builder::data_message(builder::generate_message_id(), payload, None, None);
        let frame = builder::frame_with_payload_command(cmd, Reliability::AtLeastOnce);
        client
            .send_frame(&frame)
            .await
            .map_err(|e| FlareError::connection_failed(e.to_string()))?;
        Ok(())
    }

    /// 发送同步请求并等待 DATA 回包（网关对 DATA 为 request-response，须 `send_frame_and_wait`）。
    pub async fn send_sync_and_wait(
        &self,
        sync: &flare_proto::common::Sync,
        timeout: Duration,
    ) -> Result<SyncRes> {
        let packet = DataPacket {
            kind: DataKind::SyncRequest as i32,
            payload: Some(DataPacketPayload::SyncRequest(sync.clone())),
        };
        let payload = packet.encode_to_vec();
        let mut guard = self.client.lock().await;
        let client = guard.as_mut().ok_or_else(|| {
            FlareError::localized(flare_core::common::ErrorCode::NotConnected, "未连接")
        })?;
        let cmd = builder::data_message(builder::generate_message_id(), payload, None, None);
        let frame = builder::frame_with_payload_command(cmd, Reliability::AtLeastOnce);
        let response = client
            .send_frame_and_wait(&frame, timeout)
            .await
            .map_err(|e| FlareError::general_error(e.to_string()))?;
        decode_sync_response_frame(&response)
    }

    /// 发送同步请求（仅发不等；遗留路径，新代码请用 [`Self::send_sync_and_wait`]）。
    pub async fn send_sync(&self, sync: &flare_proto::common::Sync) -> Result<()> {
        let packet = DataPacket {
            kind: DataKind::SyncRequest as i32,
            payload: Some(DataPacketPayload::SyncRequest(sync.clone())),
        };
        let payload = packet.encode_to_vec();
        let mut guard = self.client.lock().await;
        let client = guard.as_mut().ok_or_else(|| {
            FlareError::localized(flare_core::common::ErrorCode::NotConnected, "未连接")
        })?;
        let cmd = builder::data_message(builder::generate_message_id(), payload, None, None);
        let frame = builder::frame_with_payload_command(cmd, Reliability::AtLeastOnce);
        client
            .send_frame(&frame)
            .await
            .map_err(|e| FlareError::connection_failed(e.to_string()))?;
        Ok(())
    }
}

fn decode_sync_response_frame(frame: &flare_core::common::protocol::Frame) -> Result<SyncRes> {
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
    match (packet.kind, packet.payload) {
        (k, Some(DataPacketPayload::SyncResponse(res))) if k == DataKind::SyncResponse as i32 => {
            Ok(res)
        }
        (k, Some(DataPacketPayload::UserCustom(data)))
            if k == DataKind::UserCustom as i32 && data.r#type == "error" =>
        {
            Err(FlareError::general_error(
                String::from_utf8_lossy(&data.payload).into_owned(),
            ))
        }
        _ => Err(FlareError::general_error(
            "unexpected sync response payload".to_string(),
        )),
    }
}
