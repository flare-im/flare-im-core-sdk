use serde_json::{Map, Value, json};

#[derive(Debug, Clone)]
pub struct NormalizedOperation {
    pub name: String,
    pub request: Value,
}

pub const MESSAGE_BUILD_OPS: &[&str] = &[
    "create_announcement",
    "create_audio",
    "create_card",
    "create_custom",
    "create_emoji",
    "create_file",
    "create_forward",
    "create_image",
    "create_image_group",
    "create_link_card",
    "create_location",
    "create_mini_program",
    "create_notification",
    "create_placeholder",
    "create_quote",
    "create_rich_doc",
    "create_schedule",
    "create_sticker",
    "create_system",
    "create_task",
    "create_text",
    "create_thread_reply",
    "create_video",
    "create_vote",
    "create_with_content",
];

pub fn message_build_catalog() -> Value {
    Value::Array(
        MESSAGE_BUILD_OPS
            .iter()
            .map(|op| {
                json!({
                    "op": op,
                    "method": build_method_name(op),
                    "stability": if *op == "create_image_group" { "beta" } else { "stable" }
                })
            })
            .collect(),
    )
}

pub fn normalize_operation(operation: &str, request: Value) -> NormalizedOperation {
    if let Some(op) = operation.strip_prefix("messages.build.") {
        return NormalizedOperation {
            name: "message.build".to_string(),
            request: with_default_op(request, &format!("create_{op}")),
        };
    }

    if let Some(op) = operation.strip_prefix("message_builder.") {
        if op == "list_catalog" {
            return NormalizedOperation {
                name: "message_builder.list_catalog".to_string(),
                request,
            };
        }
        return NormalizedOperation {
            name: "message.build".to_string(),
            request: with_default_op(request, op),
        };
    }

    match operation {
        "client.create" => alias("sdk.create", request),
        "client.release" => alias("sdk.dispose", request),
        "client.hard_reset" => alias("sdk.hard_reset", request),
        "client.init" => alias("sdk.init", request),
        "client.uninit" => alias("sdk.uninit", request),
        "client.login" => alias("sdk.login", request),
        "client.logout" => alias("sdk.logout", request),
        "client.update_access_token" => alias("sdk.update_access_token", request),
        "client.version" => alias("diagnostics.sdk_version", request),
        "client.ffi_contract_version" => alias("diagnostics.ffi_contract_version", request),
        "client.data_root" => alias("diagnostics.data_root", request),
        "client.is_connected" => alias("sdk.is_connected", request),
        "client.session_active" => alias("sdk.session_active", request),
        "client.current_user_id" => alias("sdk.current_user_id", request),
        "client.generate_test_token" => alias("sdk.generate_test_token", request),
        "client.state" => alias("connection.get_state", request),
        "client.disconnect" => alias("connection.disconnect", request),
        "events.subscribe" => alias("event.subscribe", request),
        "events.unsubscribe" => alias("event.unsubscribe", request),
        "events.unsubscribe_all" => alias("event.unsubscribe_all", request),
        "presence.get_user" => alias("presence.get", request),
        "presence.batch_get_user" => alias("presence.batch_get", request),
        "presence.subscribe_user" => alias("presence.subscribe", request),
        "sync.set_conversation_input_state" => alias("message.typing", request),
        "messages.create_text_direct" => alias("message.create_text", request),
        "media.get_file_url" => alias("media.get_url", request),
        "capabilities.list" => alias("capability.list", request),
        "capabilities.list_user" => alias("capability.list_user", request),
        "capabilities.dispatch" => alias("capability.dispatch", request),
        "capabilities.grant" => alias("capability.grant", request),
        "capabilities.revoke" => alias("capability.revoke", request),
        "call.send_signal" => alias("capability.send_call_signal", request),
        "rich_doc_v2.create_message" => NormalizedOperation {
            name: "message.build".to_string(),
            request: with_default_op(request, "create_rich_doc"),
        },
        "rich_doc_v2.edit_message" => alias("message.edit_rich_doc_by_message_id", request),
        op if op.starts_with("messages.") => alias_owned(
            "message",
            op.strip_prefix("messages.").unwrap_or_default(),
            request,
        ),
        op if op.starts_with("conversations.") => alias_owned(
            "conversation",
            op.strip_prefix("conversations.").unwrap_or_default(),
            request,
        ),
        op if op.starts_with("rtc.") => NormalizedOperation {
            name: "capability.dispatch".to_string(),
            request: with_default_field(request, "operation", op),
        },
        _ => alias_dynamic(operation, request),
    }
}

fn alias(name: &'static str, request: Value) -> NormalizedOperation {
    NormalizedOperation {
        name: name.to_string(),
        request,
    }
}

fn alias_owned(prefix: &'static str, suffix: &str, request: Value) -> NormalizedOperation {
    alias_dynamic(&format!("{prefix}.{suffix}"), request)
}

fn alias_dynamic(operation: &str, request: Value) -> NormalizedOperation {
    NormalizedOperation {
        name: operation.to_string(),
        request,
    }
}

fn with_default_op(request: Value, op: &str) -> Value {
    with_default_field(request, "op", op)
}

fn with_default_field(request: Value, key: &str, value: &str) -> Value {
    match request {
        Value::Object(mut object) => {
            object
                .entry(key.to_string())
                .or_insert_with(|| Value::String(value.to_string()));
            Value::Object(object)
        }
        Value::Null => {
            let mut object = Map::new();
            object.insert(key.to_string(), Value::String(value.to_string()));
            Value::Object(object)
        }
        other => json!({ key: value, "payload": other }),
    }
}

fn build_method_name(op: &str) -> String {
    let suffix = op.strip_prefix("create_").unwrap_or(op);
    let mut method = String::from("build");
    let mut uppercase_next = true;
    for ch in suffix.chars() {
        if ch == '_' {
            uppercase_next = true;
            continue;
        }
        if uppercase_next {
            method.extend(ch.to_uppercase());
            uppercase_next = false;
        } else {
            method.push(ch);
        }
    }
    method
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_c_lifecycle_contract_to_web_runtime_ops() {
        let op = normalize_operation("client.login", json!({ "userId": "u1" }));
        assert_eq!(op.name, "sdk.login");
        assert_eq!(op.request["userId"], "u1");
    }

    #[test]
    fn maps_c_message_build_contract_to_dispatch_request() {
        let op = normalize_operation("messages.build.quote", json!({ "conversationId": "c1" }));
        assert_eq!(op.name, "message.build");
        assert_eq!(op.request["op"], "create_quote");
    }

    #[test]
    fn maps_generated_message_builder_ops_to_dispatch_request() {
        let op = normalize_operation("message_builder.create_image_group", Value::Null);
        assert_eq!(op.name, "message.build");
        assert_eq!(op.request["op"], "create_image_group");
    }
}
