//! Media adapter capability profile.

use crate::platform::adapters::{AdapterPlatform, AdapterProvisioning};
use crate::platform::runtime::PlatformKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaSourceSupport {
    FilePath,
    FileUri,
    Blob,
    Bytes,
    Asset,
    NativeUri,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MediaAdapterProfile {
    pub platform: AdapterPlatform,
    pub sources: Vec<MediaSourceSupport>,
    pub upload: AdapterProvisioning,
    pub cache: AdapterProvisioning,
}

impl MediaAdapterProfile {
    pub fn for_platform(platform: AdapterPlatform) -> Self {
        match platform {
            AdapterPlatform::Web => Self {
                platform,
                sources: vec![MediaSourceSupport::Blob, MediaSourceSupport::Bytes],
                upload: AdapterProvisioning::HostInjected,
                cache: AdapterProvisioning::HostInjected,
            },
            AdapterPlatform::ReactNative | AdapterPlatform::UniApp => Self {
                platform,
                sources: vec![
                    MediaSourceSupport::FileUri,
                    MediaSourceSupport::Asset,
                    MediaSourceSupport::Bytes,
                ],
                upload: AdapterProvisioning::HostInjected,
                cache: AdapterProvisioning::HostInjected,
            },
            AdapterPlatform::Android
            | AdapterPlatform::Ios
            | AdapterPlatform::Flutter
            | AdapterPlatform::Harmony => Self {
                platform,
                sources: vec![
                    MediaSourceSupport::FilePath,
                    MediaSourceSupport::FileUri,
                    MediaSourceSupport::NativeUri,
                    MediaSourceSupport::Asset,
                ],
                upload: AdapterProvisioning::BuiltIn,
                cache: AdapterProvisioning::BuiltIn,
            },
            AdapterPlatform::Native => Self {
                platform,
                sources: vec![MediaSourceSupport::FilePath, MediaSourceSupport::FileUri],
                upload: AdapterProvisioning::BuiltIn,
                cache: AdapterProvisioning::BuiltIn,
            },
        }
    }

    pub fn for_runtime_platform(platform: PlatformKind) -> Self {
        Self::for_platform(AdapterPlatform::from_runtime(platform))
    }

    pub fn requires_host_adapter(&self) -> bool {
        self.upload.is_host_injected() || self.cache.is_host_injected()
    }
}
