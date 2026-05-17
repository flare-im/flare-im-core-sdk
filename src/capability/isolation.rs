//! 能力插件隔离策略：定义开源核心保留命名空间，避免商业插件误覆盖核心行为。

use crate::capability::SdkCapabilityPlugin;

/// 开源核心保留能力命名空间（默认不允许外部插件覆盖）。
pub const CORE_RESERVED_CAPABILITY_NAMESPACES: &[&str] = &["rtc"];

#[must_use]
pub fn is_reserved_namespace(namespace: &str) -> bool {
    CORE_RESERVED_CAPABILITY_NAMESPACES
        .iter()
        .any(|n| *n == namespace)
}

/// 返回插件命中的核心保留命名空间（为空表示无冲突）。
#[must_use]
pub fn reserved_namespaces_of_plugin(plugin: &dyn SdkCapabilityPlugin) -> Vec<&'static str> {
    plugin
        .capability_namespaces()
        .iter()
        .copied()
        .filter(|ns| is_reserved_namespace(ns))
        .collect()
}
