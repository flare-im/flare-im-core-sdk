//! Browser WebAssembly binding for Flare IM Core SDK.
//!
//! Keep this crate as a platform adapter. Stable IM behavior belongs in
//! `flare-im-core-sdk/src`; this binding should only translate wasm-bindgen
//! calls, JSON payloads, lifecycle hooks, and JavaScript error surfaces.

mod local_smoke_runtime;
mod operation;
mod runtime_port;
mod web_model;

pub use local_smoke_runtime::*;
pub use runtime_port::{WasmRuntimeError, WasmRuntimePort};
