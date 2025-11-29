//! 平台抽象层
//!
//! 提供统一的平台检测、适配和平台特定的功能抽象
//!
//! 支持平台：
//! - Web (wasm32)
//! - Desktop (Windows/macOS/Linux)
//! - Android
//! - iOS
//! - HarmonyOS

mod detection;
mod adapter;
mod capabilities;

pub use detection::{Platform, detect_platform, get_platform};
pub use adapter::{PlatformAdapter, DefaultPlatformAdapter};
pub use capabilities::{PlatformCapabilities, StorageType, NetworkType};

/// 平台特定的配置
#[derive(Debug, Clone)]
pub struct PlatformConfig {
    /// 平台类型
    pub platform: Platform,
    
    /// 存储类型
    pub storage_type: StorageType,
    
    /// 网络类型
    pub network_type: NetworkType,
    
    /// 是否支持后台运行
    pub supports_background: bool,
    
    /// 是否支持推送通知
    pub supports_push: bool,
    
    /// 内存限制（MB，None 表示无限制）
    pub memory_limit_mb: Option<usize>,
    
    /// 是否启用性能优化
    pub enable_performance_optimization: bool,
}

impl PlatformConfig {
    /// 创建默认平台配置（自动检测）
    pub fn default() -> Self {
        let platform = get_platform();
        Self::for_platform(platform)
    }
    
    /// 为指定平台创建配置
    pub fn for_platform(platform: Platform) -> Self {
        let (storage_type, network_type, supports_background, supports_push, memory_limit_mb) = match platform {
            Platform::Web => (
                StorageType::IndexedDB,
                NetworkType::WebSocket, // Web 不支持 QUIC
                false,
                false,
                Some(50), // Web 端内存限制较小
            ),
            Platform::Desktop => (
                StorageType::SQLite,
                NetworkType::Both, // 桌面端支持 WebSocket 和 QUIC
                true,
                true,
                None, // 桌面端无内存限制
            ),
            Platform::Android => (
                StorageType::SQLite,
                NetworkType::Both,
                true,
                true,
                Some(200), // Android 端内存限制
            ),
            Platform::IOS => (
                StorageType::SQLite,
                NetworkType::Both,
                true,
                true,
                Some(200), // iOS 端内存限制
            ),
            Platform::HarmonyOS => (
                StorageType::SQLite,
                NetworkType::Both,
                true,
                true,
                Some(200), // HarmonyOS 端内存限制
            ),
        };
        
        Self {
            platform,
            storage_type,
            network_type,
            supports_background,
            supports_push,
            memory_limit_mb,
            enable_performance_optimization: true,
        }
    }
    
    /// 是否支持 QUIC
    pub fn supports_quic(&self) -> bool {
        matches!(self.network_type, NetworkType::QUIC | NetworkType::Both)
    }
    
    /// 是否支持 WebSocket
    pub fn supports_websocket(&self) -> bool {
        matches!(self.network_type, NetworkType::WebSocket | NetworkType::Both)
    }
}

impl Default for PlatformConfig {
    fn default() -> Self {
        Self::for_platform(get_platform())
    }
}

