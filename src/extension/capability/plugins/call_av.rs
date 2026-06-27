//! 音视频（RTC/SFU）能力插件：将 `rtc.*` capability 请求转发到 `CapabilityApi::dispatch`。

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::client::api::{CapabilityApi, CapabilityDispatchResult, UserCapabilityGrantDto};
use crate::extension::capability::{
    SdkCapabilityPlugin, SdkPluginEventManifest, SdkPluginManifest, SdkPluginOperationManifest,
    SdkPluginPermissionManifest, rtc_ids,
};
use crate::shared::error::Result;

const CALL_AV_PLUGIN_ID: &str = "sdk.plugin.av";
const RTC_CAPABILITY_NAMESPACE: &str = "rtc";

/// AV 插件（RTC/SFU），通过 capability 服务统一下发命令。
pub struct AvCapabilityPlugin {
    api: Arc<CapabilityApi>,
}

impl AvCapabilityPlugin {
    pub fn new(api: Arc<CapabilityApi>) -> Self {
        Self { api }
    }
}

#[async_trait]
impl SdkCapabilityPlugin for AvCapabilityPlugin {
    fn plugin_id(&self) -> &'static str {
        CALL_AV_PLUGIN_ID
    }

    fn capability_namespaces(&self) -> &'static [&'static str] {
        &[RTC_CAPABILITY_NAMESPACE]
    }

    fn manifest(&self) -> SdkPluginManifest {
        let mut manifest =
            SdkPluginManifest::builtin(CALL_AV_PLUGIN_ID, &[RTC_CAPABILITY_NAMESPACE]);
        manifest.display_name = "Flare RTC Call Plugin".to_string();
        manifest.operations = vec![
            rtc_operation(rtc_ids::CALL_AUDIO, &["rtc.call"]),
            rtc_operation(rtc_ids::CALL_VIDEO, &["rtc.call", "rtc.media"]),
            rtc_operation(rtc_ids::CALL_ACCEPT, &["rtc.call"]),
            rtc_operation(rtc_ids::CALL_END, &["rtc.call"]),
            rtc_operation(rtc_ids::CALL_REJECT, &["rtc.call"]),
            rtc_operation(rtc_ids::CALL_JOIN_TOKEN, &["rtc.call", "rtc.media"]),
            rtc_operation(rtc_ids::SFU_JOIN_ROOM, &["rtc.media"]),
            rtc_operation(rtc_ids::SFU_LEAVE_ROOM, &["rtc.media"]),
            rtc_operation(rtc_ids::SFU_HANDLE_SDP_OFFER, &["rtc.media"]),
            rtc_operation(rtc_ids::SFU_HANDLE_SDP_ANSWER, &["rtc.media"]),
            rtc_operation(rtc_ids::SFU_ADD_ICE_CANDIDATE, &["rtc.media"]),
            rtc_operation(rtc_ids::SFU_SET_SUBSCRIPTION, &["rtc.media"]),
            rtc_operation(rtc_ids::SFU_GET_ROOM_STATE, &["rtc.media"]),
        ];
        manifest.events = vec![
            rtc_event("rtc.call.invited", "Incoming RTC call invitation."),
            rtc_event("rtc.call.accepted", "RTC call accepted by a participant."),
            rtc_event("rtc.call.ended", "RTC call ended."),
            rtc_event(
                "rtc.media.room.updated",
                "SFU room membership or media state changed.",
            ),
        ];
        manifest.permissions = vec![
            SdkPluginPermissionManifest {
                id: "rtc.call".to_string(),
                description: "Call signaling operations.".to_string(),
            },
            SdkPluginPermissionManifest {
                id: "rtc.media".to_string(),
                description: "SFU room, SDP, ICE and subscription operations.".to_string(),
            },
        ];
        manifest.platforms = vec![
            "android".to_string(),
            "ios".to_string(),
            "flutter".to_string(),
            "typescript-web".to_string(),
            "typescript-node".to_string(),
            "electron".to_string(),
            "harmonyos-arkts".to_string(),
            "cangjie".to_string(),
        ];
        manifest
    }

    async fn invoke(
        &self,
        capability_id: &str,
        payload: Value,
        conversation_id: Option<&str>,
        tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        self.api
            .dispatch(capability_id, payload, conversation_id, tenant_id, None)
            .await
    }

    async fn list_user_grants(
        &self,
        tenant_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<UserCapabilityGrantDto>> {
        self.api.list_user_capabilities(tenant_id, user_id).await
    }
}

fn rtc_event(id: &str, description: &str) -> SdkPluginEventManifest {
    SdkPluginEventManifest {
        id: id.to_string(),
        description: Some(description.to_string()),
        schema: rtc_event_schema(id),
    }
}

fn rtc_event_schema(id: &str) -> Value {
    match id {
        "rtc.call.invited" => json!({
            "type": "object",
            "required": ["call_id", "conversation_id", "caller_user_id", "media"],
            "properties": {
                "call_id": { "type": "string", "minLength": 1 },
                "conversation_id": { "type": "string", "minLength": 1 },
                "caller_user_id": { "type": "string", "minLength": 1 },
                "media": { "type": "string", "enum": ["audio", "video"] },
                "room_id": { "type": ["string", "null"], "minLength": 1 }
            },
            "additionalProperties": false
        }),
        "rtc.call.accepted" => json!({
            "type": "object",
            "required": ["call_id", "conversation_id", "accepted_by_user_id"],
            "properties": {
                "call_id": { "type": "string", "minLength": 1 },
                "conversation_id": { "type": "string", "minLength": 1 },
                "accepted_by_user_id": { "type": "string", "minLength": 1 },
                "room_id": { "type": ["string", "null"], "minLength": 1 }
            },
            "additionalProperties": false
        }),
        "rtc.call.ended" => json!({
            "type": "object",
            "required": ["call_id", "conversation_id", "ended_by_user_id", "reason"],
            "properties": {
                "call_id": { "type": "string", "minLength": 1 },
                "conversation_id": { "type": "string", "minLength": 1 },
                "ended_by_user_id": { "type": "string", "minLength": 1 },
                "reason": {
                    "type": "string",
                    "enum": ["completed", "rejected", "missed", "cancelled", "failed"]
                }
            },
            "additionalProperties": false
        }),
        "rtc.media.room.updated" => json!({
            "type": "object",
            "required": ["room_id", "revision"],
            "properties": {
                "room_id": { "type": "string", "minLength": 1 },
                "revision": { "type": "integer", "minimum": 0 },
                "call_id": { "type": ["string", "null"], "minLength": 1 },
                "joined_peer_ids": {
                    "type": "array",
                    "items": { "type": "string", "minLength": 1 }
                },
                "left_peer_ids": {
                    "type": "array",
                    "items": { "type": "string", "minLength": 1 }
                }
            },
            "additionalProperties": false
        }),
        _ => json!({
            "type": "object",
            "additionalProperties": false
        }),
    }
}

fn rtc_operation(op: &str, permissions: &[&str]) -> SdkPluginOperationManifest {
    let mut operation = SdkPluginOperationManifest::new(op);
    operation.permissions = permissions
        .iter()
        .map(|permission| (*permission).to_string())
        .collect();
    operation.input_schema = Some(rtc_input_schema(op));
    operation.output_schema = Some(rtc_output_schema(op));
    operation
}

fn rtc_output_schema(op: &str) -> Value {
    match op {
        rtc_ids::CALL_AUDIO | rtc_ids::CALL_VIDEO => json!({
            "type": "object",
            "required": ["call_id", "room_id"],
            "properties": {
                "call_id": { "type": "string", "minLength": 1 },
                "room_id": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": true
        }),
        rtc_ids::CALL_ACCEPT | rtc_ids::CALL_END | rtc_ids::CALL_REJECT => json!({
            "type": "object",
            "required": ["call_id"],
            "properties": {
                "call_id": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": true
        }),
        rtc_ids::CALL_JOIN_TOKEN => json!({
            "type": "object",
            "required": ["call_id", "sfu_join_token", "sfu_join_token_ttl_seconds"],
            "properties": {
                "call_id": { "type": "string", "minLength": 1 },
                "sfu_join_token": { "type": "string", "minLength": 1 },
                "sfu_join_token_ttl_seconds": { "type": "integer", "minimum": 0 }
            },
            "additionalProperties": true
        }),
        rtc_ids::SFU_JOIN_ROOM => json!({
            "type": "object",
            "required": ["room_id", "peer_id", "session_id", "call_id"],
            "properties": {
                "room_id": { "type": "string", "minLength": 1 },
                "peer_id": { "type": "string", "minLength": 1 },
                "session_id": { "type": "string", "minLength": 1 },
                "call_id": { "type": "string" }
            },
            "additionalProperties": true
        }),
        rtc_ids::SFU_LEAVE_ROOM => json!({
            "type": "object",
            "required": ["left"],
            "properties": {
                "left": { "type": "boolean" }
            },
            "additionalProperties": true
        }),
        rtc_ids::SFU_HANDLE_SDP_OFFER => json!({
            "type": "object",
            "required": ["sdp_answer"],
            "properties": {
                "sdp_answer": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": true
        }),
        rtc_ids::SFU_HANDLE_SDP_ANSWER | rtc_ids::SFU_ADD_ICE_CANDIDATE => json!({
            "type": "object",
            "required": ["accepted"],
            "properties": {
                "accepted": { "type": "boolean" }
            },
            "additionalProperties": true
        }),
        rtc_ids::SFU_SET_SUBSCRIPTION => json!({
            "type": "object",
            "required": ["applied"],
            "properties": {
                "applied": { "type": "boolean" }
            },
            "additionalProperties": true
        }),
        rtc_ids::SFU_GET_ROOM_STATE => json!({
            "type": "object",
            "required": ["room_id", "exists", "revision"],
            "properties": {
                "room_id": { "type": "string", "minLength": 1 },
                "exists": { "type": "boolean" },
                "revision": { "type": "integer", "minimum": 0 },
                "room_snapshot_json": { "type": "string" },
                "room_snapshot": { "type": "object" }
            },
            "additionalProperties": true
        }),
        _ => json!({
            "type": "object",
            "additionalProperties": true
        }),
    }
}

fn rtc_input_schema(op: &str) -> Value {
    match op {
        rtc_ids::CALL_AUDIO | rtc_ids::CALL_VIDEO => json!({
            "type": "object",
            "properties": {
                "codec": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        }),
        rtc_ids::CALL_ACCEPT | rtc_ids::CALL_END | rtc_ids::CALL_REJECT => json!({
            "type": "object",
            "required": ["call_id"],
            "properties": {
                "call_id": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        }),
        rtc_ids::CALL_JOIN_TOKEN => json!({
            "type": "object",
            "additionalProperties": false
        }),
        rtc_ids::SFU_JOIN_ROOM => json!({
            "type": "object",
            "required": ["call_id", "room_id"],
            "properties": {
                "call_id": { "type": "string", "minLength": 1 },
                "room_id": { "type": "string", "minLength": 1 },
                "role": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        }),
        rtc_ids::SFU_LEAVE_ROOM => json!({
            "type": "object",
            "required": ["room_id", "peer_id", "session_id"],
            "properties": {
                "room_id": { "type": "string", "minLength": 1 },
                "peer_id": { "type": "string", "minLength": 1 },
                "session_id": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        }),
        rtc_ids::SFU_HANDLE_SDP_OFFER => json!({
            "type": "object",
            "required": ["room_id", "peer_id", "sdp_offer"],
            "properties": {
                "room_id": { "type": "string", "minLength": 1 },
                "peer_id": { "type": "string", "minLength": 1 },
                "sdp_offer": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        }),
        rtc_ids::SFU_HANDLE_SDP_ANSWER => json!({
            "type": "object",
            "required": ["room_id", "peer_id", "sdp_answer"],
            "properties": {
                "room_id": { "type": "string", "minLength": 1 },
                "peer_id": { "type": "string", "minLength": 1 },
                "sdp_answer": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        }),
        rtc_ids::SFU_ADD_ICE_CANDIDATE => json!({
            "type": "object",
            "required": ["room_id", "peer_id", "candidate_json"],
            "properties": {
                "room_id": { "type": "string", "minLength": 1 },
                "peer_id": { "type": "string", "minLength": 1 },
                "candidate_json": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        }),
        rtc_ids::SFU_SET_SUBSCRIPTION => json!({
            "type": "object",
            "required": ["room_id", "subscriber_peer_id", "track_id", "enable", "priority"],
            "properties": {
                "room_id": { "type": "string", "minLength": 1 },
                "subscriber_peer_id": { "type": "string", "minLength": 1 },
                "track_id": { "type": "string", "minLength": 1 },
                "enable": { "type": "boolean" },
                "media": { "type": ["string", "null"], "minLength": 1 },
                "preferred_layer": { "type": ["string", "null"], "minLength": 1 },
                "priority": { "type": "integer", "minimum": 0 }
            },
            "additionalProperties": false
        }),
        rtc_ids::SFU_GET_ROOM_STATE => json!({
            "type": "object",
            "required": ["room_id"],
            "properties": {
                "room_id": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        }),
        _ => json!({
            "type": "object",
            "additionalProperties": false
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtc_output_schema_declares_dispatch_result_fields() {
        let join_token = rtc_output_schema(rtc_ids::CALL_JOIN_TOKEN);
        assert_required(
            &join_token,
            &["call_id", "sfu_join_token", "sfu_join_token_ttl_seconds"],
        );

        let join_room = rtc_output_schema(rtc_ids::SFU_JOIN_ROOM);
        assert_required(&join_room, &["room_id", "peer_id", "session_id", "call_id"]);

        let room_state = rtc_output_schema(rtc_ids::SFU_GET_ROOM_STATE);
        assert_required(&room_state, &["room_id", "exists", "revision"]);
        assert!(room_state["properties"].get("room_snapshot").is_some());
    }

    #[test]
    fn rtc_event_schema_declares_lifecycle_fields() {
        let invited = rtc_event_schema("rtc.call.invited");
        assert_required(
            &invited,
            &["call_id", "conversation_id", "caller_user_id", "media"],
        );

        let room_updated = rtc_event_schema("rtc.media.room.updated");
        assert_required(&room_updated, &["room_id", "revision"]);
        assert!(room_updated["properties"].get("joined_peer_ids").is_some());
        assert_eq!(room_updated["additionalProperties"], false);
    }

    fn assert_required(schema: &Value, expected: &[&str]) {
        let required = schema["required"]
            .as_array()
            .expect("schema must have required array");
        for key in expected {
            assert!(
                required.iter().any(|value| value.as_str() == Some(*key)),
                "missing required key {key}"
            );
            assert!(
                schema["properties"].get(*key).is_some(),
                "missing property for {key}"
            );
        }
    }
}
