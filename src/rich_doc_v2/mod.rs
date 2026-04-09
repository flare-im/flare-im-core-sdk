//! RichDoc v2：协议归一化、校验与派生字段（IM 富文本唯一结构化形态）。
//!
//! - 前端 **禁止** 自行拼装 doc JSON；应通过 Tauri command 调用本模块。
//! - 与 `flare-proto` `RichTextContent` / `message_content.proto` 注释对齐。

mod error;
mod extract;
mod from_html;
mod from_markdown;
pub mod pipeline;
pub mod validate;

pub use error::RichDocV2Error;
pub use extract::{derive_from_json_str, derive_from_value, RichDocDerived};
pub use pipeline::{
    normalize_from_doc_json, normalize_from_html, normalize_from_markdown, NormalizeOutput,
};
pub use validate::validate_doc_json;
