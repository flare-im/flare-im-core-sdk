//! 能力插件命令：当前提供 RTC 音视频通话能力透传。

use tauri::State;

use crate::state::SdkState;
use flare_im_core_sdk::client::{CapabilityDispatchResult, RtcSfuSubscriptionRequest};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct RtcSfuSetSubscriptionPayload {
    pub conversation_id: String,
    pub room_id: String,
    pub subscriber_peer_id: String,
    pub track_id: String,
    pub enable: bool,
    pub media: Option<String>,
    pub preferred_layer: Option<String>,
    pub priority: Option<u32>,
    pub tenant_id: Option<String>,
}

#[tauri::command]
pub async fn sdk_rtc_start_audio(
    state: State<'_, SdkState>,
    conversation_id: String,
    codec: Option<String>,
    tenant_id: Option<String>,
) -> std::result::Result<CapabilityDispatchResult, String> {
    state
        .capability_api()
        .await
        .map_err(super::map_sdk_err)?
        .rtc_start_audio(&conversation_id, codec.as_deref(), tenant_id.as_deref())
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_rtc_start_video(
    state: State<'_, SdkState>,
    conversation_id: String,
    codec: Option<String>,
    tenant_id: Option<String>,
) -> std::result::Result<CapabilityDispatchResult, String> {
    state
        .capability_api()
        .await
        .map_err(super::map_sdk_err)?
        .rtc_start_video(&conversation_id, codec.as_deref(), tenant_id.as_deref())
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_rtc_accept_call(
    state: State<'_, SdkState>,
    conversation_id: String,
    call_id: String,
    tenant_id: Option<String>,
) -> std::result::Result<CapabilityDispatchResult, String> {
    state
        .capability_api()
        .await
        .map_err(super::map_sdk_err)?
        .rtc_accept(&conversation_id, &call_id, tenant_id.as_deref())
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_rtc_end_call(
    state: State<'_, SdkState>,
    conversation_id: String,
    call_id: String,
    tenant_id: Option<String>,
) -> std::result::Result<CapabilityDispatchResult, String> {
    state
        .capability_api()
        .await
        .map_err(super::map_sdk_err)?
        .rtc_end(&conversation_id, &call_id, tenant_id.as_deref())
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_rtc_reject_call(
    state: State<'_, SdkState>,
    conversation_id: String,
    call_id: String,
    tenant_id: Option<String>,
) -> std::result::Result<CapabilityDispatchResult, String> {
    state
        .capability_api()
        .await
        .map_err(super::map_sdk_err)?
        .rtc_reject(&conversation_id, &call_id, tenant_id.as_deref())
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_rtc_sfu_join_room(
    state: State<'_, SdkState>,
    conversation_id: String,
    call_id: String,
    room_id: String,
    role: Option<String>,
    tenant_id: Option<String>,
) -> std::result::Result<CapabilityDispatchResult, String> {
    state
        .capability_api()
        .await
        .map_err(super::map_sdk_err)?
        .rtc_sfu_join_room(
            &conversation_id,
            &call_id,
            &room_id,
            role.as_deref(),
            tenant_id.as_deref(),
        )
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_rtc_sfu_leave_room(
    state: State<'_, SdkState>,
    conversation_id: String,
    room_id: String,
    peer_id: String,
    session_id: String,
    tenant_id: Option<String>,
) -> std::result::Result<CapabilityDispatchResult, String> {
    state
        .capability_api()
        .await
        .map_err(super::map_sdk_err)?
        .rtc_sfu_leave_room(
            &conversation_id,
            &room_id,
            &peer_id,
            &session_id,
            tenant_id.as_deref(),
        )
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_rtc_sfu_handle_sdp_offer(
    state: State<'_, SdkState>,
    conversation_id: String,
    room_id: String,
    peer_id: String,
    sdp_offer: String,
    tenant_id: Option<String>,
) -> std::result::Result<CapabilityDispatchResult, String> {
    state
        .capability_api()
        .await
        .map_err(super::map_sdk_err)?
        .rtc_sfu_handle_sdp_offer(
            &conversation_id,
            &room_id,
            &peer_id,
            &sdp_offer,
            tenant_id.as_deref(),
        )
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_rtc_sfu_add_ice_candidate(
    state: State<'_, SdkState>,
    conversation_id: String,
    room_id: String,
    peer_id: String,
    candidate_json: String,
    tenant_id: Option<String>,
) -> std::result::Result<CapabilityDispatchResult, String> {
    state
        .capability_api()
        .await
        .map_err(super::map_sdk_err)?
        .rtc_sfu_add_ice_candidate(
            &conversation_id,
            &room_id,
            &peer_id,
            &candidate_json,
            tenant_id.as_deref(),
        )
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_rtc_sfu_get_room_state(
    state: State<'_, SdkState>,
    conversation_id: String,
    room_id: String,
    tenant_id: Option<String>,
) -> std::result::Result<CapabilityDispatchResult, String> {
    state
        .capability_api()
        .await
        .map_err(super::map_sdk_err)?
        .rtc_sfu_get_room_state(&conversation_id, &room_id, tenant_id.as_deref())
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_rtc_sfu_set_subscription(
    state: State<'_, SdkState>,
    payload: RtcSfuSetSubscriptionPayload,
) -> std::result::Result<CapabilityDispatchResult, String> {
    state
        .capability_api()
        .await
        .map_err(super::map_sdk_err)?
        .rtc_sfu_set_subscription(RtcSfuSubscriptionRequest {
            conversation_id: payload.conversation_id,
            room_id: payload.room_id,
            subscriber_peer_id: payload.subscriber_peer_id,
            track_id: payload.track_id,
            enable: payload.enable,
            media: payload.media,
            preferred_layer: payload.preferred_layer,
            priority: payload.priority.unwrap_or(0),
            tenant_id: payload.tenant_id,
        })
        .await
        .map_err(super::map_sdk_err)
}
