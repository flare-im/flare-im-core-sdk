//! 扩展点机制
//!
//! 提供扩展点接口，供业务SDK扩展

pub mod point;
pub mod manager;
pub mod provider;
pub mod providers;

pub use point::{
    ExtensionPoint, MessageHandlerExtension, EventListenerExtension,
    SyncStrategyExtension, StorageExtension,
};
pub use manager::ExtensionManager;
pub use provider::ExtensionManager as ExtensionInfoManager;
pub use providers::{
    StorageExtensionProvider,
    MemoryExtensionProvider,
    StorageExtensionCache,
    MemoryExtensionCache,
};

