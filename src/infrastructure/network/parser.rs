//! 消息解析器
//!
//! 下行消息帧的 payload 统一为 ServerPacket（见 flare-proto transport.proto）。
//! 解压后按 ServerPacket 解析，从 EventEnvelope / SyncResponse 中提取 Message 并转为领域消息。

use flare_core::common::compression::CompressionUtil;
use flare_core::common::protocol::Frame;

use crate::infrastructure::converter::MessageConverter;
use crate::infrastructure::network::packet::{extract_messages_from_result, parse_server_packet};

/// 若 payload 为压缩数据则解压，否则返回原切片。
pub(crate) fn ensure_decompressed_payload(payload: &[u8]) -> Vec<u8> {
    match CompressionUtil::auto_decompress(payload) {
        Ok((decompressed, _)) => decompressed,
        Err(_) => payload.to_vec(),
    }
}

/// 从 Frame 解析出领域消息列表。payload 必须为 ServerPacket 编码。
pub fn parse_frame_to_messages(
    frame: &Frame,
) -> anyhow::Result<Vec<crate::domain::message::Message>> {
    let msg_cmd = frame
        .command
        .as_ref()
        .and_then(|cmd| {
            cmd.r#type.as_ref().and_then(|t| match t {
                flare_core::common::protocol::flare::core::commands::command::Type::Message(
                    msg_cmd,
                ) => Some(msg_cmd.clone()),
                _ => None,
            })
        })
        .ok_or_else(|| anyhow::anyhow!("Frame does not contain MessageCommand"))?;

    let payload = ensure_decompressed_payload(msg_cmd.payload.as_slice());
    let packet_result = parse_server_packet(&payload)
        .map_err(|e| anyhow::anyhow!("decode ServerPacket failed (payload_len={}): {}", payload.len(), e))?;

    let proto_messages = extract_messages_from_result(&packet_result);
    let mut out = Vec::with_capacity(proto_messages.len());
    for proto_message in &proto_messages {
        if let Ok(m) = MessageConverter::from_proto(proto_message) {
            out.push(m);
        }
    }
    Ok(out)
}
