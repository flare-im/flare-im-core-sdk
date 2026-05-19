//! Tauri 命令模块。
//!
//! 性能约定：已登录热路径用 [`crate::SdkState::message_api`] 等会话快照；生命周期/信令仍用 [`crate::SdkState::client`]；错误统一 [`map_sdk_err`]。

pub mod call_signal;
pub mod capability;
pub mod conversation;
pub mod host_util;
pub mod lifecycle;
pub mod media;
pub mod message;
pub mod presence;
pub mod rich_doc_v2;

pub use call_signal::*;
pub use capability::*;
pub use conversation::*;
pub use host_util::*;
pub use lifecycle::*;
pub use media::*;
pub use message::*;
pub use presence::*;
pub use rich_doc_v2::*;

#[inline]
pub(crate) fn map_sdk_err(e: flare_im_core_sdk::FlareError) -> String {
    e.to_string()
}
