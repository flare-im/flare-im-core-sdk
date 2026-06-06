//! Tauri binding for Flare IM Core SDK vNext.
//!
//! This plugin is an IPC adapter over the shared JSON binding runtime.

use std::sync::Arc;

use flare_im_core_sdk::{IMClient, SdkConfig};
use tokio::sync::RwLock;

pub const BINDING_CONTRACT_VERSION: &str =
    flare_im_core_sdk_bindings_runtime::BINDING_CONTRACT_VERSION;

pub type SdkConfigOptions = SdkConfig;

#[derive(Clone, Default)]
pub struct SdkState {
    client: Arc<RwLock<Option<IMClient>>>,
}

impl SdkState {
    pub async fn client(&self) -> Result<IMClient, String> {
        self.client
            .read()
            .await
            .clone()
            .ok_or_else(|| "sdk is not initialized".to_string())
    }
}

pub mod commands {
    use flare_im_core_sdk::SdkConfig;
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    use tauri::State;

    use crate::{BINDING_CONTRACT_VERSION, SdkState};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SdkInitArgs {
        pub config: SdkConfig,
    }

    #[tauri::command]
    pub async fn sdk_contract_json() -> String {
        flare_im_core_sdk_bindings_runtime::contract_json()
    }

    #[tauri::command]
    pub async fn sdk_client_init_example_json() -> String {
        flare_im_core_sdk_bindings_runtime::client_init_request_example_json()
    }

    #[tauri::command]
    pub async fn sdk_init(state: State<'_, SdkState>, args: SdkInitArgs) -> Result<Value, String> {
        let client = flare_im_core_sdk::IMClient::new(args.config).map_err(|e| e.to_string())?;
        *state.client.write().await = Some(client);
        Ok(serde_json::json!({
            "binding_contract_version": BINDING_CONTRACT_VERSION
        }))
    }

    #[tauri::command]
    pub async fn sdk_release(state: State<'_, SdkState>) -> Result<Value, String> {
        *state.client.write().await = None;
        Ok(serde_json::json!({ "released": true }))
    }

    #[tauri::command]
    pub async fn sdk_invoke(
        state: State<'_, SdkState>,
        request_json: String,
    ) -> Result<String, String> {
        let client = state.client().await?;
        Ok(flare_im_core_sdk_bindings_runtime::invoke_json(&client, &request_json).await)
    }

    #[tauri::command]
    pub async fn sdk_generate_test_token(user_id: String) -> String {
        flare_im_core_sdk::generate_test_token(&user_id)
    }
}

pub use commands::SdkInitArgs;

/// Return the invoke handler for host apps:
/// `.manage(SdkState::default()).invoke_handler(im_invoke_handler())`.
pub fn im_invoke_handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + 'static {
    tauri::generate_handler![
        commands::sdk_contract_json,
        commands::sdk_client_init_example_json,
        commands::sdk_init,
        commands::sdk_release,
        commands::sdk_invoke,
        commands::sdk_generate_test_token
    ]
}
