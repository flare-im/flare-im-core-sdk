use anyhow::Result;
use std::{fs, path::Path};

use crate::{emit_errors, fail};

pub(crate) fn verify_rtc_capability_gate(root: &Path) -> Result<()> {
    let mut errors = Vec::new();
    let monorepo_root = root.parent().unwrap_or_else(|| Path::new(".."));
    let sdk_root = monorepo_root.join("flare-im-core-sdk");
    let plugin_root = monorepo_root.join("flare-sdk-plugin");
    let server_plugin_root = monorepo_root.join("flare-plugin/flare-strom-sfu");

    require_contains_all(
        &mut errors,
        &root.join("docs/rtc-capability.md"),
        "RTC capability doc",
        &[
            "rtc.call.audio",
            "rtc.media.join",
            "RtcSfuSubscriptionRequest",
            "inputSchema",
            "outputSchema",
            "typed events",
            "sfu_join_token",
            "room_snapshot",
            "flare-strom-sfu",
        ],
    );

    require_contains_all(
        &mut errors,
        &sdk_root.join("src/extension/capability/rtc_ids.rs"),
        "RTC capability ids",
        &[
            "pub const CALL_AUDIO",
            "pub const CALL_VIDEO",
            "pub const SFU_JOIN_ROOM",
            "pub const SFU_HANDLE_SDP_OFFER",
            "pub const SFU_ADD_ICE_CANDIDATE",
            "pub const SFU_SET_SUBSCRIPTION",
            "pub const SFU_GET_ROOM_STATE",
        ],
    );
    require_contains_all(
        &mut errors,
        &sdk_root.join("src/client/api/capability.rs"),
        "RTC client capability API",
        &[
            "pub struct RtcSfuSubscriptionRequest",
            "pub async fn rtc_start_audio",
            "pub async fn rtc_start_video",
            "pub async fn rtc_sfu_join_room",
            "pub async fn rtc_sfu_handle_sdp_offer",
            "pub async fn rtc_sfu_set_subscription",
            "pub async fn rtc_sfu_get_room_state",
        ],
    );
    require_contains_all(
        &mut errors,
        &sdk_root.join("src/extension/capability/plugins/call_av.rs"),
        "RTC SDK AV plugin",
        &[
            "const CALL_AV_PLUGIN_ID",
            "const RTC_CAPABILITY_NAMESPACE",
            "SdkPluginManifest::builtin",
            "rtc_operation(rtc_ids::SFU_JOIN_ROOM",
            "rtc_input_schema",
            "rtc_output_schema",
            "rtc_event_schema",
            "input_schema = Some",
            "output_schema = Some",
            "sdp_offer",
            "sfu_join_token",
            "sfu_join_token_ttl_seconds",
            "candidate_json",
            "subscriber_peer_id",
            "room_snapshot",
            "rtc.media.room.updated",
            "caller_user_id",
            "accepted_by_user_id",
            "ended_by_user_id",
            "joined_peer_ids",
            "rtc.media",
        ],
    );

    require_contains_all(
        &mut errors,
        &plugin_root.join("flare-sdk-plugin-call/plugin.json"),
        "RTC plugin manifest",
        &[
            "\"op\": \"call.audio\"",
            "\"op\": \"media.join\"",
            "\"op\": \"media.sdp.offer\"",
            "\"op\": \"media.ice.candidate\"",
            "\"op\": \"media.subscription.set\"",
            "\"op\": \"media.room.state\"",
            "\"inputSchema\"",
            "\"outputSchema\"",
            "\"schema\"",
            "\"call_id\"",
            "\"room_id\"",
            "\"caller_user_id\"",
            "\"accepted_by_user_id\"",
            "\"ended_by_user_id\"",
            "\"joined_peer_ids\"",
            "\"peer_id\"",
            "\"session_id\"",
            "\"sfu_join_token\"",
            "\"sfu_join_token_ttl_seconds\"",
            "\"sdp_offer\"",
            "\"sdp_answer\"",
            "\"accepted\"",
            "\"applied\"",
            "\"exists\"",
            "\"revision\"",
            "\"candidate_json\"",
            "\"subscriber_peer_id\"",
            "\"room_snapshot\"",
            "\"uiKits\"",
        ],
    );
    require_contains_all(
        &mut errors,
        &plugin_root.join("generated/sdk_plugin_av/typescript/pluginApi.ts"),
        "RTC generated TypeScript plugin API",
        &["rtc.call.audio", "rtc.media.join", "rtc.media.room.state"],
    );

    require_contains_all(
        &mut errors,
        &server_plugin_root.join("README.md"),
        "strom SFU README",
        &[
            "WebSocket JSON 信令",
            "str0m",
            "GET /api/metrics",
            "GET /api/debug/rooms",
        ],
    );
    require_contains_all(
        &mut errors,
        &server_plugin_root.join("Cargo.toml"),
        "strom SFU crate",
        &["name = \"flare-strom-sfu\"", "default-run", "str0m"],
    );

    emit_errors("rtc capability gate", errors)
}

fn require_contains_all(errors: &mut Vec<String>, path: &Path, label: &str, needles: &[&str]) {
    let Ok(text) = fs::read_to_string(path) else {
        fail(
            errors,
            format!("{label} missing or unreadable: {}", path.display()),
        );
        return;
    };

    for needle in needles {
        if !text.contains(needle) {
            fail(
                errors,
                format!("{label} missing `{needle}` in {}", path.display()),
            );
        }
    }
}
