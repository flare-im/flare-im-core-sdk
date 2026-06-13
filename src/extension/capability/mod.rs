//! SDK 侧 **扩展能力** 框架。
//!
//! ## 调用方式
//!
//! 1. **直连 gRPC**（不经过注册表）：[`crate::client::api::CapabilityApi`] — `dispatch` / `rtc_*` 便捷方法。
//! 2. **经插件注册表**（与 `SdkCapabilityPlugin` 对齐）：[`SdkCapabilityRegistry::invoke`] 或 [`crate::client::IMClient::invoke_capability`]。
//!
//! 通话相关 `rtc.*` capability_id 与 JSON payload 由 core SDK 维护，并统一走 DATA capability。

pub mod call_event;
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

pub use plugins::call_av::AvCapabilityPlugin;
