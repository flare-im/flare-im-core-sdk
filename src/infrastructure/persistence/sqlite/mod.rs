//! SQLite 仓储实现 — 实现 domain Reader/Writer，需启用 feature `storage-sqlite`。
//!
//! 仅负责 CRUD 与表结构；连接池由调用方创建（见 [storage/sqlite] 的 `create_pool`）。

mod conversation_participant_repo;
mod conversation_repo;
mod cursor_repo;
mod media_cache_repo;
mod message_repo;
mod pending_send_repo;
mod schema;
mod upload_manifest_repo;
mod user_file_download_repo;
mod user_repo;

pub use conversation_participant_repo::SqliteConversationParticipantRepo;
pub use conversation_repo::SqliteConversationRepo;
pub use cursor_repo::SqliteSyncCursorRepo;
pub use media_cache_repo::SqliteMediaCacheRepo;
pub use message_repo::SqliteMessageRepo;
pub use pending_send_repo::SqlitePendingSendRepo;
pub use schema::init_schema;
pub use upload_manifest_repo::SqliteUploadManifestRepo;
pub use user_file_download_repo::SqliteUserFileDownloadRepo;
pub use user_repo::SqliteUserProfileRepo;
