//! 消息解析器
//!
//! 负责将网络层的 Frame 解析为领域层的 Message

use flare_core::common::protocol::Frame;
use prost::Message;
use tracing::debug;

/// 解析 Frame 为领域 Message
///
/// 对标微信、Telegram、飞书的消息解析逻辑
/// 
/// # 协议格式
/// 
/// 服务器推送的数据可能是：
/// 1. `ServerPacket`（统一传输包，优先解析）
///    - `envelope` - MessageEnvelope
///    - `sync_messages_resp` - 包含 MessageEnvelope 的同步响应
/// 2. `MessageEnvelope`（直接的消息封装，向后兼容）
/// 3. `Message`（单条消息，向后兼容）
///
/// # 参数
/// 
/// * `frame` - 要解析的 Frame
///
/// # 返回
/// 
/// * `Ok(Message)` - 解析成功
/// * `Err` - 解析失败
pub fn parse_frame_to_message(frame: &Frame) -> anyhow::Result<crate::domain::message::Message> {
    use crate::infrastructure::converter::MessageConverter;
    use flare_proto::flare::common::v1::Message as ProtoMessage;
    use flare_proto::common::MessageEnvelope;
    use super::packet::{parse_server_packet, extract_message_envelope};
    
    // 从 Frame 中提取 MessageCommand
    let msg_cmd = frame.command.as_ref()
        .and_then(|cmd| {
            cmd.r#type.as_ref().and_then(|t| {
                match t {
                    flare_core::common::protocol::flare::core::commands::command::Type::Message(msg_cmd) => {
                        Some(msg_cmd.clone())
                    }
                    _ => None,
                }
            })
        })
        .ok_or_else(|| anyhow::anyhow!("Frame does not contain MessageCommand"))?;
    
    // 优先尝试解析为 ServerPacket
    if let Ok(packet_result) = parse_server_packet(msg_cmd.payload.as_slice()) {
        // 从 ServerPacket 中提取 MessageEnvelope
        if let Some(envelope) = extract_message_envelope(&packet_result) {
            debug!(
                envelope_kind = envelope.kind,
                message_count = envelope.messages.len(),
                "Extracted MessageEnvelope from ServerPacket"
            );
            
            // 从 envelope 中提取第一个消息
            if envelope.messages.is_empty() {
                return Err(anyhow::anyhow!("MessageEnvelope contains no messages"));
            }
            
            // 取第一个消息（通常推送时只有一个消息）
            let proto_message = envelope.messages[0].clone();
            
            debug!(
                message_id = %proto_message.id,
                conversation_id = %proto_message.conversation_id,
                sender_id = %proto_message.sender_id,
                "Extracted message from MessageEnvelope"
            );
            
            // 转换为 Domain Message
            let message = MessageConverter::from_proto(&proto_message)?;
            return Ok(message);
        }
    }
    
    // 如果不是 ServerPacket 或无法提取 MessageEnvelope，尝试直接解析为 MessageEnvelope
    match MessageEnvelope::decode(msg_cmd.payload.as_slice()) {
        Ok(envelope) => {
            debug!(
                envelope_kind = envelope.kind,
                message_count = envelope.messages.len(),
                "Successfully parsed MessageEnvelope (direct)"
            );
            
            // 从 envelope 中提取第一个消息
            if envelope.messages.is_empty() {
                return Err(anyhow::anyhow!("MessageEnvelope contains no messages"));
            }
            
            // 取第一个消息（通常推送时只有一个消息）
            let proto_message = envelope.messages[0].clone();
            
            debug!(
                message_id = %proto_message.id,
                conversation_id = %proto_message.conversation_id,
                sender_id = %proto_message.sender_id,
                "Extracted message from MessageEnvelope"
            );
            
            // 转换为 Domain Message
            let message = MessageConverter::from_proto(&proto_message)?;
            
            Ok(message)
        }
        Err(e) => {
            // 如果解析为 MessageEnvelope 失败，尝试直接解析为 Message（向后兼容）
            // 这可能是旧版本服务器或某些特殊场景
            debug!(
                error = %e,
                payload_len = msg_cmd.payload.len(),
                "Failed to parse as MessageEnvelope, trying direct Message decode"
            );
            let proto_message = ProtoMessage::decode(msg_cmd.payload.as_slice())?;
            let message = MessageConverter::from_proto(&proto_message)?;
            Ok(message)
        }
    }
}
