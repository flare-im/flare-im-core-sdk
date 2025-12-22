//! 平台模块（占位）
//!
//! TODO: 实现平台相关功能

/// 平台类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Desktop,
    Mobile,
    Android,
    IOS,
    Web,
    HarmonyOS,
}

/// 获取当前平台
pub fn get_platform() -> Platform {
    #[cfg(target_arch = "wasm32")]
    return Platform::Web;
    
    #[cfg(target_os = "android")]
    return Platform::Android;
    
    #[cfg(target_os = "ios")]
    return Platform::IOS;
    
    #[cfg(target_os = "harmonyos")]
    return Platform::HarmonyOS;
    
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios", target_os = "harmonyos")))]
    return Platform::Desktop;
}
