pub mod storage_trait;
pub mod cache;

pub use storage_trait::{
    StorageBackend, SessionFilter, SessionUpdate, LastMessageUpdate, MessageState,
};
pub use cache::CachedStorage;

// 平台特定的实现
#[cfg(not(target_arch = "wasm32"))]
pub mod sqlite;

#[cfg(target_arch = "wasm32")]
pub mod indexeddb;
