use async_trait::async_trait;
use prost::Message as ProstMessage;

use crate::error::{SdkError, Result};
use crate::model::{ClientPacket, ServerPacket};

/// 协议编解码 trait — 解耦业务层与具体序列化格式
#[async_trait]
pub trait Codec: Send + Sync {
    fn name(&self) -> &str;
    fn encode_client(&self, packet: &ClientPacket) -> Result<Vec<u8>>;
    fn decode_server(&self, data: &[u8]) -> Result<ServerPacket>;
}

/// Protobuf 编解码器（默认实现）
pub struct ProtobufCodec;

#[async_trait]
impl Codec for ProtobufCodec {
    fn name(&self) -> &str { "protobuf" }

    fn encode_client(&self, packet: &ClientPacket) -> Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(packet.encoded_len());
        packet.encode(&mut buf)
            .map_err(|e| SdkError::Codec(format!("encode: {e}")))?;
        Ok(buf)
    }

    fn decode_server(&self, data: &[u8]) -> Result<ServerPacket> {
        let decompressed = ensure_decompressed(data);
        ServerPacket::decode(decompressed.as_slice())
            .map_err(|e| SdkError::Codec(format!("decode: {e}")))
    }
}

fn ensure_decompressed(data: &[u8]) -> Vec<u8> {
    if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
        use std::io::Read;
        let mut decoder = flate2::read::GzDecoder::new(data);
        let mut out = Vec::new();
        if decoder.read_to_end(&mut out).is_ok() {
            return out;
        }
    }
    data.to_vec()
}
