//! 归一化入口：Markdown / HTML / 已有 doc JSON → 校验 + 派生字段。

use serde_json::Value;

use super::RichDocV2Error;
use super::extract::{RichDocDerived, derive_from_value};
use super::from_html::html_fragment_to_doc_value;
use super::from_markdown::markdown_to_doc_value;
use super::validate::validate_doc_json;

pub const CONTENT_SCHEMA_RICH_DOC: &str = "rich_doc";
pub const INPUT_FORMAT_MARKDOWN: &str = "markdown";
pub const INPUT_FORMAT_HTML: &str = "html";

/// Markdown → RichDoc v2 → 校验 → 派生 `plain_text` / `search_text` / `render_hints`。
pub fn normalize_from_markdown(md: &str) -> Result<NormalizeOutput, RichDocV2Error> {
    let doc = markdown_to_doc_value(md)?;
    finalize_doc_value(doc, Some(INPUT_FORMAT_MARKDOWN), md)
}

/// HTML 片段 → RichDoc v2 → …（native only）。
pub fn normalize_from_html(html: &str) -> Result<NormalizeOutput, RichDocV2Error> {
    let doc = html_fragment_to_doc_value(html)?;
    finalize_doc_value(doc, Some(INPUT_FORMAT_HTML), html)
}

/// 客户端已持有权威 doc JSON（例如自研编辑器直出）时：仅校验 + 派生。
pub fn normalize_from_doc_json(doc_json: &str) -> Result<NormalizeOutput, RichDocV2Error> {
    validate_doc_json(doc_json)?;
    let v: Value =
        serde_json::from_str(doc_json).map_err(|e| RichDocV2Error::InvalidJson(e.to_string()))?;
    let derived = derive_from_value(&v)?;
    Ok(NormalizeOutput::from_parts(
        doc_json.to_string(),
        derived,
        None,
        None,
    ))
}

fn finalize_doc_value(
    doc: Value,
    input_format: Option<&str>,
    source_snapshot: &str,
) -> Result<NormalizeOutput, RichDocV2Error> {
    let doc_json = serde_json::to_string(&doc)
        .map_err(|e| RichDocV2Error::InvalidStructure(format!("serialize doc: {e}")))?;
    validate_doc_json(&doc_json)?;
    let derived = derive_from_value(&doc)?;
    let snap_key = input_format.unwrap_or("raw");
    Ok(NormalizeOutput::from_parts(
        doc_json,
        derived,
        input_format.map(String::from),
        Some((snap_key.to_string(), source_snapshot.to_string())),
    ))
}

/// 与 `RichTextContent` 对齐的归一化结果（Rust / Tauri 侧 snake_case）。
#[derive(Debug, Clone)]
pub struct NormalizeOutput {
    pub doc_json: String,
    pub content_schema: String,
    pub version: u32,
    pub plain_text: String,
    pub search_text: String,
    /// 渲染提示对象；落库时写入 `render_hints_json`。
    pub render_hints: Value,
    pub input_format: Option<String>,
    /// 可选：`source_payload` 单键快照（如 markdown / html）。
    pub source_payload: Option<(String, String)>,
}

impl NormalizeOutput {
    fn from_parts(
        doc_json: String,
        derived: RichDocDerived,
        input_format: Option<String>,
        source_payload: Option<(String, String)>,
    ) -> Self {
        Self {
            doc_json,
            content_schema: CONTENT_SCHEMA_RICH_DOC.into(),
            version: 2,
            plain_text: derived.plain_text,
            search_text: derived.search_text,
            render_hints: derived.render_hints,
            input_format,
            source_payload,
        }
    }

    pub fn render_hints_json_string(&self) -> String {
        serde_json::to_string(&self.render_hints).unwrap_or_else(|_| "{}".into())
    }
}
