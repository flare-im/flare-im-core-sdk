//! RTC dispatch 常量：通话与 SFU 信令统一走 `DataPacket.capability`。
//!
//! `call_signal.proto` 已从协议层移除；core SDK 直接维护稳定的 `rtc.*`
//! capability IDs 和 JSON payload 形态，避免 optional feature 再拉入旧 durable Event 模型。

mod fallback {
    use serde_json::{Value, json};

    pub const CALL_AUDIO: &str = "rtc.call.audio";
    pub const CALL_VIDEO: &str = "rtc.call.video";
    pub const CALL_ACCEPT: &str = "rtc.call.accept";
    pub const CALL_END: &str = "rtc.call.end";
    pub const CALL_REJECT: &str = "rtc.call.reject";
    pub const CALL_JOIN_TOKEN: &str = "rtc.call.join_token";
    pub const SFU_JOIN_ROOM: &str = "rtc.media.join";
    pub const SFU_LEAVE_ROOM: &str = "rtc.media.leave";
    pub const SFU_HANDLE_SDP_OFFER: &str = "rtc.media.sdp.offer";
    pub const SFU_HANDLE_SDP_ANSWER: &str = "rtc.media.sdp.answer";
    pub const SFU_ADD_ICE_CANDIDATE: &str = "rtc.media.ice.candidate";
    pub const SFU_SET_SUBSCRIPTION: &str = "rtc.media.subscription.set";
    pub const SFU_GET_ROOM_STATE: &str = "rtc.media.room.state";

    pub fn payload_start_audio(codec: Option<&str>) -> Value {
        json!({ "codec": codec.unwrap_or("OPUS") })
    }

    pub fn payload_start_video(codec: Option<&str>) -> Value {
        json!({ "codec": codec.unwrap_or("VP8") })
    }

    pub fn payload_call_id(call_id: &str) -> Value {
        json!({ "call_id": call_id })
    }

    pub fn payload_call_end_with_room_policy(call_id: &str, close_room_if_vacant: bool) -> Value {
        json!({
            "call_id": call_id,
            "close_room_if_vacant": close_room_if_vacant,
        })
    }

    pub fn payload_sfu_join_room(call_id: &str, room_id: &str, role: Option<&str>) -> Value {
        json!({
            "call_id": call_id,
            "room_id": room_id,
            "role": role.unwrap_or("participant"),
        })
    }

    pub fn payload_sfu_leave_room(room_id: &str, peer_id: &str, session_id: &str) -> Value {
        json!({
            "room_id": room_id,
            "peer_id": peer_id,
            "session_id": session_id,
        })
    }

    pub fn payload_sfu_handle_sdp_offer(room_id: &str, peer_id: &str, sdp_offer: &str) -> Value {
        json!({
            "room_id": room_id,
            "peer_id": peer_id,
            "sdp_offer": sdp_offer,
        })
    }

    pub fn payload_sfu_handle_sdp_answer(room_id: &str, peer_id: &str, sdp_answer: &str) -> Value {
        json!({
            "room_id": room_id,
            "peer_id": peer_id,
            "sdp_answer": sdp_answer,
        })
    }

    pub fn payload_sfu_add_ice_candidate(
        room_id: &str,
        peer_id: &str,
        candidate_json: &str,
    ) -> Value {
        json!({
            "room_id": room_id,
            "peer_id": peer_id,
            "candidate_json": candidate_json,
        })
    }

    pub fn payload_sfu_set_subscription(
        room_id: &str,
        subscriber_peer_id: &str,
        track_id: &str,
        enable: bool,
        media: Option<&str>,
        preferred_layer: Option<&str>,
        priority: u32,
    ) -> Value {
        json!({
            "room_id": room_id,
            "subscriber_peer_id": subscriber_peer_id,
            "track_id": track_id,
            "enable": enable,
            "media": media,
            "preferred_layer": preferred_layer,
            "priority": priority,
        })
    }

    pub fn payload_sfu_get_room_state(room_id: &str) -> Value {
        json!({ "room_id": room_id })
    }
}

pub use fallback::*;
