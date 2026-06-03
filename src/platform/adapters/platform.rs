//! Shared platform adapter taxonomy.
//!
//! `PlatformKind` is the exact runtime reported by the host. `AdapterPlatform`
//! is the coarser family used to select media/storage adapter capabilities.

use crate::platform::runtime::PlatformKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterPlatform {
    Web,
    ReactNative,
    UniApp,
    Android,
    Ios,
    Flutter,
    Harmony,
    Native,
}

impl AdapterPlatform {
    pub fn from_runtime(platform: PlatformKind) -> Self {
        match platform {
            PlatformKind::Web => Self::Web,
            PlatformKind::ReactNative => Self::ReactNative,
            PlatformKind::UniApp => Self::UniApp,
            PlatformKind::Android => Self::Android,
            PlatformKind::Ios => Self::Ios,
            PlatformKind::Flutter => Self::Flutter,
            PlatformKind::HarmonyArkTs | PlatformKind::HarmonyCangjie => Self::Harmony,
            PlatformKind::Electron | PlatformKind::Tauri | PlatformKind::Native => Self::Native,
        }
    }
}

impl From<PlatformKind> for AdapterPlatform {
    fn from(platform: PlatformKind) -> Self {
        Self::from_runtime(platform)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterProvisioning {
    BuiltIn,
    HostInjected,
}

impl AdapterProvisioning {
    pub fn is_host_injected(self) -> bool {
        matches!(self, Self::HostInjected)
    }
}
