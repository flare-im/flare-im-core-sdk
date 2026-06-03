//! SDK 侧 **扩展能力** 框架。
//!
//! ## 调用方式
//!
//! 1. **直连 gRPC**（不经过注册表）：[`crate::client::api::CapabilityApi`] — `dispatch` / `rtc_*` 便捷方法。
//! 2. **经插件注册表**（与 `SdkCapabilityPlugin` 对齐）：[`SdkCapabilityRegistry::invoke`] 或 [`crate::client::IMClient::invoke_capability`]。
//!
//! 通话相关 `rtc.*` capability_id 与 JSON payload 与 **`flare-sdk-plugin-call::rtc`** 一致（启用 `plugin-call` 时由该 crate 提供；否则使用本模块内对齐的 fallback）。

pub mod call_event;
#[cfg(feature = "plugin-call")]
pub mod call_experience;
mod isolation;
mod plugin;
mod registry;
pub mod rtc_ids;

mod plugins;

pub use isolation::{
    CORE_RESERVED_CAPABILITY_NAMESPACES, is_reserved_namespace, reserved_namespaces_of_plugin,
};
pub use plugin::SdkCapabilityPlugin;
pub use registry::SdkCapabilityRegistry;

#[cfg(feature = "plugin-call")]
pub use call_experience::{
    AvExperienceSpec, CallControlSet, CallLayoutMode, ExperienceEdition,
    default_call_experience_spec, sanitize_experience_spec_for_edition,
};
pub use plugins::call_av::AvCapabilityPlugin;
