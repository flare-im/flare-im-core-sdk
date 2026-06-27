use serde_json::Value;
use std::collections::BTreeSet;

use crate::{all_spec_enums, all_spec_models, str_field};

pub(crate) fn pascal_case(value: &str) -> String {
    value
        .replace('-', "_")
        .split('_')
        .map(upper_first)
        .collect::<Vec<_>>()
        .join("")
}

pub(crate) fn listener_interface_name(kind: &str) -> String {
    format!("{}EventListener", pascal_case(kind))
}

pub(crate) fn model_package_suffix(name: &str) -> &'static str {
    if matches!(
        name,
        "ConversationType" | "MessageContentType" | "MessageSearchKind" | "SdkConnectionState"
    ) {
        "common.enums"
    } else if name == "SdkErrorPayload" {
        "common.error"
    } else if matches!(
        name,
        "MessageBuildCatalog" | "MessageBuildCatalogEntry" | "MessageBuildOp"
    ) {
        "catalog"
    } else if name == "MediaSourceInfo" {
        "media"
    } else if name.ends_with("ContentPayload")
        || matches!(name, "ImageGroupItem" | "ForwardSourceMessage")
    {
        "content"
    } else if matches!(
        name,
        "Conversation"
            | "ConversationParticipant"
            | "Message"
            | "MessageContent"
            | "MessageLocalState"
            | "MessagePreview"
            | "ReactionEntry"
    ) {
        "entity"
    } else if name.starts_with("Build") && name.ends_with("MessageRequest") {
        "command.message.build"
    } else if matches!(name, "CreateTextMessageRequest" | "SendMessageRequest") {
        "command.message"
    } else if matches!(
        name,
        "ConversationListQuery" | "ListMessagesRequest" | "MessageSearchQuery"
    ) || name.ends_with("Query")
    {
        "query"
    } else if name.ends_with("Request") || name.ends_with("Command") {
        "command"
    } else if name.ends_with("Response") {
        "response"
    } else if matches!(name, "SdkEventEnvelope" | "SdkEventKind") {
        "event"
    } else if name.starts_with("LifecycleEvent") {
        "event.lifecycle"
    } else if name.starts_with("ConnectionEvent") {
        "event.connection"
    } else if matches!(
        name,
        "MessageEventName"
            | "MessageMutationEvent"
            | "MessageReceivedBatchEvent"
            | "MessageReceivedEvent"
            | "MessageSendAckEvent"
            | "MessageSendFailedEvent"
            | "ReadReceiptEvent"
            | "ReactionChangedEvent"
            | "TypingEvent"
    ) {
        "event.message"
    } else if name.starts_with("ConversationEvent") {
        "event.conversation"
    } else if name.starts_with("SyncEvent") {
        "event.sync"
    } else if name.starts_with("ProgressEvent") {
        "event.progress"
    } else if name == "PresenceChangedEvent" {
        "event.presence"
    } else if name.starts_with("CapabilityEvent") {
        "event.capability"
    } else {
        "entity"
    }
}

pub(crate) fn kotlin_model_package_imports(spec: &Value) -> Vec<String> {
    let suffixes = all_spec_enums(spec)
        .into_iter()
        .map(|enum_value| model_package_suffix(str_field(enum_value, "name")).to_string())
        .chain(
            all_spec_models(spec)
                .into_iter()
                .map(|model| model_package_suffix(str_field(model, "name")).to_string()),
        )
        .chain(["catalog".to_string()])
        .collect::<BTreeSet<_>>();
    suffixes
        .into_iter()
        .map(|suffix| format!("import com.flare.im.model.{suffix}.*"))
        .collect()
}

pub(crate) fn facade_prop(module: &Value) -> &str {
    str_field(module, "facade")
        .split_once('.')
        .map(|(_, prop)| prop)
        .unwrap_or("")
}

pub(crate) fn ts_api_interface_name(module: &Value) -> String {
    if str_field(module, "facade") == "client" {
        "SessionApi".to_string()
    } else {
        format!("{}Api", pascal_case(facade_prop(module)))
    }
}

pub(crate) fn ts_api_module_key(module: &Value) -> &str {
    if str_field(module, "facade") == "client" {
        "session"
    } else {
        str_field(module, "key")
    }
}

pub(crate) fn ts_model_to_map_fn(model_name: &str) -> String {
    format!("{}ToMap", lower_first(model_name))
}

pub(crate) fn ts_model_from_json_fn(model_name: &str) -> String {
    format!("{}FromJson", lower_first(model_name))
}

pub(crate) fn camel_const(value: &str) -> String {
    let normalized = value.replace(['-', '.'], "_");
    let mut parts = normalized.split('_');
    let Some(first) = parts.next() else {
        return String::new();
    };
    format!(
        "{}{}",
        first,
        parts.map(upper_first).collect::<Vec<_>>().join("")
    )
}

pub(crate) fn snake_case(value: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for ch in value.chars() {
        match ch {
            '-' | '.' | ' ' | '_' => {
                if !out.is_empty() && !out.ends_with('_') {
                    out.push('_');
                }
                prev_lower = false;
            }
            _ if ch.is_ascii_uppercase() && prev_lower => {
                out.push('_');
                out.push(ch.to_ascii_lowercase());
                prev_lower = false;
            }
            _ => {
                out.push(ch.to_ascii_lowercase());
                prev_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
            }
        }
    }
    out.trim_matches('_').to_string()
}

pub(crate) fn screaming_snake(value: &str) -> String {
    snake_case(value).to_ascii_uppercase()
}

fn upper_first(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    format!("{}{}", first.to_ascii_uppercase(), chars.as_str())
}

pub(crate) fn lower_first(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    format!("{}{}", first.to_ascii_lowercase(), chars.as_str())
}

pub(crate) fn json_quote(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization cannot fail")
}

pub(crate) fn swift_identifier(name: &str) -> String {
    if matches!(
        name,
        "init"
            | "deinit"
            | "class"
            | "struct"
            | "protocol"
            | "extension"
            | "func"
            | "internal"
            | "public"
            | "private"
            | "fileprivate"
            | "open"
    ) {
        format!("`{name}`")
    } else {
        name.to_string()
    }
}

pub(crate) fn cangjie_identifier(name: &str) -> String {
    if matches!(
        name,
        "init" | "class" | "interface" | "enum" | "func" | "let" | "var" | "public"
    ) {
        if name == "init" {
            "initSdk".to_string()
        } else {
            format!("{name}Value")
        }
    } else {
        name.to_string()
    }
}
