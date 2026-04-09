//! RichDoc v2 管道错误（校验 / 解析 / 平台能力）。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RichDocV2Error {
    #[error("rich_doc_v2: invalid json: {0}")]
    InvalidJson(String),
    #[error("rich_doc_v2: {0}")]
    InvalidStructure(String),
    #[error("rich_doc_v2: markdown normalize failed: {0}")]
    Markdown(String),
    #[error("rich_doc_v2: html normalize is not available on this target")]
    HtmlUnavailable,
    #[error("rich_doc_v2: html normalize failed: {0}")]
    Html(String),
}
