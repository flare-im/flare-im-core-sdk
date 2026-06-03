use flare_im_core_sdk::model::conversation::Conversation;
use flare_im_core_sdk::model::message::IMMessage;
use flare_im_core_sdk::model::message_elem::{Elem, TextElem};
use serde::Serialize;
use serde_json::{Map, Value, json};

pub fn conversation_to_json(conversation: &Conversation) -> Value {
    camelize_value(serde_json::to_value(conversation).unwrap_or(Value::Null))
}

pub fn conversations_to_json(conversations: &[Conversation]) -> Value {
    Value::Array(conversations.iter().map(conversation_to_json).collect())
}

pub fn message_to_json(message: &IMMessage) -> Value {
    let mut value = camelize_value(serde_json::to_value(message).unwrap_or(Value::Null));
    if let Value::Object(ref mut object) = value {
        object.insert(
            "content".to_string(),
            message_content_to_json(message.content.as_ref()),
        );
    }
    value
}

pub fn messages_to_json(messages: &[IMMessage]) -> Value {
    Value::Array(messages.iter().map(message_to_json).collect())
}

pub fn content_text(content: Option<&Elem>) -> String {
    match content {
        Some(Elem::Text(value)) => value.text.clone(),
        Some(Elem::Emoji(value)) => value.emoji.clone(),
        Some(Elem::Sticker(value)) => value.sticker_id.clone(),
        Some(Elem::Custom(value)) => value.description.clone(),
        Some(Elem::Placeholder(value)) => value.fallback_text.clone(),
        Some(elem) => serde_json::to_string(elem).unwrap_or_default(),
        None => String::new(),
    }
}

fn message_content_to_json(content: Option<&Elem>) -> Value {
    match content {
        Some(Elem::Text(TextElem { text, .. })) => {
            json!({ "contentType": "text", "data": { "text": text } })
        }
        Some(Elem::Emoji(value)) => {
            json!({ "contentType": "emoji", "data": camelize_serializable(value) })
        }
        Some(Elem::Sticker(value)) => {
            json!({ "contentType": "sticker", "data": camelize_serializable(value) })
        }
        Some(elem) => json!({ "contentType": "custom", "data": camelize_serializable(elem) }),
        None => json!({ "contentType": "custom", "data": {} }),
    }
}

fn camelize_serializable(value: impl Serialize) -> Value {
    camelize_value(serde_json::to_value(value).unwrap_or(Value::Null))
}

fn camelize_value(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(camelize_value).collect()),
        Value::Object(object) => Value::Object(camelize_object(object)),
        other => other,
    }
}

fn camelize_object(object: Map<String, Value>) -> Map<String, Value> {
    object
        .into_iter()
        .map(|(key, value)| (to_lower_camel(&key), camelize_value(value)))
        .collect()
}

fn to_lower_camel(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut uppercase_next = false;
    for ch in value.chars() {
        if ch == '_' {
            uppercase_next = true;
            continue;
        }
        if uppercase_next {
            output.extend(ch.to_uppercase());
            uppercase_next = false;
        } else {
            output.push(ch);
        }
    }
    output
}
