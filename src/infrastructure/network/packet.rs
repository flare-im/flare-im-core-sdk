//! ServerPacket 解析器
//!
//! 负责解析 ServerPacket 并根据不同的 payload 类型进行处理

use flare_proto::common::ServerPacket;
use flare_proto::common::MessageEnvelope;
use flare_proto::common::CustomPushData;
use flare_proto::common::SyncMessagesResponse;
use flare_proto::common::SyncConversationsResponse;
use flare_proto::common::ConversationSyncAllResponse;
use flare_proto::common::GetConversationDetailResponse;
use flare_proto::common::SendEnvelopeAck;
use tracing::{debug, warn};


/// ServerPacket 解析结果
#[derive(Debug)]
pub enum PacketParseResult {
    /// 消息数据（需要提取 MessageEnvelope 中的消息）
    Message(MessageEnvelope),
    
    /// 自定义推送数据
    CustomPushData(CustomPushData),
    
    /// 同步消息响应（包含 MessageEnvelope）
    SyncMessagesResponse(SyncMessagesResponse),
    
    /// 会话增量同步响应
    SyncConversationsResponse(SyncConversationsResponse),
    
    /// 全量会话同步响应
    ConversationSyncAllResponse(ConversationSyncAllResponse),
    
    /// 会话详情响应
    GetConversationDetailResponse(GetConversationDetailResponse),
    
    /// ACK 消息（由 send_frame_and_wait 处理，不需要在这里处理）
    SendAck(SendEnvelopeAck),
    
    /// 未知或未支持的 payload 类型
    Unknown,
}

/// 解析 Frame 的 payload 为 ServerPacket
///
/// # 参数
///
/// * `payload` - MessageCommand 的 payload 字节
///
/// # 返回
///
/// * `Ok(PacketParseResult)` - 解析成功
/// * `Err` - 解析失败（不是 ServerPacket 格式）
pub fn parse_server_packet(payload: &[u8]) -> anyhow::Result<PacketParseResult> {
    use prost::Message as ProstMessage;
    
    match ServerPacket::decode(payload) {
        Ok(server_packet) => {
            match server_packet.payload {
                Some(flare_proto::common::server_packet::Payload::Envelope(envelope)) => {
                    debug!(
                        envelope_kind = envelope.kind,
                        message_count = envelope.messages.len(),
                        "Parsed ServerPacket as MessageEnvelope"
                    );
                    Ok(PacketParseResult::Message(envelope))
                }
                Some(flare_proto::common::server_packet::Payload::SendAck(ack)) => {
                    debug!(
                        message_id = %ack.message_id,
                        status = ack.status,
                        "Parsed ServerPacket as SendEnvelopeAck (handled by send_frame_and_wait)"
                    );
                    Ok(PacketParseResult::SendAck(ack))
                }
                Some(flare_proto::common::server_packet::Payload::SyncMessagesResp(sync_resp)) => {
                    debug!(
                        "Parsed ServerPacket as SyncMessagesResponse"
                    );
                    Ok(PacketParseResult::SyncMessagesResponse(sync_resp))
                }
                Some(flare_proto::common::server_packet::Payload::SyncConversationsResp(resp)) => {
                    debug!(
                        patch_count = resp.patches.len(),
                        "Parsed ServerPacket as SyncConversationsResponse"
                    );
                    Ok(PacketParseResult::SyncConversationsResponse(resp))
                }
                Some(flare_proto::common::server_packet::Payload::SyncConversationsAllResp(resp)) => {
                    debug!(
                        conversation_count = resp.conversations.len(),
                        "Parsed ServerPacket as ConversationSyncAllResponse"
                    );
                    Ok(PacketParseResult::ConversationSyncAllResponse(resp))
                }
                Some(flare_proto::common::server_packet::Payload::GetConversationDetailResp(resp)) => {
                    debug!("Parsed ServerPacket as GetConversationDetailResponse");
                    Ok(PacketParseResult::GetConversationDetailResponse(resp))
                }
                Some(flare_proto::common::server_packet::Payload::CustomPushData(custom)) => {
                    debug!(
                        data_type = %custom.r#type,
                        payload_len = custom.payload.len(),
                        "Parsed ServerPacket as CustomPushData"
                    );
                    Ok(PacketParseResult::CustomPushData(custom))
                }
                None => {
                    warn!("ServerPacket has no payload");
                    Ok(PacketParseResult::Unknown)
                }
            }
        }
        Err(e) => {
            // 不是 ServerPacket 格式，返回错误以便尝试其他解析方式
            Err(anyhow::anyhow!("Not a ServerPacket: {}", e))
        }
    }
}

/// 从 PacketParseResult 中提取 MessageEnvelope
///
/// 用于从不同的响应类型中提取消息数据
pub fn extract_message_envelope(result: &PacketParseResult) -> Option<&flare_proto::common::MessageEnvelope> {
    match result {
        PacketParseResult::Message(envelope) => Some(envelope),
        PacketParseResult::SyncMessagesResponse(sync_resp) => {
            sync_resp.envelope.as_ref()
        }
        _ => None,
    }
}
