//! RichDoc v2：归一化 + 富文本消息创建/编辑（校验与派生均在 `flare_im_core_sdk::rich_doc_v2`）。
//! 返回体使用 **camelCase** 顶层字段；`source_payload` / `render_hints` 内层键名不递归转换。

use std::collections::HashMap;

use flare_im_core_sdk::client::{CreateRichDocRequest, EditRichDocRequest};
use flare_im_core_sdk::model::IMMessage;
use flare_im_core_sdk::rich_doc_v2::{
    NormalizeOutput, normalize_from_doc_json, normalize_from_html, normalize_from_markdown,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::state::SdkState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RichDocV2Normalized {
    pub doc_json: String,
    pub content_schema: String,
    pub version: u32,
    pub plain_text: String,
    pub search_text: String,
    pub render_hints: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_payload: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
pub struct RichDocV2CreateMessagePayload {
    pub conversation_id: String,
    pub doc_json: String,
    pub content_schema: String,
    pub plain_text: String,
    pub input_format: Option<String>,
    pub input_format_version: Option<i32>,
    pub source_payload: Option<HashMap<String, String>>,
    pub title: Option<String>,
    pub search_text: Option<String>,
    pub render_hints_json: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RichDocV2EditMessagePayload {
    pub message_id: String,
    pub doc_json: String,
    pub content_schema: String,
    pub plain_text: String,
    pub input_format: Option<String>,
    pub input_format_version: Option<i32>,
    pub source_payload: Option<HashMap<String, String>>,
    pub title: Option<String>,
    pub search_text: Option<String>,
    pub render_hints_json: Option<String>,
}

impl From<NormalizeOutput> for RichDocV2Normalized {
    fn from(o: NormalizeOutput) -> Self {
        let source_payload = o.source_payload.map(|(k, v)| HashMap::from([(k, v)]));
        Self {
            doc_json: o.doc_json,
            content_schema: o.content_schema,
            version: o.version,
            plain_text: o.plain_text,
            search_text: o.search_text,
            render_hints: o.render_hints,
            input_format: o.input_format,
            source_payload,
        }
    }
}

#[tauri::command]
pub fn sdk_rich_doc_v2_normalize_from_markdown(
    markdown: String,
) -> std::result::Result<RichDocV2Normalized, String> {
    normalize_from_markdown(&markdown)
        .map(RichDocV2Normalized::from)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn sdk_rich_doc_v2_normalize_from_html(
    html: String,
) -> std::result::Result<RichDocV2Normalized, String> {
    normalize_from_html(&html)
        .map(RichDocV2Normalized::from)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn sdk_rich_doc_v2_normalize_from_doc_json(
    doc_json: String,
) -> std::result::Result<RichDocV2Normalized, String> {
    normalize_from_doc_json(&doc_json)
        .map(RichDocV2Normalized::from)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_rich_doc_v2_create_message(
    state: State<'_, SdkState>,
    payload: RichDocV2CreateMessagePayload,
) -> std::result::Result<IMMessage, String> {
    state
        .message_build_api()
        .await
        .map_err(|e| e.to_string())?
        .create_rich_doc(CreateRichDocRequest {
            conversation_id: payload.conversation_id,
            doc_json: payload.doc_json,
            content_schema: payload.content_schema,
            plain_text: payload.plain_text,
            input_format: payload.input_format,
            input_format_version: payload.input_format_version,
            source_payload: payload.source_payload,
            title: payload.title,
            search_text: payload.search_text,
            render_hints_json: payload.render_hints_json,
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_rich_doc_v2_edit_message(
    state: State<'_, SdkState>,
    payload: RichDocV2EditMessagePayload,
) -> std::result::Result<(), String> {
    state
        .message_api()
        .await
        .map_err(|e| e.to_string())?
        .edit_rich_doc_by_message_id(EditRichDocRequest {
            message_id: payload.message_id,
            doc_json: payload.doc_json,
            content_schema: payload.content_schema,
            plain_text: payload.plain_text,
            input_format: payload.input_format,
            input_format_version: payload.input_format_version,
            source_payload: payload.source_payload,
            title: payload.title,
            search_text: payload.search_text,
            render_hints_json: payload.render_hints_json,
        })
        .await
        .map_err(|e| e.to_string())
}
