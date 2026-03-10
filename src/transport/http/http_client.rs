use crate::error::{SdkError, Result};

/// HTTP 降级传输 — WebSocket 不可用时的 fallback
///
/// 通过 HTTP 长轮询或短轮询实现消息收发。
/// 当前为占位实现，后续可接入 reqwest / hyper。
pub struct HttpClient {
    base_url: String,
}

impl HttpClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self { base_url: base_url.into() }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn send_bytes(&self, _path: &str, _data: &[u8]) -> Result<Vec<u8>> {
        Err(SdkError::SendFailed("HTTP transport not yet implemented".into()))
    }
}
