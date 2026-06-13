//! Browser WebAssembly binding for Flare IM Core SDK.
//!
//! The public wasm surface is generated metadata plus a thin runtime facade.
//! Production builds use the real `IMClient`; smoke builds can opt into the
//! in-memory runtime with `--features local-smoke-runtime`.

use wasm_bindgen::prelude::*;

pub mod generated;
#[allow(dead_code)]
mod runtime_port;
mod tokio_runtime;

#[cfg(feature = "local-smoke-runtime")]
mod smoke;

#[cfg(not(feature = "local-smoke-runtime"))]
mod production;

pub use generated::bindings::flare_binding_contract_version;
pub use generated::client_config::{
    flare_client_config_contract_json, flare_client_init_example_json,
};

#[cfg(not(feature = "local-smoke-runtime"))]
pub use production::{
    FlareImWasmRuntime, build_web_store_provider, clear_storage_host, create_wasm_runtime,
    flare_clear_encryption_key, flare_encryption_key_len, flare_has_encryption_key,
    flare_now_rfc3339, flare_runtime_id, flare_set_encryption_key, flare_set_encryption_key_hex,
    flare_wall_clock_ms, set_storage_host, storage_host_configured,
};

#[cfg(feature = "local-smoke-runtime")]
pub use generated::bindings::flare_invoke;

#[cfg(feature = "local-smoke-runtime")]
pub use smoke::{FlareImWasmRuntime, create_wasm_runtime};

#[wasm_bindgen(start)]
pub fn wasm_start() {
    console_error_panic_hook::set_once();
    tokio_runtime::ensure_initialized();
}

#[wasm_bindgen(js_name = flareBindingContractJson)]
pub fn flare_binding_contract_json() -> String {
    flare_im_core_sdk_bindings_runtime::contract_json()
}
