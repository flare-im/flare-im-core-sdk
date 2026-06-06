use serde_json::{Map, Value, json};

use crate::contract::MESSAGE_BUILD_OPS;

#[derive(Debug, Clone)]
pub struct NormalizedOperation {
    pub name: String,
    pub request: Value,
}

pub fn message_build_catalog() -> Value {
    Value::Array(
        MESSAGE_BUILD_OPS
            .iter()
            .map(|entry| {
                json!({
                    "op": entry.op,
                    "method": entry.method,
                    "stability": entry.stability,
                    "source_operation": entry.source_operation
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

    if operation == "message_builder.list_catalog" {
        return NormalizedOperation {
            name: "message_builder.list_catalog".to_string(),
            request,
        };
    }

    match operation {
        "events.subscribe" => alias("event.subscribe", request),
        "events.unsubscribe" => alias("event.unsubscribe", request),
        "events.unsubscribe_all" => alias("event.unsubscribe_all", request),
        "sync.conversation" => alias("sync.conversation", request),
        "sync.messages" => alias("sync.messages", request),
        "sync.mark_session_read" => alias("sync.mark_session_read", request),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_removed_client_aliases_unmapped() {
        let op = normalize_operation("client.login", json!({ "user_id": "u1" }));
        assert_eq!(op.name, "client.login");
        assert_eq!(op.request["user_id"], "u1");
    }

    #[test]
    fn maps_c_message_build_contract_to_dispatch_request() {
        let op = normalize_operation("messages.build.quote", json!({ "conversation_id": "c1" }));
        assert_eq!(op.name, "message.build");
        assert_eq!(op.request["op"], "create_quote");
    }

    #[test]
    fn keeps_message_builder_catalog_route_only() {
        let op = normalize_operation("message_builder.list_catalog", Value::Null);
        assert_eq!(op.name, "message_builder.list_catalog");

        let op = normalize_operation("message_builder.create_text", Value::Null);
        assert_eq!(op.name, "message_builder.create_text");
    }

    #[test]
    fn maps_sync_contract_ids_to_direct_routes() {
        let op = normalize_operation("sync.messages", json!({ "conversation_id": "c1" }));
        assert_eq!(op.name, "sync.messages");
        let op = normalize_operation("connection.get_state", Value::Null);
        assert_eq!(op.name, "connection.get_state");
    }
}
