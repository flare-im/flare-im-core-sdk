//! Local browser smoke runtime.
//!
//! This module is intentionally feature-gated. It provides the current
//! in-memory web runtime used by TypeScript smoke tests while the production
//! wasm adapter is still being wired to the shared SDK facade.

mod runtime;
mod web_model;

pub use runtime::*;
