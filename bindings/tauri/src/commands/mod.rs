//! Tauri 命令：生命周期手写 + 契约 invoke 由 codegen 生成。

pub mod lifecycle;

pub use crate::generated::invoke::sdk_invoke;
pub use lifecycle::*;

#[inline]
pub(crate) fn map_sdk_err(e: flare_im_core_sdk::FlareError) -> String {
    e.to_string()
}
