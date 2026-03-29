use serde::{Deserialize, Serialize};

/// SDK 配置
///
/// ```ignore
/// let config = SdkConfig::builder()
///     .endpoint("wss://im.example.com")
///     .connect_timeout_secs(15)
///     .build();
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkConfig {
    pub ws_url: Option<String>,
    pub quic_url: Option<String>,
    pub http_url: Option<String>,
    pub connect_timeout_secs: Option<u64>,
    pub reconnect_interval_secs: Option<u64>,
    pub max_reconnect_attempts: Option<u32>,
    pub sync_batch_size: Option<u32>,
    pub ack_timeout_secs: Option<u64>,
    pub ack_max_retries: Option<u32>,
    pub enable_metrics: bool,
}

impl SdkConfig {
    pub fn new(ws_url: impl Into<String>) -> Self {
        Self {
            ws_url: Some(ws_url.into()),
            ..Self::default()
        }
    }

    pub fn builder() -> SdkConfigBuilder {
        SdkConfigBuilder::default()
    }

    pub fn connect_timeout_secs(&self) -> u64 {
        self.connect_timeout_secs.unwrap_or(30)
    }
    pub fn sync_batch_size(&self) -> u32 {
        self.sync_batch_size.unwrap_or(200)
    }
}

impl Default for SdkConfig {
    fn default() -> Self {
        Self {
            ws_url: Some("ws://localhost:8080".into()),
            quic_url: None,
            http_url: None,
            connect_timeout_secs: Some(30),
            reconnect_interval_secs: Some(5),
            max_reconnect_attempts: Some(10),
            sync_batch_size: Some(200),
            ack_timeout_secs: Some(10),
            ack_max_retries: Some(3),
            enable_metrics: false,
        }
    }
}

#[derive(Default)]
pub struct SdkConfigBuilder {
    config: SdkConfig,
}

impl SdkConfigBuilder {
    pub fn endpoint(mut self, url: impl Into<String>) -> Self {
        self.config.ws_url = Some(url.into());
        self
    }
    pub fn quic_endpoint(mut self, url: impl Into<String>) -> Self {
        self.config.quic_url = Some(url.into());
        self
    }
    pub fn http_endpoint(mut self, url: impl Into<String>) -> Self {
        self.config.http_url = Some(url.into());
        self
    }
    pub fn connect_timeout_secs(mut self, s: u64) -> Self {
        self.config.connect_timeout_secs = Some(s);
        self
    }
    pub fn reconnect_interval_secs(mut self, s: u64) -> Self {
        self.config.reconnect_interval_secs = Some(s);
        self
    }
    pub fn max_reconnect_attempts(mut self, n: u32) -> Self {
        self.config.max_reconnect_attempts = Some(n);
        self
    }
    pub fn sync_batch_size(mut self, n: u32) -> Self {
        self.config.sync_batch_size = Some(n);
        self
    }
    pub fn enable_metrics(mut self, b: bool) -> Self {
        self.config.enable_metrics = b;
        self
    }
    pub fn build(self) -> SdkConfig {
        self.config
    }
}
