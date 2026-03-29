//! SQLite 仓储实现 — 实现 domain Reader/Writer，需启用 feature `storage-sqlite`。
//!
//! 仅负责 CRUD 与表结构；连接池由调用方创建（见 [storage/sqlite] 的 `create_pool`）。

mod conversation_repo;
mod cursor_repo;
mod message_repo;
mod pending_send_repo;
mod schema;
mod user_repo;

pub use conversation_repo::SqliteConversationRepo;
pub use cursor_repo::SqliteSyncCursorRepo;
pub use message_repo::SqliteMessageRepo;
pub use pending_send_repo::SqlitePendingSendRepo;
pub use schema::init_schema;
pub use user_repo::SqliteUserProfileRepo;
