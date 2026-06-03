//! Storage ports.
//!
//! The concrete implementation can be SQLite, IndexedDB, memory, or a custom
//! host bridge. Core synchronization and projection logic must only use these
//! store contracts.

pub use crate::domain::{
    ConversationParticipantStore, ConversationReader, ConversationStore, ConversationWriter,
    MediaCacheAdmin, MediaCacheStore, MessageReader, MessageStore, MessageWriter,
    PendingSendReader, PendingSendWriter, SyncCursorReader, SyncCursorStore, SyncCursorWriter,
    UploadManifestStore, UserFileDownloadStore, UserReader, UserWriter,
};
pub use crate::infrastructure::persistence::StoreProvider;
