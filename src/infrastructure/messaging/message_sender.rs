use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use anyhow::{Result, Context};
use flare_core::common::protocol::{Reliability, MessageCommand};
use flare_core::common::protocol::flare::core::commands::command::Type as CommandType;
use flare_core::common::protocol::builder::*;
use flare_proto::common::{
    ClientPacket, ServerPacket,
    ConversationSyncAllRequest, ConversationSyncAllResponse,
    SyncConversationsRequest, SyncConversationsResponse,
    SyncRequest, SyncResponse,
};
use prost::Message as ProstMessage;

use crate::domain::message::Message;
use crate::infrastructure::network::NetworkClient;
use crate::infrastructure::converter::MessageConverter;
use crate::application::ports::sync_transport::SyncTransport;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct SendMessageResult {
    pub server_msg_id: String,
    pub seq: u64,
    pub success: bool,
    pub error_code: i32,
    pub error_message: String,
}

pub struct MessageSender {
    network: Arc<Mutex<Option<NetworkClient>>>,
}

impl MessageSender {
    pub fn new(network: Arc<Mutex<Option<NetworkClient>>>) -> Self {
        Self { network }
    }

    /// 发送 ClientPacket 并等待 ServerPacket 响应
    async fn send_client_packet_and_wait<T, R>(
        &self,
        request: T,
        wrap_payload: fn(T) -> flare_proto::common::client_packet::Payload,
        unwrap_payload: fn(flare_proto::common::server_packet::Payload) -> Option<R>,
        timeout: Duration,
    ) -> Result<R>
    where
        T: ProstMessage,
        R: ProstMessage + Default,
    {
        let client_packet = ClientPacket {
            payload: Some(wrap_payload(request)),
        };

        let server_packet = self.send_packet_and_wait(client_packet, timeout).await?;
        let payload = server_packet
            .payload
            .ok_or_else(|| anyhow::anyhow!("ServerPacket.payload is None"))?;

        match payload {
            flare_proto::common::server_packet::Payload::Error(err_pkt) => Err(anyhow::anyhow!(
                "Server error (code={}): {}",
                err_pkt.code,
                err_pkt.message
            )),
            other => unwrap_payload(other)
                .ok_or_else(|| anyhow::anyhow!("ServerPacket payload mismatch")),
        }
    }

    async fn send_packet_and_wait(
        &self,
        packet: ClientPacket,
        timeout: Duration,
    ) -> Result<ServerPacket> {
        let mut packet_bytes = Vec::new();
        packet
            .encode(&mut packet_bytes)
            .context("Failed to encode ClientPacket")?;

        let request_id = flare_core::common::protocol::generate_message_id();
        let msg_cmd = MessageCommand {
            r#type: flare_core::common::protocol::flare::core::commands::message_command::Type::Data
                as i32,
            message_id: request_id.clone(),
            payload: packet_bytes,
            metadata: Default::default(),
            seq: 0,
        };

        let frame = FrameBuilder::new()
            .with_command(flare_core::common::protocol::flare::core::commands::Command {
                r#type: Some(CommandType::Message(msg_cmd)),
            })
            .with_message_id(request_id.clone())
            .with_reliability(Reliability::BestEffort)
            .build();

        let network_guard = self.network.lock().await;
        let client = network_guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Network client is not connected"))?;

        if !client.is_connected() {
            return Err(anyhow::anyhow!("Network client is not connected"));
        }

        let response_frame = client
            .send_frame_and_wait(&frame, timeout)
            .await
            .context("Failed to send frame and wait for response")?;

        let msg_cmd = response_frame
            .command
            .as_ref()
            .and_then(|cmd| {
                cmd.r#type.as_ref().and_then(|t| match t {
                    flare_core::common::protocol::flare::core::commands::command::Type::Message(
                        msg_cmd,
                    ) => Some(msg_cmd),
                    _ => None,
                })
            })
            .ok_or_else(|| anyhow::anyhow!("Response frame is not a MessageCommand"))?;

        if msg_cmd.r#type
            != flare_core::common::protocol::flare::core::commands::message_command::Type::Data
                as i32
        {
            return Err(anyhow::anyhow!(
                "Expected MessageCommand(Data=2) response, got type={}",
                msg_cmd.r#type
            ));
        }

        if msg_cmd.payload.is_empty() {
            return Err(anyhow::anyhow!("MessageCommand payload is empty"));
        }

        ServerPacket::decode(msg_cmd.payload.as_slice()).context("Failed to decode ServerPacket")
    }

    /// 发送会话增量同步请求
    pub async fn send_sync_conversations(
        &self,
        req: SyncConversationsRequest,
        timeout: Duration,
    ) -> Result<SyncConversationsResponse> {
        self.send_client_packet_and_wait(
            req,
            |r| flare_proto::common::client_packet::Payload::SyncConversations(r),
            |p| match p {
                flare_proto::common::server_packet::Payload::SyncConversationsResp(r) => Some(r),
                _ => None,
            },
            timeout,
        )
        .await
    }

    /// 发送按会话事件同步请求（长连接线缆：SyncRequest → SyncResponse）
    pub async fn send_sync_request(
        &self,
        req: SyncRequest,
        timeout: Duration,
    ) -> Result<SyncResponse> {
        self.send_client_packet_and_wait(
            req,
            |r| flare_proto::common::client_packet::Payload::SyncRequest(r),
            |p| match p {
                flare_proto::common::server_packet::Payload::SyncResp(r) => Some(r),
                _ => None,
            },
            timeout,
        )
        .await
    }

    /// 发送全量会话同步请求
    pub async fn send_sync_conversations_all(
        &self,
        req: ConversationSyncAllRequest,
        timeout: Duration,
    ) -> Result<ConversationSyncAllResponse> {
        self.send_client_packet_and_wait(
            req,
            |r| flare_proto::common::client_packet::Payload::SyncConversationsAll(r),
            |p| match p {
                flare_proto::common::server_packet::Payload::SyncConversationsAllResp(r) => {
                    Some(r)
                }
                _ => None,
            },
            timeout,
        )
        .await
    }

    /// 发送单条消息并等待 SendAck（ClientPacket.send_message = Message）
    pub async fn send_message_and_wait_ack(
        &self,
        message: &Message,
        timeout: Duration,
    ) -> Result<SendMessageResult> {
        let proto_message = MessageConverter::to_proto(message)
            .context("Failed to convert message to proto")?;

        let packet = ClientPacket {
            payload: Some(flare_proto::common::client_packet::Payload::SendMessage(
                proto_message,
            )),
        };

        let server_packet = self.send_packet_and_wait(packet, timeout).await?;
        let payload = server_packet
            .payload
            .ok_or_else(|| anyhow::anyhow!("ServerPacket.payload is None"))?;

        match payload {
            flare_proto::common::server_packet::Payload::SendAck(ack) => Ok(SendMessageResult {
                server_msg_id: ack.server_msg_id,
                seq: ack.seq,
                success: ack.success,
                error_code: ack.error_code,
                error_message: ack.error_message,
            }),
            flare_proto::common::server_packet::Payload::Error(err_pkt) => Err(anyhow::anyhow!(
                "Server error (code={}): {}",
                err_pkt.code,
                err_pkt.message
            )),
            other => Err(anyhow::anyhow!(
                "Unexpected ServerPacket payload: {:?}",
                other
            )),
        }
    }

    /// 发送 Event（操作）并等待 OperationAck
    pub async fn send_event_and_wait_ack(
        &self,
        event: flare_proto::common::Event,
        timeout: Duration,
    ) -> Result<flare_proto::common::OperationAck> {
        let packet = ClientPacket {
            payload: Some(flare_proto::common::client_packet::Payload::SendEvent(event)),
        };
        let server_packet = self.send_packet_and_wait(packet, timeout).await?;
        let payload = server_packet
            .payload
            .ok_or_else(|| anyhow::anyhow!("ServerPacket.payload is None"))?;
        match payload {
            flare_proto::common::server_packet::Payload::OperationAck(ack) => Ok(ack),
            flare_proto::common::server_packet::Payload::Error(err_pkt) => Err(anyhow::anyhow!(
                "Server error (code={}): {}",
                err_pkt.code,
                err_pkt.message
            )),
            other => Err(anyhow::anyhow!(
                "Unexpected ServerPacket payload: {:?}",
                other
            )),
        }
    }
}

#[async_trait]
impl SyncTransport for MessageSender {
    async fn sync_conversations_all(
        &self,
        req: ConversationSyncAllRequest,
        timeout: Duration,
    ) -> anyhow::Result<ConversationSyncAllResponse> {
        self.send_sync_conversations_all(req, timeout)
            .await
            .map_err(Into::into)
    }

    async fn sync_messages(
        &self,
        req: SyncRequest,
        timeout: Duration,
    ) -> anyhow::Result<SyncResponse> {
        self.send_sync_request(req, timeout).await.map_err(Into::into)
    }
}
