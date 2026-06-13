use crate::infrastructure::protocol::DownlinkPayload;
use crate::shared::error::{FlareError, Result};
use flare_proto::common::data_packet::Payload as DataPacketPayload;
use flare_proto::common::{CustomData, DataPacket, Event, EventEnvelope, MessagePush, SyncRes};
use prost::Message;

/// 服务端下行包解码（与 flare-proto 对齐：含 `DataPacket` 封装的同步响应与用户扩展）
pub trait Codec: Send + Sync {
    fn decode_server(&self, payload: &[u8]) -> Result<DownlinkPayload>;
}

/// Protobuf 编解码实现：按协议约定顺序尝试解码
#[derive(Debug, Clone, Default)]
pub struct ProtobufCodec;

impl Codec for ProtobufCodec {
    fn decode_server(&self, payload: &[u8]) -> Result<DownlinkPayload> {
        if let Ok(push) = MessagePush::decode(payload) {
            return Ok(DownlinkPayload::MessagePush(push));
        }
        if let Ok(ev) = Event::decode(payload) {
            return Ok(DownlinkPayload::Event(ev));
        }
        if let Ok(env) = EventEnvelope::decode(payload) {
            return Ok(DownlinkPayload::EventEnvelope(env));
        }
        if let Ok(packet) = DataPacket::decode(payload) {
            match packet.payload {
                Some(DataPacketPayload::SyncResponse(res)) => {
                    return Ok(DownlinkPayload::SyncResp(res));
                }
                Some(DataPacketPayload::UserCustom(c)) => {
                    return Ok(DownlinkPayload::CustomData(c));
                }
                Some(DataPacketPayload::Capability(c)) => {
                    return Ok(DownlinkPayload::Capability(c));
                }
                Some(DataPacketPayload::RealtimeControl(control)) => {
                    return Ok(DownlinkPayload::RealtimeControl(control));
                }
                _ => {}
            }
        }
        if let Ok(data) = CustomData::decode(payload) {
            return Ok(DownlinkPayload::CustomData(data));
        }
        if let Ok(resp) = SyncRes::decode(payload) {
            return Ok(DownlinkPayload::SyncResp(resp));
        }
        Err(FlareError::deserialization_error(
            "payload is not MessagePush, Event, EventEnvelope, DataPacket, CustomData, or SyncRes"
                .to_string(),
        ))
    }
}
