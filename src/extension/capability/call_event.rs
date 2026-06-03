//! 上行 `EVENT_CALL_SIGNAL` 领域事件封装（经 [`crate::infrastructure::protocol::PacketSender::send_event`]）。

use flare_proto::common::{CallSignalEvent, Event};

#[cfg(feature = "plugin-call")]
pub use flare_sdk_plugin_call::call_signal::{
    call_accept, call_hangup, call_hangup_no_answer_self_only, call_hangup_with_metadata,
    call_hangup_with_room_policy, call_ice_candidate, call_ice_candidate_with_json, call_invite,
    call_invite_broadcast, call_invite_explicit, call_invite_for_conversation, call_reject,
    call_renegotiate_p2p_sdp, call_signal_shell, inferred_call_media_session_kind,
    normalize_call_invite_user_ids,
};
#[cfg(feature = "plugin-call")]
pub use flare_sdk_plugin_call::errors::CallInviteBuildError;

/// 构造并返回待发送的 `Event`（`seq` 一般由服务端分配，上行常用 `0`）。
pub fn event_call_signal_uplink(conversation_id: &str, seq: u64, call: CallSignalEvent) -> Event {
    #[cfg(feature = "plugin-call")]
    {
        flare_sdk_plugin_call::call_signal::event_with_call_signal(conversation_id, seq, call)
    }
    #[cfg(not(feature = "plugin-call"))]
    {
        use flare_proto::common::{EventType, event};
        Event {
            conversation_id: conversation_id.to_string(),
            seq,
            r#type: EventType::EventCallSignal as i32,
            payload: Some(event::Payload::CallSignal(call)),
            ..Default::default()
        }
    }
}
