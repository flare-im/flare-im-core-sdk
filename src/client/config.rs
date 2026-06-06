use serde::{Deserialize, Serialize};

use flare_core::common::config_types::TransportProtocol as CoreTransport;

/// Wire transport kind for init overlay and protocol race ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    WebSocket,
    Quic,
}

impl TransportKind {
    pub fn to_core(self) -> CoreTransport {
        match self {
            Self::WebSocket => CoreTransport::WebSocket,
            Self::Quic => CoreTransport::QUIC,
        }
    }

    pub fn parse_list(values: &[String]) -> Option<Vec<Self>> {
        let mut out = Vec::with_capacity(values.len());
        for raw in values {
            let kind = match raw.trim().to_ascii_lowercase().as_str() {
                "websocket" | "ws" => Self::WebSocket,
                "quic" => Self::Quic,
                _ => return None,
            };
            out.push(kind);
        }
        if out.is_empty() { None } else { Some(out) }
    }
}

/// Transport selection policy.
///
/// Browser/WASM must use WebSocket because QUIC/native protocol racing is not
/// available in the browser sandbox. Native targets can keep protocol racing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportPolicy {
    Auto,
    WebSocketOnly,
    ProtocolRace,
}

impl Default for TransportPolicy {
    fn default() -> Self {
        Self::Auto
    }
}

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
    pub capability_url: Option<String>,
    pub online_url: Option<String>,
    pub tenant_id: Option<String>,
    pub connect_timeout_secs: Option<u64>,
    pub reconnect_interval_secs: Option<u64>,
    pub max_reconnect_attempts: Option<u32>,
    pub transport_policy: TransportPolicy,
    /// 非竞速时的首选传输；未设置时以 WebSocket 为主入口。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_transport: Option<TransportKind>,
    /// 协议竞速顺序（前项优先），如 `["quic", "websocket"]`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_race_order: Option<Vec<TransportKind>>,
    pub sync_batch_size: Option<u32>,
    pub init_message_sync_concurrency: Option<u32>,
    pub ack_timeout_secs: Option<u64>,
    pub ack_max_retries: Option<u32>,
    pub enable_metrics: bool,
}

impl SdkConfig {
    /// 以给定 WebSocket 地址创建配置。
    pub fn new(ws_url: impl Into<String>) -> Self {
        Self {
            ws_url: Some(ws_url.into()),
            ..Self::default()
        }
    }

    /// 返回配置构建器。
    pub fn builder() -> SdkConfigBuilder {
        SdkConfigBuilder::default()
    }

    /// 获取连接超时时间（秒），未配置时使用默认值。
    pub fn connect_timeout_secs(&self) -> u64 {
        self.connect_timeout_secs.unwrap_or(30)
    }
    /// 获取同步批大小，未配置时使用默认值。
    pub fn sync_batch_size(&self) -> u32 {
        self.sync_batch_size.unwrap_or(200)
    }

    /// Init/重连阶段按会话补拉消息的并发上限（默认 4，最小 1）。
    pub fn init_message_sync_concurrency(&self) -> u32 {
        self.init_message_sync_concurrency.unwrap_or(4).max(1)
    }

    /// Effective transport policy for the current compilation target.
    pub fn effective_transport_policy(&self) -> TransportPolicy {
        if cfg!(target_arch = "wasm32") {
            TransportPolicy::WebSocketOnly
        } else {
            self.transport_policy
        }
    }

    /// 协议竞速列表；需同时配置 `ws_url` 与 `quic_url`，且策略为 Auto / ProtocolRace。
    pub fn effective_protocol_race_order(&self) -> Option<Vec<CoreTransport>> {
        if cfg!(target_arch = "wasm32") {
            return None;
        }
        let policy = self.effective_transport_policy();
        if !matches!(
            policy,
            TransportPolicy::Auto | TransportPolicy::ProtocolRace
        ) {
            return None;
        }
        self.ws_url.as_ref()?;
        self.quic_url.as_ref()?;
        let order = self
            .protocol_race_order
            .clone()
            .unwrap_or_else(|| vec![TransportKind::Quic, TransportKind::WebSocket]);
        Some(order.into_iter().map(TransportKind::to_core).collect())
    }

    /// 建立长连接时的主 URL（竞速时仍作为 builder 入口，具体协议由 flare-core 选择）。
    pub fn primary_connect_url(&self) -> Option<String> {
        let ws = self.ws_url.clone()?;
        match self.default_transport {
            Some(TransportKind::Quic) => self.quic_url.clone().or(Some(ws)),
            _ => Some(ws),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_race_order_respects_overlay() {
        let config = SdkConfig {
            ws_url: Some("ws://a".into()),
            quic_url: Some("quic://b".into()),
            transport_policy: TransportPolicy::ProtocolRace,
            protocol_race_order: Some(vec![TransportKind::WebSocket, TransportKind::Quic]),
            ..SdkConfig::default()
        };
        let order = config.effective_protocol_race_order().expect("race");
        assert_eq!(order[0], CoreTransport::WebSocket);
        assert_eq!(order[1], CoreTransport::QUIC);
    }
}

impl Default for SdkConfig {
    fn default() -> Self {
        Self {
            ws_url: Some("ws://localhost:8080".into()),
            quic_url: None,
            http_url: Some("http://localhost:50050".into()),
            capability_url: Some("http://localhost:50110".into()),
            online_url: Some("http://localhost:50061".into()),
            tenant_id: Some("0".into()),
            connect_timeout_secs: Some(30),
            reconnect_interval_secs: Some(5),
            max_reconnect_attempts: None,
            transport_policy: TransportPolicy::Auto,
            default_transport: None,
            protocol_race_order: None,
            sync_batch_size: Some(200),
            init_message_sync_concurrency: None,
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
    /// 设置 WebSocket 入口地址。
    pub fn endpoint(mut self, url: impl Into<String>) -> Self {
        self.config.ws_url = Some(url.into());
        self
    }
    /// 设置 QUIC 入口地址。
    pub fn quic_endpoint(mut self, url: impl Into<String>) -> Self {
        self.config.quic_url = Some(url.into());
        self
    }
    /// 设置 HTTP 入口地址（用于 REST/上传等扩展能力）。
    pub fn http_endpoint(mut self, url: impl Into<String>) -> Self {
        self.config.http_url = Some(url.into());
        self
    }
    /// 设置 capability gRPC 端点（`http://host:port`，与 `flare-capability` 或网关后端一致）。
    pub fn capability_endpoint(mut self, url: impl Into<String>) -> Self {
        self.config.capability_url = Some(url.into());
        self
    }
    /// 设置 signaling-online gRPC 端点（`http://host:port`）。
    pub fn online_endpoint(mut self, url: impl Into<String>) -> Self {
        self.config.online_url = Some(url.into());
        self
    }
    /// 设置默认租户 ID（能力授权 gRPC 与编排器默认 `0` 对齐）。
    pub fn tenant_id(mut self, tenant_id: impl Into<String>) -> Self {
        self.config.tenant_id = Some(tenant_id.into());
        self
    }
    /// 设置连接超时（秒）。
    pub fn connect_timeout_secs(mut self, s: u64) -> Self {
        self.config.connect_timeout_secs = Some(s);
        self
    }
    /// 设置重连间隔（秒）。
    pub fn reconnect_interval_secs(mut self, s: u64) -> Self {
        self.config.reconnect_interval_secs = Some(s);
        self
    }
    /// 设置最大重连次数。
    pub fn max_reconnect_attempts(mut self, n: u32) -> Self {
        self.config.max_reconnect_attempts = Some(n);
        self
    }
    /// 设置传输策略。
    pub fn transport_policy(mut self, policy: TransportPolicy) -> Self {
        self.config.transport_policy = policy;
        self
    }
    /// 设置单次同步批大小。
    pub fn sync_batch_size(mut self, n: u32) -> Self {
        self.config.sync_batch_size = Some(n);
        self
    }
    /// 是否开启 SDK 指标采集。
    pub fn enable_metrics(mut self, b: bool) -> Self {
        self.config.enable_metrics = b;
        self
    }
    /// 产出最终配置对象。
    pub fn build(self) -> SdkConfig {
        self.config
    }
}
