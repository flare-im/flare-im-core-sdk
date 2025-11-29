//! 平台检测
//!
//! 自动检测当前运行平台

/// 支持的平台类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    /// Web 平台（wasm32）
    Web,
    
    /// 桌面平台（Windows/macOS/Linux）
    Desktop,
    
    /// Android 平台
    Android,
    
    /// iOS 平台
    IOS,
    
    /// HarmonyOS 平台
    HarmonyOS,
}

impl Platform {
    /// 获取平台名称
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::Web => "web",
            Platform::Desktop => "desktop",
            Platform::Android => "android",
            Platform::IOS => "ios",
            Platform::HarmonyOS => "harmonyos",
        }
    }
    
    /// 是否为移动平台
    pub fn is_mobile(&self) -> bool {
        matches!(self, Platform::Android | Platform::IOS | Platform::HarmonyOS)
    }
    
    /// 是否为桌面平台
    pub fn is_desktop(&self) -> bool {
        matches!(self, Platform::Desktop)
    }
    
    /// 是否为 Web 平台
    pub fn is_web(&self) -> bool {
        matches!(self, Platform::Web)
    }
}

/// 检测当前平台
pub fn detect_platform() -> Platform {
    // 首先检查是否为 Web 平台
    #[cfg(target_arch = "wasm32")]
    {
        return Platform::Web;
    }
    
    // 检查是否为 Android
    #[cfg(target_os = "android")]
    {
        return Platform::Android;
    }
    
    // 检查是否为 iOS
    #[cfg(target_os = "ios")]
    {
        return Platform::IOS;
    }
    
    // 检查是否为 HarmonyOS
    // 注意：HarmonyOS 可能使用特定的 target，这里需要根据实际情况调整
    #[cfg(target_os = "harmonyos")]
    {
        return Platform::HarmonyOS;
    }
    
    // 默认桌面平台（Windows/macOS/Linux）
    #[cfg(not(any(
        target_arch = "wasm32",
        target_os = "android",
        target_os = "ios",
        target_os = "harmonyos"
    )))]
    {
        return Platform::Desktop;
    }
    
    // 如果所有条件都不匹配，返回桌面平台作为默认值
    #[allow(unreachable_code)]
    Platform::Desktop
}

/// 获取当前平台（缓存版本）
pub fn get_platform() -> Platform {
    use std::sync::OnceLock;
    
    static PLATFORM: OnceLock<Platform> = OnceLock::new();
    
    *PLATFORM.get_or_init(detect_platform)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_platform_detection() {
        let platform = detect_platform();
        assert!(matches!(
            platform,
            Platform::Web | Platform::Desktop | Platform::Android | Platform::IOS | Platform::HarmonyOS
        ));
    }
    
    #[test]
    fn test_platform_methods() {
        let web = Platform::Web;
        assert!(web.is_web());
        assert!(!web.is_mobile());
        assert!(!web.is_desktop());
        
        let android = Platform::Android;
        assert!(!android.is_web());
        assert!(android.is_mobile());
        assert!(!android.is_desktop());
        
        let desktop = Platform::Desktop;
        assert!(!desktop.is_web());
        assert!(!desktop.is_mobile());
        assert!(desktop.is_desktop());
    }
}

