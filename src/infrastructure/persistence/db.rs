use crate::domain::{
    ConversationParticipantStore, ConversationStore, MediaCacheAdmin, MediaCacheStore,
    MessageStore, PendingSendReader, PendingSendWriter, SyncCursorStore, UploadManifestStore,
    UserFileDownloadStore, UserReader, UserWriter,
};
use std::sync::Arc;

/// 存储提供者 — 统一持有各 Store；待发/用户资料使用 domain Reader/Writer 分开持有
pub struct StoreProvider {
    pub messages: Arc<dyn MessageStore>,
    pub conversations: Arc<dyn ConversationStore>,
    pub conversation_participants: Option<Arc<dyn ConversationParticipantStore>>,
    pub cursors: Arc<dyn SyncCursorStore>,
    pub pending_send_reader: Option<Arc<dyn PendingSendReader>>,
    pub pending_send_writer: Option<Arc<dyn PendingSendWriter>>,
    pub upload_manifest_store: Option<Arc<dyn UploadManifestStore>>,
    pub media_cache_store: Option<Arc<dyn MediaCacheStore>>,
    pub media_cache_admin: Option<Arc<dyn MediaCacheAdmin>>,
    pub user_file_download_store: Option<Arc<dyn UserFileDownloadStore>>,
    pub user_profiles_reader: Option<Arc<dyn UserReader>>,
    pub user_profiles_writer: Option<Arc<dyn UserWriter>>,
}

impl StoreProvider {
    /// 返回已配置的待发读端，用于可靠队列（与 pending_send_writer 配对使用）
    pub fn pending_sends(
        &self,
    ) -> Option<(Arc<dyn PendingSendReader>, Arc<dyn PendingSendWriter>)> {
        match (&self.pending_send_reader, &self.pending_send_writer) {
            (Some(r), Some(w)) => Some((r.clone(), w.clone())),
            _ => None,
        }
    }

    /// 返回已配置的用户资料读端，或默认内存实现（保证不为空）
    pub fn user_profiles_or_memory(&self) -> Arc<dyn UserReader> {
        self.user_profiles_reader.clone().unwrap_or_else(|| {
            Arc::new(crate::infrastructure::persistence::MemoryUserProfileStore::new())
        })
    }
}

impl Clone for StoreProvider {
    fn clone(&self) -> Self {
        Self {
            messages: self.messages.clone(),
            conversations: self.conversations.clone(),
            conversation_participants: self.conversation_participants.clone(),
            cursors: self.cursors.clone(),
            pending_send_reader: self.pending_send_reader.clone(),
            pending_send_writer: self.pending_send_writer.clone(),
            upload_manifest_store: self.upload_manifest_store.clone(),
            media_cache_store: self.media_cache_store.clone(),
            media_cache_admin: self.media_cache_admin.clone(),
            user_file_download_store: self.user_file_download_store.clone(),
            user_profiles_reader: self.user_profiles_reader.clone(),
            user_profiles_writer: self.user_profiles_writer.clone(),
        }
    }
}
