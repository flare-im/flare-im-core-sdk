//! Shared binding runtime for Flare IM Core SDK vNext.
//!
//! C, Tauri, UniFFI, and Wasm bindings use this crate as the thin JSON
//! boundary. Stable IM behavior lives in `flare-im-core-sdk`.

use flare_im_core_sdk::{FlareError, IMClient, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const BINDING_CONTRACT_VERSION: &str = "flare.im.core-sdk.binding.vnext.1";
pub const API_CONTRACT_VERSION: &str = "flare.im.core-sdk.api.vnext.1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingRequest {
    pub route: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingResponse {
    pub ok: bool,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub error: Option<BindingError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingError {
    pub code: String,
    pub message: String,
}

impl BindingResponse {
    pub fn ok(data: Value) -> Self {
        Self {
            ok: true,
            data,
            error: None,
        }
    }

    pub fn err(error: FlareError) -> Self {
        Self {
            ok: false,
            data: Value::Null,
            error: Some(BindingError {
                code: format!("{:?}", error.code),
                message: error.message,
            }),
        }
    }
}

pub fn contract_json() -> String {
    json!({
        "binding_contract_version": BINDING_CONTRACT_VERSION,
        "api_contract_version": API_CONTRACT_VERSION,
        "routes": [
            "sdk.connect",
            "sdk.disconnect",
            "sdk.state",
            "sdk.snapshot",
            "events.poll",
            "outbox.drain",
            "message.send_text",
            "message.list",
            "conversation.list",
            "capability.send",
            "capability.dispatch"
        ]
    })
    .to_string()
}

pub fn client_init_request_example_json() -> String {
    json!({
        "endpoint": "wss://im.example.com",
        "tenant_id": "default",
        "user_id": "user_123",
        "device_id": "device_mobile_1",
        "access_token": "replace-with-token",
        "transport": "web_socket",
        "outbound_queue_capacity": 1024,
        "event_buffer_capacity": 1024
    })
    .to_string()
}

pub async fn invoke_value(client: &IMClient, request: BindingRequest) -> Result<BindingResponse> {
    client
        .invoke_json_value(&request.route, request.params)
        .await
        .map(BindingResponse::ok)
}

pub async fn invoke_json(client: &IMClient, request_json: &str) -> String {
    let response = async {
        let request = serde_json::from_str::<BindingRequest>(request_json)
            .map_err(|e| flare_im_core_sdk::FlareError::invalid_argument(e.to_string()))?;
        invoke_value(client, request).await
    }
    .await
    .unwrap_or_else(BindingResponse::err);

    serde_json::to_string(&response).unwrap_or_else(|_| {
        r#"{"ok":false,"data":null,"error":{"code":"Internal","message":"json encode failed"}}"#
            .to_string()
    })
}
