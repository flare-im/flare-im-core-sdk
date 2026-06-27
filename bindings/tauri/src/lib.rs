//! Tauri binding for Flare IM Core SDK.
//!
//! This crate is an IPC adapter over the shared binding runtime. Dedicated
//! commands are limited to lifecycle paths that need `AppHandle` or host state;
//! all normal SDK operations go through generated `sdk_invoke_json`.

pub mod commands;
pub mod convert;
pub mod generated;
pub mod state;

pub mod model {
    use flare_im_core_sdk::client::SdkConfigOverlay;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SdkInitArgs {
        #[serde(default)]
        pub environment: Option<String>,
        #[serde(default, rename = "sdkConfig")]
        pub sdk_config: Option<SdkConfigOverlay>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct RtcIceConfigSnapshotPayload {
        pub source: String,
        pub turn_enabled: bool,
        pub default_ice_tf: String,
        pub ice_servers: serde_json::Value,
    }
}

pub const BINDING_CONTRACT_VERSION: &str =
    flare_im_core_sdk_bindings_runtime::BINDING_CONTRACT_VERSION;

pub type SdkConfigOptions = flare_im_core_sdk::client::SdkConfigOverlay;

pub use commands::*;
pub use generated::handler::im_invoke_handler;
pub use model::{RtcIceConfigSnapshotPayload, SdkInitArgs};
pub use state::SdkState;
