//! Tauri 命令：生命周期手写 + 契约 invoke 由 codegen 生成。

pub mod lifecycle;

pub use crate::generated::invoke::sdk_invoke_json;
pub use lifecycle::*;

/// What every command rejects with: the structured error (`sdk_error_value`), so the
/// frontend reads `code`, `reason` and the typed facts instead of parsing a message.
pub type CommandError = flare_im_core_sdk::serde_json::Value;

#[inline]
pub(crate) fn map_sdk_err(e: flare_im_core_sdk::FlareError) -> CommandError {
    flare_im_core_sdk_bindings_runtime::sdk_error_value(&e, None)
}
