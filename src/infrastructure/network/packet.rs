//! ServerPacket 解析（对齐 flare-proto transport.proto + event.proto）
//!
//! 下行：EventEnvelope / SendAck / SyncResponse / CustomPush / Error 等

use flare_proto::common::{
    EventEnvelope, SendAck, SyncResponse, CustomPushData,
    SyncConversationsResponse, ConversationSyncAllResponse,
    GetConversationDetailResponse, ErrorPacket, OperationAck,
};
use flare_proto::common::event::Payload as EventPayload;
use flare_proto::common::Message;
use tracing::debug;

/// 从 EventEnvelope 中提取所有 payload 为 Message 的事件消息
pub fn messages_from_event_envelope(envelope: &EventEnvelope) -> Vec<Message> {
    envelope
        .events
        .iter()
        .filter_map(|ev| {
            if let Some(EventPayload::Message(m)) = &ev.payload {
                Some(m.clone())
            } else {
                None
            }
        })
        .collect()
}

/// ServerPacket 解析结果
#[derive(Debug)]
pub enum PacketParseResult {
    /// 事件批（推送消息等）
    EventEnvelope(EventEnvelope),
    /// 同步响应（含 EventEnvelope）
    SyncResponse(SyncResponse),
    /// 会话增量同步
    SyncConversationsResponse(SyncConversationsResponse),
    /// 全量会话同步
    ConversationSyncAllResponse(ConversationSyncAllResponse),
    /// 会话详情
    GetConversationDetailResponse(GetConversationDetailResponse),
    /// 自定义推送
    CustomPush(CustomPushData),
    /// 发送回执
    SendAck(SendAck),
    /// 操作回执（Event 上行）
    OperationAck(OperationAck),
    /// 错误
    Error(ErrorPacket),
    Unknown,
}

/// 解析 Frame 的 payload 为 ServerPacket
pub fn parse_server_packet(payload: &[u8]) -> anyhow::Result<PacketParseResult> {
    use prost::Message as ProstMessage;
    match flare_proto::common::ServerPacket::decode(payload) {
        Ok(server_packet) => {
            match server_packet.payload {
                Some(flare_proto::common::server_packet::Payload::EventEnvelope(env)) => {
                    debug!(
                        event_count = env.events.len(),
                        "Parsed ServerPacket as EventEnvelope"
                    );
                    Ok(PacketParseResult::EventEnvelope(env))
                }
                Some(flare_proto::common::server_packet::Payload::SendAck(ack)) => {
                    debug!(
                        server_msg_id = %ack.server_msg_id,
                        success = ack.success,
                        "Parsed ServerPacket as SendAck"
                    );
                    Ok(PacketParseResult::SendAck(ack))
                }
                Some(flare_proto::common::server_packet::Payload::SyncResp(resp)) => {
                    debug!("Parsed ServerPacket as SyncResponse");
                    Ok(PacketParseResult::SyncResponse(resp))
                }
                Some(flare_proto::common::server_packet::Payload::SyncConversationsResp(r)) => {
                    debug!(patch_count = r.patches.len(), "Parsed SyncConversationsResponse");
                    Ok(PacketParseResult::SyncConversationsResponse(r))
                }
                Some(flare_proto::common::server_packet::Payload::SyncConversationsAllResp(r)) => {
                    debug!(conversation_count = r.conversations.len(), "Parsed ConversationSyncAllResponse");
                    Ok(PacketParseResult::ConversationSyncAllResponse(r))
                }
                Some(flare_proto::common::server_packet::Payload::GetConversationDetailResp(r)) => {
                    debug!("Parsed GetConversationDetailResponse");
                    Ok(PacketParseResult::GetConversationDetailResponse(r))
                }
                Some(flare_proto::common::server_packet::Payload::CustomPush(c)) => {
                    debug!(data_type = %c.r#type, "Parsed CustomPush");
                    Ok(PacketParseResult::CustomPush(c))
                }
                Some(flare_proto::common::server_packet::Payload::OperationAck(ack)) => {
                    debug!(success = ack.success, "Parsed OperationAck");
                    Ok(PacketParseResult::OperationAck(ack))
                }
                Some(flare_proto::common::server_packet::Payload::Error(e)) => {
                    debug!(code = e.code, message = %e.message, "Parsed ErrorPacket");
                    Ok(PacketParseResult::Error(e))
                }
                _ => Ok(PacketParseResult::Unknown),
            }
        }
        Err(e) => Err(anyhow::anyhow!("Not a ServerPacket: {}", e)),
    }
}

/// 从解析结果中提取消息列表（用于推送或同步）
pub fn extract_messages_from_result(result: &PacketParseResult) -> Vec<Message> {
    match result {
        PacketParseResult::EventEnvelope(env) => messages_from_event_envelope(env),
        PacketParseResult::SyncResponse(resp) => resp
            .envelope
            .as_ref()
            .map(messages_from_event_envelope)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}
