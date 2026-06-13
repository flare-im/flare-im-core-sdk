//! Shared helpers for generated JSON dispatch modules.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

use crate::{BindingResponse, binding_invalid_parameter};
use flare_im_core_sdk::Result;
use flare_im_core_sdk::client::HeartbeatAppState;
use flare_im_core_sdk::client::api::{
    CreateLocationRequest, CreateRichDocRequest, CreateStickerRequest, EditRichDocRequest,
};
use flare_im_core_sdk::model::content_builder::BuiltContent;
use flare_im_core_sdk::model::conversation::ConversationType;
use flare_im_core_sdk::model::media::UploadOptions;
use flare_im_core_sdk::model::message::{IMMessage, MarkType, SendAck};
use flare_im_core_sdk::model::message_elem::{Elem, elem_to_message_content};
use flare_proto::common::MessageType;

pub fn json<T: serde::Serialize>(value: T) -> Result<BindingResponse> {
    serde_json::to_value(value)
        .map(BindingResponse::json)
        .map_err(|e| {
            binding_invalid_parameter(format!("failed to serialize binding response: {e}"))
        })
}

pub fn json_send_ack(ack: SendAck) -> Result<BindingResponse> {
    let (server_msg_id, seq, timestamp, success, error_code, error_message) =
        match ack.result.as_ref() {
            Some(flare_proto::common::send_ack::Result::Accepted(accepted)) => (
                accepted.server_msg_id.clone(),
                accepted.conversation_seq,
                accepted.server_time,
                true,
                0,
                String::new(),
            ),
            Some(flare_proto::common::send_ack::Result::Error(error)) => (
                String::new(),
                0,
                0,
                false,
                error.code,
                error.message.clone(),
            ),
            None => (
                String::new(),
                0,
                0,
                false,
                0,
                "missing send ack result".to_string(),
            ),
        };
    Ok(BindingResponse::json(serde_json::json!({
        "clientMsgId": ack.client_msg_id,
        "serverId": server_msg_id,
        "seq": seq,
        "conversationId": ack.conversation_id,
        "ackId": ack.ack_id,
        "timestamp": timestamp,
        "success": success,
        "errorCode": error_code,
        "errorMessage": error_message,
    })))
}

pub fn conversation_id(value: &Value) -> Result<String> {
    json_string(value, "conversationId")
}

pub fn json_string(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| binding_invalid_parameter(format!("missing or invalid JSON field: {key}")))
}

pub fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

pub fn string_any(value: &Value, keys: &[&str]) -> Result<String> {
    keys.iter()
        .find_map(|key| {
            value
                .get(*key)
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .ok_or_else(|| {
            binding_invalid_parameter(format!("missing or invalid JSON field: {}", keys.join("/")))
        })
}

pub fn optional_string_any(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    })
}

pub fn json_u64(value: &Value, key: &str) -> Result<u64> {
    value
        .get(key)
        .and_then(|x| x.as_u64().or_else(|| x.as_i64().map(|i| i as u64)))
        .ok_or_else(|| binding_invalid_parameter(format!("missing or invalid JSON field: {key}")))
}

pub fn json_i64(value: &Value, key: &str) -> Result<i64> {
    value
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| binding_invalid_parameter(format!("missing or invalid JSON field: {key}")))
}

pub fn json_i32(value: &Value, key: &str) -> Result<i32> {
    value
        .get(key)
        .and_then(Value::as_i64)
        .map(|i| i as i32)
        .ok_or_else(|| binding_invalid_parameter(format!("missing or invalid JSON field: {key}")))
}

pub fn json_f64(value: &Value, key: &str) -> Result<f64> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| binding_invalid_parameter(format!("missing or invalid JSON field: {key}")))
}

pub fn json_bool(value: &Value, key: &str) -> Result<bool> {
    value
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| binding_invalid_parameter(format!("missing or invalid JSON field: {key}")))
}

pub fn optional_bool(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(Value::as_bool)
}

pub fn optional_upload_options(value: &Value, key: &str) -> Result<Option<UploadOptions>> {
    let Some(raw) = value.get(key) else {
        return Ok(None);
    };
    if raw.is_null() {
        return Ok(None);
    }
    let Some(chunk_size) = raw.get("chunkSize").and_then(|v| v.as_u64()) else {
        return Ok(None);
    };
    let chunk_size = usize::try_from(chunk_size).map_err(|_| {
        binding_invalid_parameter(format!("invalid upload options field: {key}.chunkSize"))
    })?;
    if chunk_size == 0 {
        return Err(binding_invalid_parameter(format!(
            "invalid upload options field: {key}.chunkSize"
        )));
    }
    Ok(Some(UploadOptions { chunk_size }))
}

pub fn json_bytes_vec(value: &Value, key: &str) -> Result<Vec<u8>> {
    let arr = value.get(key).and_then(Value::as_array).ok_or_else(|| {
        binding_invalid_parameter(format!("missing or invalid JSON field: {key}"))
    })?;
    arr.iter()
        .map(|v| {
            v.as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .ok_or_else(|| {
                    binding_invalid_parameter(format!("invalid JSON bytes field: {key}"))
                })
        })
        .collect()
}

pub fn json_vec_string(value: &Value, key: &str) -> Result<Vec<String>> {
    let arr = value.get(key).and_then(Value::as_array).ok_or_else(|| {
        binding_invalid_parameter(format!("missing or invalid JSON field: {key}"))
    })?;
    arr.iter()
        .map(|v| {
            v.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                binding_invalid_parameter(format!("invalid JSON array field: {key}"))
            })
        })
        .collect()
}

pub fn json_vec_message(value: &Value, key: &str) -> Result<Vec<IMMessage>> {
    let arr = value.get(key).and_then(Value::as_array).ok_or_else(|| {
        binding_invalid_parameter(format!("missing or invalid JSON field: {key}"))
    })?;
    arr.iter().map(|v| from_value(v.clone(), key)).collect()
}

pub fn optional_value<T: for<'de> Deserialize<'de>>(value: &Value, key: &str) -> Result<Option<T>> {
    value
        .get(key)
        .cloned()
        .map(|v| from_value(v, key))
        .transpose()
}

pub fn optional_u32(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
}

pub fn optional_i32(value: &Value, key: &str) -> Option<i32> {
    value
        .get(key)
        .and_then(Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
}

pub fn heartbeat_app_state(value: &Value, key: &str) -> Result<HeartbeatAppState> {
    let raw = value.get(key).ok_or_else(|| {
        binding_invalid_parameter(format!("missing or invalid JSON field: {key}"))
    })?;

    if let Some(text) = raw.as_str() {
        return match text {
            "foreground" => Ok(HeartbeatAppState::Foreground),
            "background" => Ok(HeartbeatAppState::Background),
            other => Err(binding_invalid_parameter(format!(
                "invalid app_state: {other}"
            ))),
        };
    }

    if let Some(index) = raw.as_u64() {
        return match index {
            0 => Ok(HeartbeatAppState::Foreground),
            1 => Ok(HeartbeatAppState::Background),
            other => Err(binding_invalid_parameter(format!(
                "invalid app_state index: {other}"
            ))),
        };
    }

    Err(binding_invalid_parameter(
        "app_state must be a string or enum index",
    ))
}

pub fn message_from_params(params: &Value) -> Result<IMMessage> {
    serde_json::from_value(params.clone())
        .or_else(|_| {
            params
                .get("message")
                .cloned()
                .ok_or_else(|| serde_json::Error::io(std::io::Error::other("missing message")))
                .and_then(serde_json::from_value)
        })
        .map_err(|_| binding_invalid_parameter("missing or invalid JSON message"))
}

pub fn from_value<T: for<'de> Deserialize<'de>>(value: Value, label: &str) -> Result<T> {
    serde_json::from_value(value)
        .map_err(|e| binding_invalid_parameter(format!("invalid {label}: {e}")))
}

pub fn parse_mark_type(value: i32) -> MarkType {
    match value {
        1 => MarkType::Important,
        2 => MarkType::Todo,
        3 => MarkType::Done,
        _ => MarkType::Custom,
    }
}

pub fn conversation_type(value: &Value) -> Result<ConversationType> {
    let Some(raw) = value.get("conversationType") else {
        return Err(binding_invalid_parameter(
            "missing or invalid JSON field: conversationType",
        ));
    };
    if let Some(v) = raw.as_i64() {
        return Ok(ConversationType::from_proto_int(v as i32));
    }
    if let Some(v) = raw.as_str() {
        return Ok(ConversationType::from(v));
    }
    Err(binding_invalid_parameter(
        "missing or invalid JSON field: conversationType",
    ))
}

pub fn built_content_from_value(value: &Value) -> Result<BuiltContent> {
    let elem: Elem = from_value(value.clone(), "built content")?;
    let message_type = message_type_for_elem(&elem);
    Ok(BuiltContent::new(
        message_type,
        elem_to_message_content(&elem),
    ))
}

fn message_type_for_elem(elem: &Elem) -> MessageType {
    match elem {
        Elem::Text(_) => MessageType::Text,
        Elem::Image(_) => MessageType::Image,
        Elem::Video(_) => MessageType::Video,
        Elem::Audio(_) => MessageType::Audio,
        Elem::File(_) => MessageType::File,
        Elem::Location(_) => MessageType::Location,
        Elem::Card(_) => MessageType::Card,
        Elem::Sticker(_) => MessageType::Sticker,
        Elem::Emoji(_) => MessageType::Emoji,
        Elem::Quote(_) => MessageType::Quote,
        Elem::LinkCard(_) => MessageType::LinkCard,
        Elem::Forward(_) => MessageType::Forward,
        Elem::Thread(_) => MessageType::Thread,
        Elem::MiniProgram(_)
        | Elem::Vote(_)
        | Elem::Task(_)
        | Elem::Schedule(_)
        | Elem::Announcement(_) => MessageType::AppCard,
        Elem::RichText(_) => MessageType::RichText,
        Elem::ImageGroup(_) => MessageType::ImageGroup,
        Elem::System(_) => MessageType::System,
        Elem::Notification(_) => MessageType::Notification,
        Elem::Custom(_) => MessageType::Custom,
        Elem::Placeholder(_) => MessageType::Placeholder,
    }
}

pub fn build_create_location_request(params: Value) -> Result<CreateLocationRequest> {
    Ok(CreateLocationRequest {
        conversation_id: conversation_id(&params)?,
        longitude: json_f64(&params, "longitude")?,
        latitude: json_f64(&params, "latitude")?,
        address: optional_string(&params, "address").unwrap_or_default(),
        title: optional_string(&params, "title").unwrap_or_default(),
        zoom: params
            .get("zoom")
            .and_then(Value::as_u64)
            .map(|z| z.min(255) as u8),
        snapshot_url: optional_string(&params, "snapshotUrl"),
        snapshot_local_path: optional_string(&params, "snapshotLocalPath"),
    })
}

pub fn build_create_sticker_request(params: Value) -> Result<CreateStickerRequest> {
    Ok(CreateStickerRequest {
        conversation_id: conversation_id(&params)?,
        sticker_id: json_string(&params, "stickerId")?,
        package_id: optional_string(&params, "packageId"),
        url: optional_string(&params, "url"),
        width: params
            .get("width")
            .and_then(Value::as_i64)
            .map(|i| i as i32),
        height: params
            .get("height")
            .and_then(Value::as_i64)
            .map(|i| i as i32),
        sticker_format: optional_string(&params, "stickerFormat"),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditRichDocJson {
    message_id: String,
    doc_json: String,
    content_schema: String,
    plain_text: String,
    input_format: Option<String>,
    input_format_version: Option<i32>,
    source_payload: Option<HashMap<String, String>>,
    title: Option<String>,
    search_text: Option<String>,
    render_hints_json: Option<String>,
}

impl From<EditRichDocJson> for EditRichDocRequest {
    fn from(value: EditRichDocJson) -> Self {
        Self {
            message_id: value.message_id,
            doc_json: value.doc_json,
            content_schema: value.content_schema,
            plain_text: value.plain_text,
            input_format: value.input_format,
            input_format_version: value.input_format_version,
            source_payload: value.source_payload,
            title: value.title,
            search_text: value.search_text,
            render_hints_json: value.render_hints_json,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRichDocJson {
    conversation_id: String,
    doc_json: String,
    content_schema: String,
    plain_text: String,
    input_format: Option<String>,
    input_format_version: Option<i32>,
    source_payload: Option<HashMap<String, String>>,
    title: Option<String>,
    search_text: Option<String>,
    render_hints_json: Option<String>,
}

impl From<CreateRichDocJson> for CreateRichDocRequest {
    fn from(value: CreateRichDocJson) -> Self {
        Self {
            conversation_id: value.conversation_id,
            doc_json: value.doc_json,
            content_schema: value.content_schema,
            plain_text: value.plain_text,
            input_format: value.input_format,
            input_format_version: value.input_format_version,
            source_payload: value.source_payload,
            title: value.title,
            search_text: value.search_text,
            render_hints_json: value.render_hints_json,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn heartbeat_app_state_accepts_string_wire_values() {
        let foreground = heartbeat_app_state(&json!({ "appState": "foreground" }), "appState")
            .expect("foreground should parse");
        let background = heartbeat_app_state(&json!({ "appState": "background" }), "appState")
            .expect("background should parse");

        assert_eq!(foreground, HeartbeatAppState::Foreground);
        assert_eq!(background, HeartbeatAppState::Background);
    }

    #[test]
    fn heartbeat_app_state_accepts_platform_enum_indices() {
        let foreground = heartbeat_app_state(&json!({ "appState": 0 }), "appState")
            .expect("foreground index should parse");
        let background = heartbeat_app_state(&json!({ "appState": 1 }), "appState")
            .expect("background index should parse");

        assert_eq!(foreground, HeartbeatAppState::Foreground);
        assert_eq!(background, HeartbeatAppState::Background);
    }

    #[test]
    fn heartbeat_app_state_rejects_unknown_values() {
        assert!(heartbeat_app_state(&json!({ "appState": "inactive" }), "appState").is_err());
        assert!(heartbeat_app_state(&json!({ "appState": 2 }), "appState").is_err());
        assert!(heartbeat_app_state(&json!({ "appState": true }), "appState").is_err());
    }
}
