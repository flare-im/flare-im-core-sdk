//! 消息与会话 **Facade**：对应用层引擎与构建器的稳定调用面。
//!
//! 由 [`crate::client::IMClient`] 在连接后持有；事件订阅为 `IMClient::on_*`（见 [`crate::client::events`]，委托 [`crate::event::EventBus`]）。

mod capability;
mod conversation;
mod media;
mod message;
mod message_build;
mod presence;

pub use capability::{
    CapabilityApi, CapabilityDescriptorDto, CapabilityDispatchResult, UserCapabilityGrantDto,
};
pub use conversation::ConversationApi;
pub use media::{
    FileDownloadProgress, FileDownloadProgressCallback, MediaApi, UploadPhase, UploadProgress,
    UploadProgressCallback,
};
pub use message::MessageApi;
pub use message_build::MessageBuildApi;
pub use presence::{DevicePresenceDto, PresenceApi, UserPresenceDto};
