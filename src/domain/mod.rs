//! 领域层：消息、会话、用户展示模型
//!
//! 所有模型均包含展示用**显示名称（display_name）**与**头像（avatar_url）**，
//! 由本地 UserProfile 缓存或同步填充，供 UI 列表与详情展示。

pub mod conversation;
mod media_cache;
mod message;
mod pending_send;
mod repository;
mod sync;
mod sync_cursor;
mod upload_manifest;
mod user_profile;

pub use conversation::*;
pub use media_cache::{MediaCacheEntryVo, MediaCacheStatsVo};
pub use message::*;
pub use pending_send::PendingSendVo;
pub use repository::*;
pub use sync::*;
pub use sync_cursor::SyncCursorVo;
pub use upload_manifest::{
    DirectUploadTransportKindVo, MediaUploadManifestVo, MediaUploadPartVo, UploadManifestState,
    UploadSourceKind,
};
pub use user_profile::UserProfile;
