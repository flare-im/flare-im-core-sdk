//! 通话信令：经 IM 领域事件 `EVENT_CALL_SIGNAL` 上行（与下行 `im://call_signal` 对称）。
//! 入参为 [`crate::model`] 中的结构化请求体，与 **`flare-sdk-plugin-call::production`** 语义一致。

use flare_proto::common::CallMediaType;
use flare_proto::common::SfuTransportContext;
use tauri::State;

use crate::model::{
    CallPluginAcceptRequest, CallPluginHangupRequest, CallPluginIceCandidateRequest,
    CallPluginInviteRequest, CallPluginMediaConstraintsRequest, CallPluginRejectRequest,
    CallPluginWebrtcSdpRequest,
};
use crate::state::SdkState;

fn non_empty_opt(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn apply_sfu_runtime_hint(
    ev: &mut flare_proto::common::CallSignalEvent,
    room_id: Option<String>,
    peer_id: Option<String>,
    signaling_ws_base: Option<String>,
    join_token: Option<String>,
) {
    let room_id = non_empty_opt(room_id);
    let peer_id = non_empty_opt(peer_id);
    let signaling_ws_base = non_empty_opt(signaling_ws_base);
    let join_token = non_empty_opt(join_token);

    if room_id.is_none() && peer_id.is_none() && signaling_ws_base.is_none() && join_token.is_none()
    {
        return;
    }

    if room_id.is_some() || peer_id.is_some() || signaling_ws_base.is_some() {
        let mut transport = ev.transport.take().unwrap_or_default();
        if let Some(v) = room_id {
            transport.room_id = v;
        }
        if let Some(v) = peer_id {
            transport.peer_id = v;
        }
        if let Some(v) = signaling_ws_base {
            transport.signaling_ws_base = Some(v);
        }
        ev.transport = Some(SfuTransportContext {
            room_id: transport.room_id,
            peer_id: transport.peer_id,
            media_session_id: transport.media_session_id,
            track_id: transport.track_id,
            signaling_ws_base: transport.signaling_ws_base,
            instance_id: transport.instance_id,
        });
    }
    if let Some(v) = join_token {
        ev.ext.insert("sfu_join_token".to_string(), v);
    }
}

#[tauri::command]
pub async fn sdk_send_call_invite(
    state: State<'_, SdkState>,
    req: CallPluginInviteRequest,
) -> std::result::Result<(), String> {
    let client = state.client();
    let from = client
        .current_user_id()
        .await
        .unwrap_or_default()
        .trim()
        .to_string();
    if from.is_empty() {
        return Err("not logged in".to_string());
    }
    let types: Vec<CallMediaType> = if req.video {
        vec![CallMediaType::Audio, CallMediaType::Video]
    } else {
        vec![CallMediaType::Audio]
    };
    let mut ev =
        flare_im_core_sdk::extension::capability::call_event::call_invite_for_conversation(
            req.conversation_id.clone(),
            req.call_id.clone(),
            from,
            req.to_user_id,
            req.participant_user_ids,
            types.as_slice(),
        )
        .map_err(|e| e.to_string())?;
    apply_sfu_runtime_hint(
        &mut ev,
        req.sfu_room_id,
        req.sfu_peer_id,
        req.sfu_signaling_ws_base,
        req.sfu_join_token,
    );
    client
        .send_call_signal(&req.conversation_id, ev)
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_send_call_accept(
    state: State<'_, SdkState>,
    req: CallPluginAcceptRequest,
) -> std::result::Result<(), String> {
    let to_user_id = req.to_user_id.clone();
    let client = state.client();
    let from = client
        .current_user_id()
        .await
        .unwrap_or_default()
        .trim()
        .to_string();
    if from.is_empty() {
        return Err("not logged in".to_string());
    }
    let types: Vec<CallMediaType> = if req.video {
        vec![CallMediaType::Audio, CallMediaType::Video]
    } else {
        vec![CallMediaType::Audio]
    };
    let ev = flare_im_core_sdk::extension::capability::call_event::call_accept(
        req.conversation_id.clone(),
        req.call_id,
        from,
        types.as_slice(),
    );
    let mut ev = ev;
    flare_im_core_sdk::call_plugin::apply_session_signaling_audience(
        &req.conversation_id,
        &mut ev,
        to_user_id.as_deref(),
    );
    apply_sfu_runtime_hint(
        &mut ev,
        req.sfu_room_id,
        req.sfu_peer_id,
        req.sfu_signaling_ws_base,
        req.sfu_join_token,
    );
    client
        .send_call_signal(&req.conversation_id, ev)
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_send_call_hangup(
    state: State<'_, SdkState>,
    req: CallPluginHangupRequest,
) -> std::result::Result<(), String> {
    let to_user_id = req.to_user_id.clone();
    let client = state.client();
    let from = client
        .current_user_id()
        .await
        .unwrap_or_default()
        .trim()
        .to_string();
    if from.is_empty() {
        return Err("not logged in".to_string());
    }
    let ev = flare_im_core_sdk::extension::capability::call_event::call_hangup_with_metadata(
        req.conversation_id.clone(),
        req.call_id,
        from,
        req.reason,
        req.duration_seconds,
        req.close_room_if_vacant,
        flare_im_core_sdk::call_plugin::parse_call_end_reason_code(req.reason_code.as_deref()),
        flare_im_core_sdk::call_plugin::parse_call_visibility_scope(
            req.visibility_scope.as_deref(),
        ),
        req.timeout_seconds,
    );
    let mut ev = ev;
    if let Some(mode) = req.mode.as_deref() {
        let normalized = mode.trim().to_ascii_lowercase();
        if normalized == "audio" || normalized == "video" {
            ev.ext.insert("call_mode".to_string(), normalized);
        }
    }
    flare_im_core_sdk::call_plugin::apply_session_signaling_audience(
        &req.conversation_id,
        &mut ev,
        to_user_id.as_deref(),
    );
    client
        .send_call_signal(&req.conversation_id, ev)
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_send_call_reject(
    state: State<'_, SdkState>,
    req: CallPluginRejectRequest,
) -> std::result::Result<(), String> {
    let to_user_id = req.to_user_id.clone();
    let client = state.client();
    let from = client
        .current_user_id()
        .await
        .unwrap_or_default()
        .trim()
        .to_string();
    if from.is_empty() {
        return Err("not logged in".to_string());
    }
    let ev = flare_im_core_sdk::extension::capability::call_event::call_reject(
        req.conversation_id.clone(),
        req.call_id,
        from,
        req.reason,
        req.code,
    );
    let mut ev = ev;
    flare_im_core_sdk::call_plugin::apply_session_signaling_audience(
        &req.conversation_id,
        &mut ev,
        to_user_id.as_deref(),
    );
    client
        .send_call_signal(&req.conversation_id, ev)
        .await
        .map_err(super::map_sdk_err)
}

/// ICE 候选（`signal.ice_candidate`），与 `RTCPeerConnection.onicecandidate` 对齐。
#[tauri::command]
pub async fn sdk_send_call_ice_candidate(
    state: State<'_, SdkState>,
    req: CallPluginIceCandidateRequest,
) -> std::result::Result<(), String> {
    let to_user_id = req.to_user_id.clone();
    flare_im_core_sdk::call_plugin::validate_ice_payload_size(req.candidate.len())
        .map_err(|e| e.to_string())?;
    let client = state.client();
    let from = client
        .current_user_id()
        .await
        .unwrap_or_default()
        .trim()
        .to_string();
    if from.is_empty() {
        return Err("not logged in".to_string());
    }
    let candidate_json = serde_json::json!({
        "candidate": req.candidate,
        "sdpMid": req.sdp_mid,
        "sdpMLineIndex": req.sdp_mline_index
    })
    .to_string();
    let ev = flare_im_core_sdk::extension::capability::call_event::call_ice_candidate_with_json(
        req.conversation_id.clone(),
        req.call_id,
        from,
        req.candidate,
        req.sdp_mid,
        req.sdp_mline_index,
        Some(candidate_json),
    );
    let mut ev = ev;
    flare_im_core_sdk::call_plugin::apply_session_signaling_audience(
        &req.conversation_id,
        &mut ev,
        to_user_id.as_deref(),
    );
    client
        .send_call_signal(&req.conversation_id, ev)
        .await
        .map_err(super::map_sdk_err)
}

/// P2P WebRTC：SDP 经 `signal.renegotiate` + `ext`（标准键名，见 `flare-sdk-plugin-call::rtc::P2P_EXT_*`）。
#[tauri::command]
pub async fn sdk_send_call_webrtc_sdp(
    state: State<'_, SdkState>,
    req: CallPluginWebrtcSdpRequest,
) -> std::result::Result<(), String> {
    let to_user_id = req.to_user_id.clone();
    flare_im_core_sdk::call_plugin::validate_sdp_payload_size(req.sdp.len())
        .map_err(|e| e.to_string())?;
    let kind = req.sdp_type.to_ascii_lowercase();
    if kind != "offer" && kind != "answer" {
        return Err("sdp_type must be offer or answer".to_string());
    }
    let client = state.client();
    let from = client
        .current_user_id()
        .await
        .unwrap_or_default()
        .trim()
        .to_string();
    if from.is_empty() {
        return Err("not logged in".to_string());
    }
    let ev = flare_im_core_sdk::extension::capability::call_event::call_renegotiate_p2p_sdp(
        req.conversation_id.clone(),
        req.call_id,
        from,
        &kind,
        req.sdp,
        &[],
    );
    let mut ev = ev;
    flare_im_core_sdk::call_plugin::apply_session_signaling_audience(
        &req.conversation_id,
        &mut ev,
        to_user_id.as_deref(),
    );
    client
        .send_call_signal(&req.conversation_id, ev)
        .await
        .map_err(super::map_sdk_err)
}

/// `getUserMedia` 约束 JSON（与 `flare-sdk-plugin-call::media::user_media_constraints_json` 一致）。
#[tauri::command]
pub fn sdk_build_call_media_constraints(
    req: CallPluginMediaConstraintsRequest,
) -> std::result::Result<serde_json::Value, String> {
    use flare_im_core_sdk::flare_sdk_plugin_call::media::{
        CallMediaProfile, user_media_constraints_json,
    };

    let profile: CallMediaProfile = match req.profile_json {
        Some(ref s) if !s.trim().is_empty() => {
            serde_json::from_str(s).map_err(|e| e.to_string())?
        }
        _ => CallMediaProfile::default(),
    };
    Ok(user_media_constraints_json(&profile, req.include_video))
}
