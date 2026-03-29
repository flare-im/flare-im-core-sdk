//! 持久化（Repository + Store）
//!
//! - **repository**：仓储 trait 定义（Message/Conversation/PendingSend/UserProfile Query+Command）。
//! - **Store trait**：MessageStore / ConversationStore 供应用层与 StoreProvider 使用。
//! - **memory**：纯内存实现，用于 fallback（如未配置 SQLite 时的 user_profiles）。
//! - **sqlite**（feature `storage-sqlite`）：SQLite 仓储实现，并实现 MessageStore/ConversationStore 适配。
//! - **layered**：仅 PendingSend 的内存缓存 + 后端分层。

pub mod conversation_store;
pub mod db;
pub mod message_store;

pub mod memory;

/// 待发送队列：内存 + 后端分层（推荐与 sqlite 组合使用）
pub mod layered;

/// IndexedDB 便捷接入：实现 domain Reader/Writer 或 MessageStorageBackend + MessageBackendAdapter
pub mod indexeddb_adapter;

#[cfg(feature = "storage-sqlite")]
pub mod sqlite;

pub use conversation_store::ConversationStore;
pub use db::{StoreProvider, SyncCursorStore};
pub use indexeddb_adapter::{MessageBackendAdapter, MessageStorageBackend};
pub use layered::LayeredPendingSendStore;
pub use memory::{MemoryPendingSendStore, MemoryUserProfileStore};
pub use message_store::MessageStore;

#[cfg(feature = "storage-sqlite")]
pub use sqlite::{
    SqliteConversationRepo, SqliteMessageRepo, SqlitePendingSendRepo, SqliteSyncCursorRepo,
    SqliteUserProfileRepo, init_schema as sqlite_init_schema,
};
