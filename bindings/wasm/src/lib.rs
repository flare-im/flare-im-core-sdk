//! Browser WebAssembly binding for Flare IM Core SDK vNext.

use flare_im_core_sdk::{IMClient, SdkConfig};
use serde_json::Value;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn wasm_start() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen(js_name = flareBindingContractVersion)]
pub fn flare_binding_contract_version() -> String {
    flare_im_core_sdk_bindings_runtime::BINDING_CONTRACT_VERSION.to_string()
}

#[wasm_bindgen(js_name = flareBindingContractJson)]
pub fn flare_binding_contract_json() -> String {
    flare_im_core_sdk_bindings_runtime::contract_json()
}

#[wasm_bindgen(js_name = flareClientInitExampleJson)]
pub fn flare_client_init_example_json() -> String {
    flare_im_core_sdk_bindings_runtime::client_init_request_example_json()
}

#[wasm_bindgen(js_name = flareGenerateTestToken)]
pub fn flare_generate_test_token(user_id: &str) -> String {
    flare_im_core_sdk::generate_test_token(user_id)
}

#[wasm_bindgen]
pub struct FlareImClient {
    client: IMClient,
}

#[wasm_bindgen]
impl FlareImClient {
    #[wasm_bindgen(constructor)]
    pub fn new(config: JsValue) -> Result<FlareImClient, JsValue> {
        let config: SdkConfig = serde_wasm_bindgen::from_value(config)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let client = IMClient::new(config).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Self { client })
    }

    #[wasm_bindgen(js_name = invoke)]
    pub async fn invoke(&self, request: JsValue) -> Result<JsValue, JsValue> {
        let value: Value = serde_wasm_bindgen::from_value(request)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let request =
            serde_json::from_value::<flare_im_core_sdk_bindings_runtime::BindingRequest>(value)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let response = flare_im_core_sdk_bindings_runtime::invoke_value(&self.client, request)
            .await
            .unwrap_or_else(flare_im_core_sdk_bindings_runtime::BindingResponse::err);
        serde_wasm_bindgen::to_value(&response).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    #[wasm_bindgen(js_name = invokeJson)]
    pub async fn invoke_json(&self, request_json: String) -> String {
        flare_im_core_sdk_bindings_runtime::invoke_json(&self.client, &request_json).await
    }

    #[wasm_bindgen(js_name = pollEvent)]
    pub async fn poll_event(&self) -> Result<JsValue, JsValue> {
        let event = self.client.poll_event().await;
        serde_wasm_bindgen::to_value(&event).map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
