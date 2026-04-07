//! Tauri 命令模块。
//!
//! 性能约定：先 [`crate::SdkState::client`] 再调 SDK Facade；错误统一 [`map_sdk_err`]。

pub mod conversation;
pub mod host_util;
pub mod lifecycle;
pub mod message;

pub use conversation::*;
pub use host_util::*;
pub use lifecycle::*;
pub use message::*;

#[inline]
pub(crate) fn map_sdk_err(e: flare_im_core_sdk::FlareError) -> String {
    e.to_string()
}
