//! 扩展点机制
//!
//! 提供扩展点接口，供业务SDK扩展

#[cfg(feature = "extensions")]
pub mod bridge;
pub mod business;
pub mod manager;
pub mod point;
pub mod provider;
pub mod providers;
#[cfg(feature = "extensions")]
pub mod registry;

pub use manager::ExtensionManager;
pub use point::{
    EventListenerExtension, ExtensionPoint, MessageHandlerExtension, StorageExtension,
    SyncStrategyExtension,
};
pub use provider::ExtensionManager as ExtensionInfoManager;
pub use providers::{
    MemoryExtensionCache, MemoryExtensionProvider, StorageExtensionCache, StorageExtensionProvider,
};

#[cfg(feature = "extensions")]
pub use bridge::{
    ChannelExtensionBridge, CompositeExtensionProvider, GroupExtensionBridge, UserExtensionBridge,
};
#[cfg(feature = "extensions")]
pub use business::{
    BusinessDomain, BusinessExtensionPoint, ChannelBusinessExtension, ChannelInfo,
    ChannelMembersResult, ChannelType, GroupBusinessExtension, GroupChangeCallback,
    GroupChangeEvent, GroupChangeType, GroupInfo, GroupMember, GroupMemberRole, GroupMembersResult,
    OnlineStatus, UserBusinessExtension, UserChangeCallback, UserChangeEvent, UserChangeType,
    UserInfo,
};
#[cfg(feature = "extensions")]
pub use registry::{BusinessExtensionRegistry, ExtensionInfo, HealthCheckResult};
