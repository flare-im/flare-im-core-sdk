//! 配置合并、连接 token、登录存储选型；路径类工具在 [`crate::shared::util::paths`]。
//!
//! - **SQLite 仓储构建**：[`crate::shared::util::sqlite_store`]（`feature = "lifecycle-sqlite"`），由 [`super::IMClient::login`] 在 `LoginDbKind::Sqlite` 时调用。
//! - **IndexedDB（Web）**：宿主实现 [`crate::infrastructure::persistence`] 后 [`LoginDbKind::IndexedDb`] 传入 [`super::IMClient::login`]。

pub use crate::shared::util::paths::{
    default_sdk_data_root, dev_data_dir_relative_to_cwd, parse_data_url_to_path,
    resolve_sdk_data_root, resolve_user_db_path, sanitize_user_id_for_dir,
};

use crate::client::{SdkConfig, SdkResourceProfile, TransportKind, TransportPolicy};
#[cfg(feature = "lifecycle-sqlite")]
use crate::platform::ports::storage::SecureKeyStore;
use crate::shared::error::Result;
#[cfg(not(feature = "dev-test-token"))]
use crate::shared::error::{ErrorCode, FlareError};
#[cfg(feature = "lifecycle-sqlite")]
use std::sync::Arc;

/// 上层可选覆盖项（JSON 字段为 SDK canonical camelCase）。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SdkConfigOverlay {
    pub data_url: Option<String>,
    pub ws_url: Option<String>,
    pub quic_url: Option<String>,
    pub http_url: Option<String>,
    pub media_storage_proxy_prefix: Option<String>,
    pub media_storage_proxy_targets: Option<Vec<String>>,
    pub capability_url: Option<String>,
    pub online_url: Option<String>,
    pub tenant_id: Option<String>,
    /// Stable client device id for multi-device delivery and sync cursors.
    pub device_id: Option<String>,
    pub connect_timeout_secs: Option<u64>,
    pub reconnect_interval_secs: Option<u64>,
    pub max_reconnect_attempts: Option<u32>,
    pub transport_policy: Option<TransportPolicy>,
    /// 非竞速模式下的默认传输（`websocket` / `quic`）。
    pub default_transport: Option<TransportKind>,
    /// 协议竞速优先级（从高到低），如 `["quic", "websocket"]`。
    pub protocol_race_order: Option<Vec<TransportKind>>,
    /// 运行资源预算 profile（`desktop` / `mobile`）。
    pub resource_profile: Option<SdkResourceProfile>,
    pub sync_batch_size: Option<u32>,
    /// Init/重连后会话消息补拉并发数（默认 4）。
    pub init_message_sync_concurrency: Option<u32>,
    pub event_bus_capacity: Option<usize>,
    pub event_dedupe_capacity: Option<usize>,
    pub message_dedupe_capacity: Option<usize>,
    pub ack_timeout_secs: Option<u64>,
    pub ack_max_retries: Option<u32>,
    pub ack_max_in_flight: Option<usize>,
    pub tls_ca_cert_path: Option<String>,
    pub tls_spki_sha256_pins: Option<Vec<String>>,
    pub tls_certificate_sha256_pins: Option<Vec<String>>,
    pub enable_metrics: Option<bool>,
}

/// 解析默认 WebSocket 地址。
///
/// 优先级：
/// 1. `overlay.wsUrl`
/// 2. 环境变量 `FLARE_IM_SERVER_URL`
/// 3. 内置默认 `ws://localhost:60051`
pub fn default_ws_url(overlay: Option<&SdkConfigOverlay>) -> String {
    overlay
        .and_then(|c| c.ws_url.as_deref())
        .map(String::from)
        .or_else(|| std::env::var("FLARE_IM_SERVER_URL").ok())
        .unwrap_or_else(|| "ws://localhost:60051".to_string())
}

/// 将上层 overlay 合并进基础 [`SdkConfig`]。
///
/// 仅覆盖 `Some(...)` 字段，未提供的字段保持默认值。
pub fn merge_sdk_config(ws_url: &str, overlay: Option<&SdkConfigOverlay>) -> SdkConfig {
    let mut config = SdkConfig::new(ws_url);
    if let Some(o) = overlay {
        if let Some(u) = &o.ws_url {
            config.ws_url = Some(u.clone());
        }
        if o.quic_url.is_some() {
            config.quic_url = o.quic_url.clone();
        }
        if o.http_url.is_some() {
            config.http_url = o.http_url.clone();
        }
        if o.media_storage_proxy_prefix.is_some() {
            config.media_storage_proxy_prefix = o.media_storage_proxy_prefix.clone();
        }
        if let Some(targets) = &o.media_storage_proxy_targets {
            config.media_storage_proxy_targets = targets.clone();
        }
        if o.capability_url.is_some() {
            config.capability_url = o.capability_url.clone();
        }
        if o.online_url.is_some() {
            config.online_url = o.online_url.clone();
        }
        if o.tenant_id.is_some() {
            config.tenant_id = o.tenant_id.clone();
        }
        if o.device_id.is_some() {
            config.device_id = o.device_id.clone();
        }
        if o.connect_timeout_secs.is_some() {
            config.connect_timeout_secs = o.connect_timeout_secs;
        }
        if o.reconnect_interval_secs.is_some() {
            config.reconnect_interval_secs = o.reconnect_interval_secs;
        }
        if o.max_reconnect_attempts.is_some() {
            config.max_reconnect_attempts = o.max_reconnect_attempts;
        }
        if let Some(policy) = o.transport_policy {
            config.transport_policy = policy;
        }
        if o.default_transport.is_some() {
            config.default_transport = o.default_transport;
        }
        if o.protocol_race_order.is_some() {
            config.protocol_race_order = o.protocol_race_order.clone();
        }
        if let Some(profile) = o.resource_profile {
            config.resource_profile = profile;
        }
        if o.sync_batch_size.is_some() {
            config.sync_batch_size = o.sync_batch_size;
        }
        if o.init_message_sync_concurrency.is_some() {
            config.init_message_sync_concurrency = o.init_message_sync_concurrency;
        }
        if o.event_bus_capacity.is_some() {
            config.event_bus_capacity = o.event_bus_capacity;
        }
        if o.event_dedupe_capacity.is_some() {
            config.event_dedupe_capacity = o.event_dedupe_capacity;
        }
        if o.message_dedupe_capacity.is_some() {
            config.message_dedupe_capacity = o.message_dedupe_capacity;
        }
        if o.ack_timeout_secs.is_some() {
            config.ack_timeout_secs = o.ack_timeout_secs;
        }
        if o.ack_max_retries.is_some() {
            config.ack_max_retries = o.ack_max_retries;
        }
        if o.ack_max_in_flight.is_some() {
            config.ack_max_in_flight = o.ack_max_in_flight;
        }
        if o.tls_ca_cert_path.is_some() {
            config.tls_ca_cert_path = o.tls_ca_cert_path.clone();
        }
        if let Some(pins) = &o.tls_spki_sha256_pins {
            config.tls_spki_sha256_pins = pins.clone();
        }
        if let Some(pins) = &o.tls_certificate_sha256_pins {
            config.tls_certificate_sha256_pins = pins.clone();
        }
        if let Some(b) = o.enable_metrics {
            config.enable_metrics = b;
        }
    }
    config
}

/// 解析连接 token。
///
/// 优先级：
/// 1. 显式参数 `explicit_token`
/// 2. 环境变量 `FLARE_IM_TOKEN`
/// 3. 环境变量 `TOKEN`
/// 4. 开发态自动生成临时 token（仅用于开发/测试）
pub fn resolve_connect_token(user_id: &str, explicit_token: Option<&str>) -> Result<String> {
    if let Some(token) = explicit_token {
        let t = token.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    if let Ok(token) = std::env::var("FLARE_IM_TOKEN") {
        let t = token.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    if let Ok(token) = std::env::var("TOKEN") {
        let t = token.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    #[cfg(feature = "dev-test-token")]
    {
        return crate::shared::util::generate_core_token(&crate::shared::util::CoreTokenConfig {
            secret: "insecure-secret".to_string(),
            issuer: "flare-im-core".to_string(),
            user_id: user_id.to_string(),
            ttl_secs: 3600,
            device_id: None,
            tenant_id: None,
        });
    }

    #[cfg(not(feature = "dev-test-token"))]
    {
        let _ = user_id;
        Err(FlareError::localized(
            ErrorCode::ConfigurationError,
            "connect token required; automatic development token generation is disabled",
        ))
    }
}

/// 登录时存储选型，配合 [`super::IMClient::login`]。
pub enum LoginDbKind {
    #[cfg(feature = "lifecycle-sqlite")]
    Sqlite,
    #[cfg(feature = "lifecycle-sqlite")]
    EncryptedSqlite {
        key_store: Arc<dyn SecureKeyStore>,
        key_namespace: Option<String>,
        tenant_id: Option<String>,
        key_name: Option<String>,
    },
    IndexedDb(crate::infrastructure::persistence::StoreProvider),
}

#[cfg(feature = "lifecycle-sqlite")]
impl LoginDbKind {
    pub fn encrypted_sqlite(key_store: Arc<dyn SecureKeyStore>) -> Self {
        Self::EncryptedSqlite {
            key_store,
            key_namespace: None,
            tenant_id: None,
            key_name: None,
        }
    }

    pub fn encrypted_sqlite_with_descriptor(
        key_store: Arc<dyn SecureKeyStore>,
        key_namespace: impl Into<String>,
        tenant_id: impl Into<String>,
        key_name: impl Into<String>,
    ) -> Self {
        Self::EncryptedSqlite {
            key_store,
            key_namespace: Some(key_namespace.into()),
            tenant_id: Some(tenant_id.into()),
            key_name: Some(key_name.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_config_overlay_uses_canonical_camel_case_json() {
        let overlay: SdkConfigOverlay = serde_json::from_value(serde_json::json!({
            "wsUrl": "ws://localhost:60051",
            "quicUrl": "quic://localhost:60052",
            "transportPolicy": "protocol_race",
            "defaultTransport": "websocket",
            "protocolRaceOrder": ["quic", "websocket"],
            "connectTimeoutSecs": 30,
            "tlsCaCertPath": "/tmp/flare-ca.crt",
            "tlsSpkiSha256Pins": ["spki-sha256/current", "spki-sha256/next"],
            "tlsCertificateSha256Pins": ["sha256/legacy"],
            "enableMetrics": true
        }))
        .expect("camelCase overlay");

        assert_eq!(overlay.ws_url.as_deref(), Some("ws://localhost:60051"));
        assert_eq!(overlay.quic_url.as_deref(), Some("quic://localhost:60052"));
        assert_eq!(
            overlay.transport_policy,
            Some(TransportPolicy::ProtocolRace)
        );
        assert_eq!(overlay.default_transport, Some(TransportKind::WebSocket));
        assert_eq!(
            overlay.protocol_race_order,
            Some(vec![TransportKind::Quic, TransportKind::WebSocket])
        );
        assert_eq!(overlay.connect_timeout_secs, Some(30));
        assert_eq!(
            overlay.tls_ca_cert_path.as_deref(),
            Some("/tmp/flare-ca.crt")
        );
        assert_eq!(
            overlay.tls_spki_sha256_pins,
            Some(vec![
                "spki-sha256/current".to_string(),
                "spki-sha256/next".to_string()
            ])
        );
        assert_eq!(
            overlay.tls_certificate_sha256_pins,
            Some(vec!["sha256/legacy".to_string()])
        );
        assert_eq!(overlay.enable_metrics, Some(true));

        let merged = merge_sdk_config("ws://fallback", Some(&overlay));
        assert_eq!(
            merged.tls_ca_cert_path.as_deref(),
            Some("/tmp/flare-ca.crt")
        );

        let json = serde_json::to_value(&overlay).expect("serialize overlay");
        assert!(json.get("wsUrl").is_some());
        assert!(json.get("ws_url").is_none());
        assert!(json.get("transportPolicy").is_some());
        assert!(json.get("transport_policy").is_none());
        assert!(json.get("tlsCaCertPath").is_some());
        assert!(json.get("tls_ca_cert_path").is_none());
        assert!(json.get("tlsSpkiSha256Pins").is_some());
        assert!(json.get("tls_spki_sha256_pins").is_none());
        assert!(json.get("tlsCertificateSha256Pins").is_some());
    }
}
