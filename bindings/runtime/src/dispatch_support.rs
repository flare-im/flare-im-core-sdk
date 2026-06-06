//! Shared helpers for generated JSON dispatch modules.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

use crate::{BindingResponse, binding_invalid_parameter};
use flare_im_core_sdk::client::api::{
    CreateLocationRequest, CreateRichDocRequest, CreateStickerRequest, EditRichDocRequest,
};
use flare_im_core_sdk::model::content_builder::BuiltContent;
use flare_im_core_sdk::model::conversation::ConversationType;
use flare_im_core_sdk::model::media::UploadOptions;
use flare_im_core_sdk::model::message::{IMMessage, MarkType, SendAck};
use flare_im_core_sdk::model::message_elem::{Elem, elem_to_message_content};
use flare_im_core_sdk::shared::error::Result;
use flare_proto::common::MessageType;

pub fn json<T: serde::Serialize>(value: T) -> Result<BindingResponse> {
    serde_json::to_value(value)
        .map(BindingResponse::json)
        .map_err(|e| {
            binding_invalid_parameter(format!("failed to serialize binding response: {e}"))
        })
}

pub fn json_send_ack(ack: SendAck) -> Result<BindingResponse> {
    Ok(BindingResponse::json(serde_json::json!({
        "client_msg_id": ack.client_msg_id,
        "server_msg_id": ack.server_msg_id,
        "seq": ack.seq,
        "conversation_id": ack.conversation_id,
        "success": ack.success,
        "error_code": ack.error_code,
        "error_message": ack.error_message,
    })))
}

pub fn conversation_id(value: &Value) -> Result<String> {
    json_string(value, "conversation_id")
}

pub fn json_string(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .or_else(|| camel_alias(value, key))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| binding_invalid_parameter(format!("missing or invalid JSON field: {key}")))
}

pub fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .or_else(|| camel_alias(value, key))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

pub fn string_any(value: &Value, keys: &[&str]) -> Result<String> {
    keys.iter()
        .find_map(|key| {
            value
                .get(*key)
                .or_else(|| camel_alias(value, key))
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
            .or_else(|| camel_alias(value, key))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    })
}

pub fn json_u64(value: &Value, key: &str) -> Result<u64> {
    value
        .get(key)
        .or_else(|| camel_alias(value, key))
        .and_then(|x| x.as_u64().or_else(|| x.as_i64().map(|i| i as u64)))
        .ok_or_else(|| binding_invalid_parameter(format!("missing or invalid JSON field: {key}")))
}

pub fn json_i64(value: &Value, key: &str) -> Result<i64> {
    value
        .get(key)
        .or_else(|| camel_alias(value, key))
        .and_then(Value::as_i64)
        .ok_or_else(|| binding_invalid_parameter(format!("missing or invalid JSON field: {key}")))
}

pub fn json_i32(value: &Value, key: &str) -> Result<i32> {
    value
        .get(key)
        .or_else(|| camel_alias(value, key))
        .and_then(Value::as_i64)
        .map(|i| i as i32)
        .ok_or_else(|| binding_invalid_parameter(format!("missing or invalid JSON field: {key}")))
}

pub fn json_f64(value: &Value, key: &str) -> Result<f64> {
    value
        .get(key)
        .or_else(|| camel_alias(value, key))
        .and_then(Value::as_f64)
        .ok_or_else(|| binding_invalid_parameter(format!("missing or invalid JSON field: {key}")))
}

pub fn json_bool(value: &Value, key: &str) -> Result<bool> {
    value
        .get(key)
        .or_else(|| camel_alias(value, key))
        .and_then(Value::as_bool)
        .ok_or_else(|| binding_invalid_parameter(format!("missing or invalid JSON field: {key}")))
}

pub fn optional_upload_options(value: &Value, key: &str) -> Result<Option<UploadOptions>> {
    let Some(raw) = value.get(key).or_else(|| camel_alias(value, key)) else {
        return Ok(None);
    };
    if raw.is_null() {
        return Ok(None);
    }
    let Some(chunk_size) = raw.get("chunk_size").and_then(|v| v.as_u64()) else {
        return Ok(None);
    };
    let chunk_size = usize::try_from(chunk_size).map_err(|_| {
        binding_invalid_parameter(format!("invalid upload options field: {key}.chunk_size"))
    })?;
    if chunk_size == 0 {
        return Err(binding_invalid_parameter(format!(
            "invalid upload options field: {key}.chunk_size"
        )));
    }
    Ok(Some(UploadOptions { chunk_size }))
}

pub fn json_bytes_vec(value: &Value, key: &str) -> Result<Vec<u8>> {
    let arr = value
        .get(key)
        .or_else(|| camel_alias(value, key))
        .and_then(Value::as_array)
        .ok_or_else(|| {
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
    let arr = value
        .get(key)
        .or_else(|| camel_alias(value, key))
        .and_then(Value::as_array)
        .ok_or_else(|| {
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
        .or_else(|| camel_alias(value, key))
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
}

pub fn optional_i32(value: &Value, key: &str) -> Option<i32> {
    value
        .get(key)
        .or_else(|| camel_alias(value, key))
        .and_then(Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
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
    let Some(raw) = value
        .get("conversation_type")
        .or_else(|| value.get("conversationType"))
    else {
        return Err(binding_invalid_parameter(
            "missing or invalid JSON field: conversation_type",
        ));
    };
    if let Some(v) = raw.as_i64() {
        return Ok(ConversationType::from_proto_int(v as i32));
    }
    if let Some(v) = raw.as_str() {
        return Ok(ConversationType::from(v));
    }
    Err(binding_invalid_parameter(
        "missing or invalid JSON field: conversation_type",
    ))
}

#[derive(Deserialize)]
struct BuiltContentJsonShell {
    message_type: i32,
    content: Elem,
}

pub fn built_content_from_value(value: &Value) -> Result<BuiltContent> {
    let shell: BuiltContentJsonShell = from_value(value.clone(), "built content")?;
    let message_type =
        MessageType::try_from(shell.message_type).unwrap_or(MessageType::Unspecified);
    Ok(BuiltContent::new(
        message_type,
        elem_to_message_content(&shell.content),
    ))
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
        snapshot_url: optional_string(&params, "snapshot_url"),
        snapshot_local_path: optional_string(&params, "snapshot_local_path"),
    })
}

pub fn build_create_sticker_request(params: Value) -> Result<CreateStickerRequest> {
    Ok(CreateStickerRequest {
        conversation_id: conversation_id(&params)?,
        sticker_id: json_string(&params, "sticker_id")?,
        package_id: optional_string(&params, "package_id"),
        url: optional_string(&params, "url"),
        width: params
            .get("width")
            .and_then(Value::as_i64)
            .map(|i| i as i32),
        height: params
            .get("height")
            .and_then(Value::as_i64)
            .map(|i| i as i32),
        sticker_format: optional_string(&params, "sticker_format"),
    })
}

#[derive(Deserialize)]
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

fn camel_alias<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    let mut out = String::with_capacity(key.len());
    let mut upper_next = false;
    for ch in key.chars() {
        if ch == '_' {
            upper_next = true;
        } else if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    value.get(out)
}

#[cfg(feature = "plugin-call")]
pub async fn dispatch_call_signal(
    client: &flare_im_core_sdk::client::IMClient,
    params: Value,
) -> Result<()> {
    use flare_im_core_sdk::shared::error::FlareError;
    use flare_proto::common::CallMediaType;

    let kind = json_string(&params, "kind")?;
    let conversation_id = json_string(&params, "conversation_id")?;
    let call_id = json_string(&params, "call_id")?;
    let to_user_id = optional_string(&params, "to_user_id");
    let video = params
        .get("video")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reason = optional_string(&params, "reason").unwrap_or_else(|| "hangup".to_string());
    let code = params
        .get("code")
        .and_then(Value::as_i64)
        .map(|v| v as i32)
        .unwrap_or(486);
    let close_room_if_vacant = params.get("close_room_if_vacant").and_then(Value::as_bool);
    let participant_user_ids = params
        .get("participant_user_ids")
        .or_else(|| params.get("participantUserIds"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let from = client
        .current_user_id()
        .await
        .unwrap_or_default()
        .trim()
        .to_string();
    if from.is_empty() {
        return Err(FlareError::general_error("not logged in"));
    }

    let media_types: Vec<CallMediaType> = if video {
        vec![CallMediaType::Audio, CallMediaType::Video]
    } else {
        vec![CallMediaType::Audio]
    };
    let mut event = match kind.as_str() {
        "invite" => {
            flare_im_core_sdk::extension::capability::call_event::call_invite_for_conversation(
                conversation_id.clone(),
                call_id.clone(),
                from,
                to_user_id.clone().unwrap_or_default(),
                participant_user_ids,
                media_types.as_slice(),
            )
            .map_err(|e| FlareError::general_error(e.to_string()))?
        }
        "accept" => flare_im_core_sdk::extension::capability::call_event::call_accept(
            conversation_id.clone(),
            call_id.clone(),
            from,
            media_types.as_slice(),
        ),
        "reject" => flare_im_core_sdk::extension::capability::call_event::call_reject(
            conversation_id.clone(),
            call_id.clone(),
            from,
            reason,
            code,
        ),
        "hangup" => {
            flare_im_core_sdk::extension::capability::call_event::call_hangup_with_room_policy(
                conversation_id.clone(),
                call_id.clone(),
                from,
                reason,
                None,
                close_room_if_vacant,
            )
        }
        _ => return Err(FlareError::general_error("invalid call signal kind")),
    };
    if kind != "invite" {
        flare_im_core_sdk::call_plugin::apply_session_signaling_audience(
            &conversation_id,
            &mut event,
            to_user_id.as_deref(),
        );
    }
    client.send_call_signal(&conversation_id, event).await
}

#[cfg(not(feature = "plugin-call"))]
pub async fn dispatch_call_signal(
    _client: &flare_im_core_sdk::client::IMClient,
    _params: Value,
) -> Result<()> {
    Err(crate::binding_operation_not_supported("send_call_signal"))
}
