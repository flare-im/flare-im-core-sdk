pub mod cache;
pub mod cache_layer;
pub mod media_upload;
pub mod pending_message_queue;
pub mod storage_trait;

pub use cache::CachedStorage;
pub use cache_layer::{CacheStats, CachedStorageBackend, QueryCache};
pub use media_upload::{
    MediaInfo, MediaType, MediaUploadOptions, MediaUploadResult, MediaUploadService, UploadProgress,
};
pub use pending_message_queue::{PendingMessage, PendingMessageQueue, PendingMessageQueueConfig};
pub use storage_trait::{
    LastMessageUpdate, MessageState, SessionFilter, SessionUpdate, StorageBackend,
};

// 平台特定的实现
#[cfg(not(target_arch = "wasm32"))]
pub mod sqlite;

#[cfg(target_arch = "wasm32")]
pub mod indexeddb;
