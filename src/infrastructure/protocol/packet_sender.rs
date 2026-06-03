//! 上行发送：与 flare-proto 对齐 — 消息=Message，事件=Event，回执=Ack；DATA 载荷=`DataPacket`（`common/data.proto`）。

#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
use flare_core::client::builder::flare::FlareClient;
#[cfg(not(target_arch = "wasm32"))]
use flare_core::common::protocol::{
    PayloadCommand, Reliability, builder, flare::core::commands::command::Type as CommandType,
    flare::core::commands::payload_command::Type as PayloadType,
};
#[cfg(not(target_arch = "wasm32"))]
use flare_proto::common::data_packet::Payload as DataPacketPayload;
#[cfg(not(target_arch = "wasm32"))]
use flare_proto::common::{
    Ack, CustomData, DataKind, DataPacket, Event, Message as ProtoMessage, SyncRes,
};
#[cfg(target_arch = "wasm32")]
use flare_proto::common::{Ack, CustomData, Event, Message as ProtoMessage, SyncRes};
#[cfg(not(target_arch = "wasm32"))]
use prost::Message;
use tokio::sync::Mutex;

use crate::infrastructure::protocol::Codec;
use crate::shared::error::{FlareError, Result};
#[cfg(not(target_arch = "wasm32"))]
use crate::shared::util::system_time_to_prost_timestamp;

#[cfg(not(target_arch = "wasm32"))]
const CONTROL_SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// 上行包发送器 — Message / Event / Ack / DataPacket（同步与用户扩展）
#[cfg(not(target_arch = "wasm32"))]
pub struct PacketSender {
    client: Arc<Mutex<Option<FlareClient>>>,
    /// 串行化 `send_frame_and_wait`：`HybridClient` 按 message_id 匹配响应，不宜并发等待。
    rpc_wait: Arc<Mutex<()>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl PacketSender {
    pub fn new(client: Arc<Mutex<Option<FlareClient>>>, _codec: Arc<dyn Codec>) -> Self {
        Self {
            client,
            rpc_wait: Arc::new(Mutex::new(())),
        }
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
        tokio::time::timeout(CONTROL_SEND_TIMEOUT, client.send_frame(&frame))
            .await
            .map_err(|_| {
                FlareError::localized(
                    flare_core::common::ErrorCode::OperationTimeout,
                    "ack send timeout",
                )
            })?
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
        tokio::time::timeout(CONTROL_SEND_TIMEOUT, client.send_frame(&frame))
            .await
            .map_err(|_| {
                FlareError::localized(
                    flare_core::common::ErrorCode::OperationTimeout,
                    "custom data send timeout",
                )
            })?
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
        let cmd = builder::data_message(builder::generate_message_id(), payload, None, None);
        let frame = builder::frame_with_payload_command(cmd, Reliability::AtLeastOnce);
        // 多会话同步会并发调用本方法；用 rpc_wait 排队，禁止 take 走 client（否则并行方得到「未连接」）。
        let _rpc = self.rpc_wait.lock().await;
        let mut guard = self.client.lock().await;
        let client = guard.as_mut().ok_or_else(|| {
            FlareError::localized(flare_core::common::ErrorCode::NotConnected, "未连接")
        })?;
        let response = client
            .send_frame_and_wait(&frame, timeout)
            .await
            .map_err(|e| FlareError::general_error(e.to_string()))?;
        drop(guard);
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
        tokio::time::timeout(CONTROL_SEND_TIMEOUT, client.send_frame(&frame))
            .await
            .map_err(|_| {
                FlareError::localized(
                    flare_core::common::ErrorCode::OperationTimeout,
                    "sync send timeout",
                )
            })?
            .map_err(|e| FlareError::connection_failed(e.to_string()))?;
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
pub struct PacketSender {
    _codec: Arc<dyn Codec>,
    _rpc_wait: Arc<Mutex<()>>,
}

#[cfg(target_arch = "wasm32")]
impl PacketSender {
    pub fn new(_client: Arc<Mutex<Option<()>>>, codec: Arc<dyn Codec>) -> Self {
        Self {
            _codec: codec,
            _rpc_wait: Arc::new(Mutex::new(())),
        }
    }

    pub async fn send_event(&self, _event: &Event, _timeout_duration: Duration) -> Result<()> {
        Err(wasm_transport_unavailable("send_event"))
    }

    pub async fn send_message(
        &self,
        _message: &ProtoMessage,
        _timeout_duration: Duration,
    ) -> Result<()> {
        Err(wasm_transport_unavailable("send_message"))
    }

    pub async fn send_ack(&self, _ack: &Ack) -> Result<()> {
        Err(wasm_transport_unavailable("send_ack"))
    }

    pub async fn send_custom_data(&self, _data: &CustomData) -> Result<()> {
        Err(wasm_transport_unavailable("send_custom_data"))
    }

    pub async fn send_sync_and_wait(
        &self,
        _sync: &flare_proto::common::Sync,
        _timeout: Duration,
    ) -> Result<SyncRes> {
        Err(wasm_transport_unavailable("send_sync_and_wait"))
    }

    pub async fn send_sync(&self, _sync: &flare_proto::common::Sync) -> Result<()> {
        Err(wasm_transport_unavailable("send_sync"))
    }
}

#[cfg(target_arch = "wasm32")]
fn wasm_transport_unavailable(operation: &str) -> FlareError {
    FlareError::localized(
        flare_core::common::ErrorCode::OperationNotSupported,
        format!("{operation} requires a Web runtime transport adapter"),
    )
}

#[cfg(not(target_arch = "wasm32"))]
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
