use crate::error::{FlareError, Result};

/// HTTP 降级传输 — WebSocket 不可用时的 fallback
pub struct HttpClient {
    base_url: String,
}

impl HttpClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn send_bytes(&self, _path: &str, _data: &[u8]) -> Result<Vec<u8>> {
        Err(FlareError::system("HTTP transport not yet implemented"))
    }
}
