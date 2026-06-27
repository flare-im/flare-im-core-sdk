//! Re-export shared WASM Tokio driver from `flare-core`.

#[cfg(target_arch = "wasm32")]
pub use flare_core::client::wasm_tokio::ensure_initialized;

#[cfg(all(target_arch = "wasm32", feature = "production-runtime"))]
pub use flare_core::client::wasm_tokio::{run_async as run_sdk, spawn_detached};

#[cfg(not(target_arch = "wasm32"))]
pub fn ensure_initialized() {}
