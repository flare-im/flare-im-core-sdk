//! Extension 模块
//!
//! 业务能力通过 Extension 接入

#[cfg(feature = "extensions")]
pub mod friend;
#[cfg(feature = "extensions")]
pub mod group;

#[cfg(feature = "extensions")]
pub use friend::FriendExtension;
#[cfg(feature = "extensions")]
pub use group::GroupExtension;
