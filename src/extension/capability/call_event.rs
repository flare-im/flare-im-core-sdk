//! RTC/通话能力包构造：实时信令走 `DataPacket.capability`，不进入 durable Event，不占用 conversation_seq。

use std::collections::HashMap;

use flare_proto::common::CapabilityPacket;

pub const RTC_CAPABILITY_ID: &str = "rtc.call";

pub fn rtc_capability_packet(
    packet_type: impl Into<String>,
    payload: Vec<u8>,
    correlation_id: Option<String>,
    attributes: HashMap<String, String>,
) -> CapabilityPacket {
    CapabilityPacket {
        capability_id: RTC_CAPABILITY_ID.to_string(),
        packet_type: packet_type.into(),
        version: "1".to_string(),
        payload,
        attributes,
        correlation_id,
    }
}
