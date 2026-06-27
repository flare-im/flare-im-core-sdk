//! Production browser runtime backed by real [`IMClient`] + WebSocket transport.

use std::sync::Arc;

use flare_im_core_sdk::client::lifecycle::LoginDbKind;
#[cfg(feature = "dev-test-token")]
use flare_im_core_sdk::client::{CoreTokenConfig, IMClient};
use flare_im_core_sdk::event::SharedEventReceiver;
use flare_im_core_sdk_bindings_runtime::{
    SessionTaskSlot, binding_response_to_value, invoke_api_id_json,
};
use js_sys::Function;
use serde::Serialize;
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use super::events::{clear_event_callback, forward_event_rx_to_js, set_event_callback};
use super::session::WasmSdkState;
use crate::tokio_runtime;
use flare_im_core_sdk_storage_indexeddb::{
    build_web_store_provider, clear_storage_host, set_storage_host, storage_host_configured,
};

fn js_error(code: &str, operation: &str, error: impl ToString) -> JsValue {
    JsValue::from_str(
        &json!({
            "code": code,
            "operation": operation,
            "message": error.to_string(),
        })
        .to_string(),
    )
}

fn map_sdk_err(operation: &str, error: flare_im_core_sdk::FlareError) -> JsValue {
    js_error("sdk.error", operation, error)
}

fn parse_request(request_json: &str) -> Result<Value, JsValue> {
    if request_json.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(request_json).map_err(|error| js_error("invalidParameter", "parse", error))
}

fn spawn_event_bridge(rx: SharedEventReceiver, bridge: SessionTaskSlot) {
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    bridge.replace(move || {
        let _ = cancel_tx.send(());
    });
    tokio_runtime::spawn_detached(forward_event_rx_to_js(rx, cancel_rx));
}

#[wasm_bindgen]
pub struct FlareImWasmRuntime {
    state: Arc<WasmSdkState>,
}

#[wasm_bindgen(js_name = createWasmRuntime)]
pub fn create_wasm_runtime() -> FlareImWasmRuntime {
    FlareImWasmRuntime::new()
}

#[wasm_bindgen]
impl FlareImWasmRuntime {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            state: Arc::new(WasmSdkState::new()),
        }
    }

    #[wasm_bindgen(js_name = setEventCallback)]
    pub fn set_event_callback(&self, callback: Option<Function>) {
        let _ = self;
        set_event_callback(callback);
    }

    /// Register JS IndexedDB persistence host callbacks before `sdk.login`.
    #[wasm_bindgen(js_name = setStorageHost)]
    pub fn set_storage_host(
        &self,
        load_snapshot: Function,
        save_message: Function,
        save_conversation: Function,
        save_cursor: Function,
        save_pending_send: Function,
        delete_message: Function,
        delete_conversation: Function,
        delete_pending_send: Function,
    ) {
        let _ = self;
        set_storage_host(
            load_snapshot,
            save_message,
            save_conversation,
            save_cursor,
            save_pending_send,
            delete_message,
            delete_conversation,
            delete_pending_send,
        );
    }

    #[wasm_bindgen(js_name = clearStorageHost)]
    pub fn clear_storage_host(&self) {
        let _ = self;
        clear_storage_host();
    }

    #[wasm_bindgen(js_name = storageHostConfigured)]
    pub fn storage_host_configured(&self) -> bool {
        let _ = self;
        storage_host_configured()
    }

    /// Sync export returning a JS Promise — avoids nested `block_on` inside wasm-bindgen `async fn`.
    #[wasm_bindgen]
    pub fn invoke(&self, operation: &str, request_json: &str) -> js_sys::Promise {
        let operation = operation.to_string();
        let request_json = request_json.to_string();
        let state = self.state.clone();
        future_to_promise(async move {
            tokio_runtime::run_sdk(
                async move { invoke_impl(state, &operation, &request_json).await },
            )
            .await
        })
    }

    pub fn dispose(&self) {
        clear_event_callback();
    }
}

async fn invoke_impl(
    state: Arc<WasmSdkState>,
    operation: &str,
    request_json: &str,
) -> Result<JsValue, JsValue> {
    let result = match operation {
        "sdk.create" => Ok(json!({ "handle": 1 })),
        "sdk.init" => {
            let request = parse_request(request_json)?;
            let environment = request
                .get("environment")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let sdk_config = if request.get("sdkConfig").is_some() {
                Some(
                    serde_json::from_value(
                        request.get("sdkConfig").cloned().unwrap_or(Value::Null),
                    )
                    .map_err(|e| js_error("invalidParameter", operation, e))?,
                )
            } else {
                serde_json::from_value(request.clone()).ok()
            };
            state
                .set_config(environment, sdk_config)
                .await
                .map_err(|e| map_sdk_err(operation, e))?;
            Ok(Value::Null)
        }
        "sdk.login" => {
            let request = parse_request(request_json)?;
            let user_id = request
                .get("userId")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| js_error("invalidParameter", operation, "userId is required"))?
                .to_string();
            let token = request
                .get("token")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_default();
            let token = if token.trim().is_empty() {
                None
            } else {
                Some(token.as_str())
            };
            let client = state.client();
            let session_state = state.clone();
            let event_bridge = session_state.event_bridge();
            event_bridge.clear();
            let event_bridge_for_login = event_bridge.clone();
            let store_provider = build_web_store_provider(&user_id).await;
            let login_result = client
                .login(
                    &user_id,
                    token,
                    LoginDbKind::IndexedDb(store_provider),
                    move |bus, _| {
                        let rx = bus.subscribe_shared_raw();
                        spawn_event_bridge(rx, event_bridge_for_login.clone());
                    },
                )
                .await;
            let apis = match login_result {
                Ok(apis) => apis,
                Err(err) => {
                    event_bridge.clear();
                    return Err(map_sdk_err(operation, err));
                }
            };
            session_state.install_session(apis).await;
            Ok(Value::Null)
        }
        "sdk.prepare" => {
            let request = parse_request(request_json)?;
            let user_id = request
                .get("userId")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| js_error("invalidParameter", operation, "userId is required"))?
                .to_string();
            let client = state.client();
            let event_bridge = state.event_bridge();
            event_bridge.clear();
            let store_provider = build_web_store_provider(&user_id).await;
            client
                .prepare(&user_id, LoginDbKind::IndexedDb(store_provider))
                .await
                .map_err(|e| map_sdk_err(operation, e))?;
            // 预热后立即订阅事件总线 → 转发 JS（等价 login 闭包在 connect 前所做）。
            let bus = client.bus().await.map_err(|e| map_sdk_err(operation, e))?;
            let rx = bus.subscribe_shared_raw();
            spawn_event_bridge(rx, event_bridge);
            Ok(Value::Null)
        }
        "sdk.connect" => {
            let request = parse_request(request_json)?;
            let user_id = request
                .get("userId")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| js_error("invalidParameter", operation, "userId is required"))?
                .to_string();
            let token = request
                .get("token")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_default();
            let token = if token.trim().is_empty() {
                None
            } else {
                Some(token.as_str())
            };
            let client = state.client();
            let session_state = state.clone();
            let apis = client
                .connect(&user_id, token)
                .await
                .map_err(|e| map_sdk_err(operation, e))?;
            session_state.install_session(apis).await;
            Ok(Value::Null)
        }
        "sdk.logout" => {
            state
                .logout()
                .await
                .map_err(|e| map_sdk_err(operation, e))?;
            clear_event_callback();
            Ok(Value::Null)
        }
        "sdk.uninit" => {
            state.clear_event_bridge();
            state.clear_session().await;
            state
                .client()
                .uninit()
                .await
                .map_err(|e| map_sdk_err(operation, e))?;
            Ok(Value::Null)
        }
        "sdk.dispose" | "sdk.hard_reset" => {
            clear_event_callback();
            state.clear_event_bridge();
            state.clear_session().await;
            let _ = state.client().logout().await;
            Ok(Value::Null)
        }
        "event.subscribe" => Ok(json!({ "id": 1 })),
        "event.unsubscribe" | "event.unsubscribe_all" => Ok(Value::Null),
        #[cfg(feature = "dev-test-token")]
        "sdk.generate_core_token" => {
            let request = parse_request(request_json)?;
            let user_id = request
                .get("userId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| js_error("invalidParameter", operation, "userId is required"))?;
            let secret = request
                .get("secret")
                .and_then(|v| v.as_str())
                .ok_or_else(|| js_error("invalidParameter", operation, "secret is required"))?;
            let issuer = request
                .get("issuer")
                .and_then(|v| v.as_str())
                .ok_or_else(|| js_error("invalidParameter", operation, "issuer is required"))?;
            let ttl_secs = request
                .get("ttlSecs")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| js_error("invalidParameter", operation, "ttlSecs is required"))?;
            let device_id = request
                .get("deviceId")
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            let tenant_id = request
                .get("tenantId")
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            let token = IMClient::generate_core_token(CoreTokenConfig {
                secret: secret.to_string(),
                issuer: issuer.to_string(),
                user_id: user_id.to_string(),
                ttl_secs,
                device_id,
                tenant_id,
            })
            .map_err(|e| map_sdk_err(operation, e))?;
            Ok(json!({ "token": token }))
        }
        operation => invoke_api_id_json(&*state, operation, request_json)
            .await
            .map(binding_response_to_value)
            .map_err(|e| map_sdk_err(operation, e)),
    }?;

    result
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|error| js_error("wasm.serialize_failed", operation, error))
}
