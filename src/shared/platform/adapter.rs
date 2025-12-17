//! 平台适配器
//!
//! 提供平台特定的功能适配

use crate::shared::platform::{Platform, PlatformCapabilities};
use async_trait::async_trait;
use std::sync::Arc;

/// 平台适配器 trait
///
/// 为不同平台提供统一的接口，隐藏平台差异
#[async_trait]
pub trait PlatformAdapter: Send + Sync {
    /// 获取平台类型
    fn platform(&self) -> Platform;

    /// 获取平台能力
    fn capabilities(&self) -> PlatformCapabilities;

    /// 获取设备 ID
    fn device_id(&self) -> String;

    /// 获取应用版本
    fn app_version(&self) -> Option<String>;

    /// 获取系统版本
    fn system_version(&self) -> String;

    /// 是否支持后台运行
    fn supports_background(&self) -> bool {
        self.capabilities().background
    }

    /// 是否支持推送通知
    fn supports_push(&self) -> bool {
        self.capabilities().push
    }

    /// 初始化平台特定的资源
    async fn initialize(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// 清理平台特定的资源
    async fn cleanup(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// 默认平台适配器
///
/// 提供基本的平台适配，不包含平台特定的功能
pub struct DefaultPlatformAdapter {
    platform: Platform,
    capabilities: PlatformCapabilities,
}

impl DefaultPlatformAdapter {
    pub fn new(platform: Platform) -> Self {
        let capabilities = match platform {
            Platform::Web => PlatformCapabilities::web(),
            Platform::Desktop => PlatformCapabilities::desktop(),
            Platform::Android | Platform::IOS | Platform::HarmonyOS => {
                PlatformCapabilities::mobile()
            }
        };

        Self {
            platform,
            capabilities,
        }
    }
}

#[async_trait]
impl PlatformAdapter for DefaultPlatformAdapter {
    fn platform(&self) -> Platform {
        self.platform
    }

    fn capabilities(&self) -> PlatformCapabilities {
        self.capabilities.clone()
    }

    fn device_id(&self) -> String {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use uuid::Uuid;
            Uuid::new_v4().to_string()
        }

        #[cfg(target_arch = "wasm32")]
        {
            use js_sys::{Date, Math};
            use wasm_bindgen::prelude::*;
            let now = Date::now();
            let rnd = Math::floor(Math::random() * 1_000_000.0) as u32;
            format!("web-{}-{}", now as u64, rnd)
        }
    }

    fn app_version(&self) -> Option<String> {
        None
    }

    fn system_version(&self) -> String {
        match self.platform {
            Platform::Web => "Web Browser".to_string(),
            Platform::Desktop => {
                #[cfg(target_os = "windows")]
                return "Windows".to_string();
                #[cfg(target_os = "macos")]
                return "macOS".to_string();
                #[cfg(target_os = "linux")]
                return "Linux".to_string();
                #[cfg(not(any(
                    target_os = "windows",
                    target_os = "macos",
                    target_os = "linux"
                )))]
                return "Unknown".to_string();
            }
            Platform::Android => "Android".to_string(),
            Platform::IOS => "iOS".to_string(),
            Platform::HarmonyOS => "HarmonyOS".to_string(),
        }
    }
}

/// 平台适配器类型别名
pub type ArcPlatformAdapter = Arc<dyn PlatformAdapter>;
