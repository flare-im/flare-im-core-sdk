//! Storage adapter capability profile.

use crate::platform::adapters::{AdapterPlatform, AdapterProvisioning};
use crate::platform::runtime::{PlatformKind, StorageKind};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StorageAdapterProfile {
    pub platform: AdapterPlatform,
    pub preferred_kind: StorageKind,
    pub provisioning: AdapterProvisioning,
    pub per_user_namespace_required: bool,
}

impl StorageAdapterProfile {
    pub fn for_platform(platform: AdapterPlatform) -> Self {
        match platform {
            AdapterPlatform::Web => Self {
                platform,
                preferred_kind: StorageKind::IndexedDb,
                provisioning: AdapterProvisioning::HostInjected,
                per_user_namespace_required: true,
            },
            AdapterPlatform::ReactNative | AdapterPlatform::UniApp => Self {
                platform,
                preferred_kind: StorageKind::Sqlite,
                provisioning: AdapterProvisioning::HostInjected,
                per_user_namespace_required: true,
            },
            AdapterPlatform::Android
            | AdapterPlatform::Ios
            | AdapterPlatform::Flutter
            | AdapterPlatform::Harmony
            | AdapterPlatform::Native => Self {
                platform,
                preferred_kind: StorageKind::Sqlite,
                provisioning: AdapterProvisioning::BuiltIn,
                per_user_namespace_required: true,
            },
        }
    }

    pub fn for_runtime_platform(platform: PlatformKind) -> Self {
        Self::for_platform(AdapterPlatform::from_runtime(platform))
    }

    pub fn requires_host_adapter(&self) -> bool {
        self.provisioning.is_host_injected()
    }
}
