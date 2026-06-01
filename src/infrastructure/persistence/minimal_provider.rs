//! 最小内存 [`StoreProvider`]：空消息/会话实现 + 同步游标 + 可选用户资料缓存（供 IM 联调）。

use std::sync::Arc;

use super::StoreProvider;
use super::empty_stores::{EmptyConversationStore, EmptyMessageStore, MemorySyncCursorStore};
use super::memory::MemoryUserProfileStore;

/// 无 SQLite 时的 IM 空仓储（不含好友/群；社交域见 `flare-social-sdk::store`）。
pub fn in_memory_empty_im_provider() -> StoreProvider {
    let user_profiles = Arc::new(MemoryUserProfileStore::new());
    StoreProvider {
        messages: Arc::new(EmptyMessageStore),
        conversations: Arc::new(EmptyConversationStore),
        conversation_participants: None,
        cursors: Arc::new(MemorySyncCursorStore::new()),
        pending_send_reader: None,
        pending_send_writer: None,
        upload_manifest_store: None,
        media_cache_store: None,
        media_cache_admin: None,
        user_file_download_store: None,
        user_profiles_reader: Some(user_profiles.clone()),
        user_profiles_writer: Some(user_profiles),
    }
}
