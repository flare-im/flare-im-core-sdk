use std::sync::Arc;
use std::time::Duration;

use flare_core::client::builder::flare::FlareClient;
use flare_core::common::protocol::builder::FrameBuilder;
use flare_core::common::protocol::flare::core::commands::{
    command::Type as CommandType, MessageCommand,
    message_command::Type as MsgType,
};
use flare_core::common::protocol::{Reliability, generate_message_id};
use tokio::sync::Mutex;

use crate::error::{SdkError, Result};
use crate::model::*;
use super::codec::Codec;

/// 发包器 — 将 ClientPacket 编码并通过 FlareClient 发送
pub struct PacketSender {
    client: Arc<Mutex<Option<FlareClient>>>,
    codec: Arc<dyn Codec>,
}

impl PacketSender {
    pub fn new(client: Arc<Mutex<Option<FlareClient>>>, codec: Arc<dyn Codec>) -> Self {
        Self { client, codec }
    }

    // ── 底层 ────────────────────────────────────────────────

    pub async fn request(&self, packet: ClientPacket, timeout: Duration) -> Result<ServerPacket> {
        let buf = self.codec.encode_client(&packet)?;
        let request_id = generate_message_id();

        let frame = build_frame(&request_id, buf);

        let guard = self.client.lock().await;
        let client = guard.as_ref().ok_or(SdkError::NotConnected)?;

        let resp = client
            .send_frame_and_wait(&frame, timeout)
            .await
            .map_err(|e| SdkError::SendFailed(e.to_string()))?;

        let payload = extract_payload(&resp)?;
        self.codec.decode_server(&payload)
    }

    pub async fn fire_and_forget(&self, packet: ClientPacket) -> Result<()> {
        let buf = self.codec.encode_client(&packet)?;
        let request_id = generate_message_id();

        let frame = build_frame(&request_id, buf);

        let guard = self.client.lock().await;
        let client = guard.as_ref().ok_or(SdkError::NotConnected)?;
        client.send_frame(&frame).await
            .map_err(|e| SdkError::SendFailed(e.to_string()))
    }

    // ── 业务便捷方法 ────────────────────────────────────────

    pub async fn send_message(
        &self,
        message: message::Message,
        timeout: Duration,
    ) -> Result<message::SendAck> {
        let packet = ClientPacket {
            payload: Some(client_packet::Payload::SendMessage(message)),
        };
        let sp = self.request(packet, timeout).await?;
        match sp.payload {
            Some(server_packet::Payload::SendAck(ack)) => Ok(ack),
            Some(server_packet::Payload::Error(e)) => Err(SdkError::Server { code: e.code, message: e.message }),
            _ => Err(SdkError::SendFailed("unexpected response".into())),
        }
    }

    pub async fn send_event(
        &self,
        event: event::Event,
        timeout: Duration,
    ) -> Result<message::OperationResponse> {
        let packet = ClientPacket {
            payload: Some(client_packet::Payload::SendEvent(event)),
        };
        let sp = self.request(packet, timeout).await?;
        match sp.payload {
            Some(server_packet::Payload::OperationResponse(r)) => {
                const ERROR_CODE_OK: i32 = 1;
                let ok = r
                    .status
                    .as_ref()
                    .map(|s| s.code == ERROR_CODE_OK)
                    .unwrap_or(false);
                if !ok {
                    let code_i32 = r.status.as_ref().map(|s| s.code).unwrap_or(0);
                    let message = r
                        .status
                        .as_ref()
                        .and_then(|s| if s.message.is_empty() { None } else { Some(s.message.clone()) })
                        .unwrap_or_else(|| format!("operation failed with code {}", code_i32));
                    return Err(SdkError::Server {
                        code: code_i32,
                        message,
                    });
                }
                Ok(r)
            }
            Some(server_packet::Payload::Error(e)) => Err(SdkError::Server { code: e.code, message: e.message }),
            _ => Err(SdkError::SendFailed("unexpected response".into())),
        }
    }

    pub async fn sync_conversations_all(
        &self,
        req: ConversationSyncAllRequest,
        timeout: Duration,
    ) -> Result<ConversationSyncAllResponse> {
        let packet = ClientPacket {
            payload: Some(client_packet::Payload::SyncConversationsAll(req)),
        };
        let sp = self.request(packet, timeout).await?;
        match sp.payload {
            Some(server_packet::Payload::SyncConversationsAllResp(r)) => Ok(r),
            Some(server_packet::Payload::Error(e)) => Err(SdkError::Server { code: e.code, message: e.message }),
            _ => Err(SdkError::SendFailed("unexpected response".into())),
        }
    }

    pub async fn sync_conversations(
        &self,
        req: SyncConversationsRequest,
        timeout: Duration,
    ) -> Result<SyncConversationsResponse> {
        let packet = ClientPacket {
            payload: Some(client_packet::Payload::SyncConversations(req)),
        };
        let sp = self.request(packet, timeout).await?;
        match sp.payload {
            Some(server_packet::Payload::SyncConversationsResp(r)) => Ok(r),
            Some(server_packet::Payload::Error(e)) => Err(SdkError::Server { code: e.code, message: e.message }),
            _ => Err(SdkError::SendFailed("unexpected response".into())),
        }
    }

    pub async fn sync_messages(
        &self,
        req: SyncRequest,
        timeout: Duration,
    ) -> Result<SyncResponse> {
        let packet = ClientPacket {
            payload: Some(client_packet::Payload::SyncRequest(req)),
        };
        let sp = self.request(packet, timeout).await?;
        match sp.payload {
            Some(server_packet::Payload::SyncResp(r)) => Ok(r),
            Some(server_packet::Payload::Error(e)) => Err(SdkError::Server { code: e.code, message: e.message }),
            _ => Err(SdkError::SendFailed("unexpected response".into())),
        }
    }

    pub async fn send_ack_batch(&self, batch: AckBatch) -> Result<()> {
        let packet = ClientPacket {
            payload: Some(client_packet::Payload::AckBatch(batch)),
        };
        self.fire_and_forget(packet).await
    }
}

fn build_frame(request_id: &str, payload: Vec<u8>) -> flare_core::common::protocol::Frame {
    let msg_cmd = MessageCommand {
        r#type: MsgType::Data as i32,
        message_id: request_id.to_string(),
        payload,
        metadata: Default::default(),
        seq: 0,
    };
    FrameBuilder::new()
        .with_command(flare_core::common::protocol::flare::core::commands::Command {
            r#type: Some(CommandType::Message(msg_cmd)),
        })
        .with_message_id(request_id.to_string())
        .with_reliability(Reliability::BestEffort)
        .build()
}

fn extract_payload(frame: &flare_core::common::protocol::Frame) -> Result<Vec<u8>> {
    let msg_cmd = frame.command.as_ref()
        .and_then(|c| c.r#type.as_ref())
        .and_then(|t| match t {
            CommandType::Message(m) => Some(m),
            _ => None,
        })
        .ok_or_else(|| SdkError::SendFailed("response not MessageCommand".into()))?;
    Ok(msg_cmd.payload.clone())
}
