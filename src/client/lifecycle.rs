//! 配置合并、连接 token、登录存储选型；路径类工具在 [`crate::util::paths`]，此处 `pub use` 保持 `crate::lifecycle::*` 路径稳定。
//!
//! - **SQLite 仓储构建**：[`crate::util::sqlite_store`]（`feature = "lifecycle-sqlite"`），由 [`super::IMClient::login`] 在 `LoginDbKind::Sqlite` 时调用。
//! - **IndexedDB（Web）**：宿主实现 [`crate::store`] 后 [`LoginDbKind::IndexedDb`] 传入 [`super::IMClient::login`]。

pub use crate::util::paths::{
    dev_data_dir_relative_to_cwd, parse_data_url_to_path, resolve_user_db_path,
    sanitize_user_id_for_dir,
};

use crate::client::SdkConfig;
use crate::Result;

/// 上层可选覆盖项（camelCase JSON）。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkConfigOverlay {
    pub data_url: Option<String>,
    pub ws_url: Option<String>,
    pub quic_url: Option<String>,
    pub http_url: Option<String>,
    pub connect_timeout_secs: Option<u64>,
    pub reconnect_interval_secs: Option<u64>,
    pub max_reconnect_attempts: Option<u32>,
    pub sync_batch_size: Option<u32>,
    pub ack_timeout_secs: Option<u64>,
    pub ack_max_retries: Option<u32>,
    pub enable_metrics: Option<bool>,
}

pub fn default_ws_url(overlay: Option<&SdkConfigOverlay>) -> String {
    overlay
        .and_then(|c| c.ws_url.as_deref())
        .map(String::from)
        .or_else(|| std::env::var("FLARE_IM_SERVER_URL").ok())
        .unwrap_or_else(|| "ws://localhost:60051".to_string())
}

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
        if o.connect_timeout_secs.is_some() {
            config.connect_timeout_secs = o.connect_timeout_secs;
        }
        if o.reconnect_interval_secs.is_some() {
            config.reconnect_interval_secs = o.reconnect_interval_secs;
        }
        if o.max_reconnect_attempts.is_some() {
            config.max_reconnect_attempts = o.max_reconnect_attempts;
        }
        if o.sync_batch_size.is_some() {
            config.sync_batch_size = o.sync_batch_size;
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
    crate::util::generate_test_token("insecure-secret", "flare-im-core", user_id, 3600, None, None)
}

/// 登录时存储选型，配合 [`super::IMClient::login`]。
pub enum LoginDbKind {
    #[cfg(feature = "lifecycle-sqlite")]
    Sqlite,
    IndexedDb(crate::store::StoreProvider),
}
