use anyhow::{Context, Result};
use flare_im_core_sdk::model::{
    Conversation, HomeTimelineSnapshot, TimelineSyncState, ViewDelta, ViewDeltaKind, ViewUpdate,
    ViewUpdateKind,
};
use serde_json::{Value, json};
use std::path::Path;

use crate::{emit_errors, fail, load_json, spec_dir};

pub(crate) fn verify_golden_contracts(root: &Path) -> Result<()> {
    let mut errors = Vec::new();
    let golden = spec_dir(root).join("golden");

    let conversation_value = load_json(&golden.join("responses/conversation_get_one.json"))?;
    let conversation: Conversation = serde_json::from_value(conversation_value.clone())
        .context("decode golden conversation_get_one.json as Rust Conversation")?;
    if conversation.conversation_id != "single-u2" {
        fail(
            &mut errors,
            "conversation_get_one.json conversationId drifted",
        );
    }
    let conversation_roundtrip = serde_json::to_value(&conversation)
        .context("encode golden conversation_get_one.json from Rust Conversation")?;
    assert_json_field(
        &mut errors,
        &conversation_roundtrip,
        "conversationType",
        json!("single"),
        "Rust Conversation must serialize conversationType as a serde string",
    );

    let home_value = load_json(&golden.join("responses/home_timeline_snapshot.json"))?;
    let home: HomeTimelineSnapshot = serde_json::from_value(home_value.clone())
        .context("decode golden home_timeline_snapshot.json as Rust HomeTimelineSnapshot")?;
    if home.sync_state != TimelineSyncState::Synced {
        fail(
            &mut errors,
            "home_timeline_snapshot.json syncState must decode as synced",
        );
    }
    let home_roundtrip =
        serde_json::to_value(&home).context("encode home_timeline_snapshot.json from Rust")?;
    assert_json_field(
        &mut errors,
        &home_roundtrip,
        "syncState",
        json!("synced"),
        "Rust HomeTimelineSnapshot must serialize syncState as a serde string",
    );
    let first_conversation = home_roundtrip
        .get("conversations")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .unwrap_or(&Value::Null);
    assert_json_field(
        &mut errors,
        first_conversation,
        "conversationType",
        json!("group"),
        "home_timeline_snapshot.json first conversationType must serialize as group",
    );

    let view_update_value = load_json(&golden.join("events/view_update_delta.json"))?;
    let view_update: ViewUpdate = serde_json::from_value(view_update_value.clone())
        .context("decode golden view_update_delta.json as Rust ViewUpdate")?;
    if view_update.kind != ViewUpdateKind::Delta {
        fail(
            &mut errors,
            "view_update_delta.json kind must decode as delta",
        );
    }
    match &view_update.delta {
        Some(ViewDelta::Timeline { ops, has_more, .. }) => {
            if *has_more {
                fail(
                    &mut errors,
                    "view_update_delta.json timeline hasMore must decode as false",
                );
            }
            if ops.first().map(|op| &op.op) != Some(&ViewDeltaKind::Insert) {
                fail(
                    &mut errors,
                    "view_update_delta.json first op must decode as insert",
                );
            }
            let first_item = ops.first().and_then(|op| op.item.as_ref());
            assert_json_field(
                &mut errors,
                first_item.unwrap_or(&Value::Null),
                "timelineKey",
                json!("client:cm1"),
                "view_update_delta.json item.timelineKey drifted",
            );
        }
        _ => fail(
            &mut errors,
            "view_update_delta.json must decode as timeline delta",
        ),
    }
    let view_update_roundtrip =
        serde_json::to_value(&view_update).context("encode view_update_delta.json from Rust")?;
    if view_update_roundtrip != view_update_value {
        fail(
            &mut errors,
            "view_update_delta.json must roundtrip exactly through Rust ViewUpdate serde",
        );
    }

    emit_errors("golden-contract", errors)
}

fn assert_json_field(
    errors: &mut Vec<String>,
    value: &Value,
    field: &str,
    expected: Value,
    message: &'static str,
) {
    if value.get(field) != Some(&expected) {
        fail(errors, message);
    }
}
