use flare_im_core_sdk::model::conversation::Conversation;
use flare_im_core_sdk::model::message::IMMessage;
use flare_im_core_sdk::model::message_elem::{Elem, TextElem};
use serde_json::{Value, json};

/// Serialize core models with canonical snake_case JSON (matches C ABI / contract).
pub fn conversation_to_json(conversation: &Conversation) -> Value {
    serde_json::to_value(conversation).unwrap_or(Value::Null)
}

pub fn conversations_to_json(conversations: &[Conversation]) -> Value {
    Value::Array(conversations.iter().map(conversation_to_json).collect())
}

pub fn message_to_json(message: &IMMessage) -> Value {
    let mut value = serde_json::to_value(message).unwrap_or(Value::Null);
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
            json!({ "content_type": "text", "data": { "text": text } })
        }
        Some(Elem::Emoji(value)) => {
            json!({ "content_type": "emoji", "data": serde_json::to_value(value).unwrap_or(Value::Null) })
        }
        Some(Elem::Sticker(value)) => {
            json!({ "content_type": "sticker", "data": serde_json::to_value(value).unwrap_or(Value::Null) })
        }
        Some(elem) => json!({
            "content_type": "custom",
            "data": serde_json::to_value(elem).unwrap_or(Value::Null)
        }),
        None => json!({ "content_type": "custom", "data": {} }),
    }
}
