//! Web/WASM presence facade.
//!
//! Native platforms use `presence/native.rs` with gRPC. Browser runtimes
//! provide presence through the Web adapter or a capability plugin.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::core::event::EventBus;
use crate::infrastructure::transport::http::http_client::HttpRequestContext;
use crate::shared::error::{ErrorCode, FlareError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePresenceDto {
    pub device_id: String,
    pub platform: String,
    pub last_active_time_ms: i64,
    pub conversation_id: String,
    pub gateway_id: String,
    pub server_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPresenceDto {
    pub user_id: String,
    pub is_online: bool,
    pub status: String,
    pub last_seen_ms: i64,
    pub devices: Vec<DevicePresenceDto>,
}

#[derive(Clone)]
pub struct PresenceApi;

impl PresenceApi {
    pub fn new(
        _grpc_endpoint: impl Into<String>,
        _current_user_id: Arc<RwLock<String>>,
        _default_tenant_id: impl Into<String>,
        _http_request_context: Arc<HttpRequestContext>,
        _bus: EventBus,
    ) -> Self {
        Self
    }

    pub async fn get_user_presence(&self, user_id: &str) -> Result<UserPresenceDto> {
        let uid = user_id.trim();
        if uid.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "sdk.presence.user_id_required",
            ));
        }
        Err(wasm_presence_unavailable("get_user_presence"))
    }

    pub async fn batch_get_user_presence(
        &self,
        _user_ids: &[String],
    ) -> Result<HashMap<String, UserPresenceDto>> {
        Err(wasm_presence_unavailable("batch_get_user_presence"))
    }

    pub async fn logout_current_device_presence(&self) -> Result<()> {
        Ok(())
    }

    pub async fn subscribe_user_presence(&self, _user_ids: Vec<String>) -> Result<()> {
        Err(wasm_presence_unavailable("subscribe_user_presence"))
    }
}

fn wasm_presence_unavailable(operation: &str) -> FlareError {
    FlareError::localized(
        ErrorCode::OperationNotSupported,
        format!("{operation} requires a Web presence adapter"),
    )
}
