//! Platform boundary.
//!
//! Everything in this module represents runtime differences between Web,
//! React Native, uni-app, Android, iOS, Flutter, Tauri/Electron, and native
//! desktop builds. Domain and application code should depend on the ports here
//! and let client/runtime assembly choose concrete adapters.

pub mod adapters;
pub mod ports;
pub mod runtime;

pub use adapters::{
    AdapterPlatform, AdapterProvisioning, MediaAdapterProfile, MediaSourceSupport,
    PlatformAdapterProfile, StorageAdapterProfile, UploadOnlyMediaService,
};
pub use runtime::{
    MediaRuntimeConfig, MediaRuntimeKind, NativeRuntimeAssembler, PlatformKind, RuntimeAssembler,
    RuntimeComponents, RuntimeConfig, StorageConfig, StorageKind,
};
