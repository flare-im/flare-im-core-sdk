//! 配置合并、连接 token、登录存储选型；路径类工具在 [`crate::shared::util::paths`]。
//!
//! - **SQLite 仓储构建**：[`crate::shared::util::sqlite_store`]（`feature = "lifecycle-sqlite"`），由 [`super::IMClient::login`] 在 `LoginDbKind::Sqlite` 时调用。
//! - **IndexedDB（Web）**：宿主实现 [`crate::infrastructure::persistence`] 后 [`LoginDbKind::IndexedDb`] 传入 [`super::IMClient::login`]。

pub use crate::shared::util::paths::{
    default_sdk_data_root, dev_data_dir_relative_to_cwd, parse_data_url_to_path,
    resolve_sdk_data_root, resolve_user_db_path, sanitize_user_id_for_dir,
};

use crate::client::{SdkConfig, TransportKind, TransportPolicy};
use crate::shared::error::Result;

/// 上层可选覆盖项（JSON 字段 snake_case，与 Rust 字段一致）。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SdkConfigOverlay {
    pub data_url: Option<String>,
    pub ws_url: Option<String>,
    pub quic_url: Option<String>,
    pub http_url: Option<String>,
    pub capability_url: Option<String>,
    pub online_url: Option<String>,
    pub tenant_id: Option<String>,
    pub connect_timeout_secs: Option<u64>,
    pub reconnect_interval_secs: Option<u64>,
    pub max_reconnect_attempts: Option<u32>,
    pub transport_policy: Option<TransportPolicy>,
    /// 非竞速模式下的默认传输（`websocket` / `quic`）。
    pub default_transport: Option<TransportKind>,
    /// 协议竞速优先级（从高到低），如 `["quic", "websocket"]`。
    pub protocol_race_order: Option<Vec<TransportKind>>,
    pub sync_batch_size: Option<u32>,
    /// Init/重连后会话消息补拉并发数（默认 4）。
    pub init_message_sync_concurrency: Option<u32>,
    pub ack_timeout_secs: Option<u64>,
    pub ack_max_retries: Option<u32>,
    pub enable_metrics: Option<bool>,
}

/// 解析默认 WebSocket 地址。
///
/// 优先级：
/// 1. `overlay.ws_url`
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
        if o.capability_url.is_some() {
            config.capability_url = o.capability_url.clone();
        }
        if o.online_url.is_some() {
            config.online_url = o.online_url.clone();
        }
        if o.tenant_id.is_some() {
            config.tenant_id = o.tenant_id.clone();
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
        if o.sync_batch_size.is_some() {
            config.sync_batch_size = o.sync_batch_size;
        }
        if o.init_message_sync_concurrency.is_some() {
            config.init_message_sync_concurrency = o.init_message_sync_concurrency;
        }
        if o.ack_timeout_secs.is_some() {
            config.ack_timeout_secs = o.ack_timeout_secs;
        }
        if o.ack_max_retries.is_some() {
            config.ack_max_retries = o.ack_max_retries;
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
    crate::shared::util::generate_test_token(
        "insecure-secret",
        "flare-im-core",
        user_id,
        3600,
        None,
        None,
    )
}

/// 登录时存储选型，配合 [`super::IMClient::login`]。
pub enum LoginDbKind {
    #[cfg(feature = "lifecycle-sqlite")]
    Sqlite,
    IndexedDb(crate::infrastructure::persistence::StoreProvider),
}
