use crate::domain::{PendingSendReader, PendingSendWriter, SyncCursorVo, UserReader, UserWriter};
use crate::infrastructure::persistence::{ConversationStore, MessageStore};
use async_trait::async_trait;
use std::sync::Arc;

/// 同步游标存储
#[async_trait]
pub trait SyncCursorStore: Send + Sync {
    async fn get_raw(&self, key: &str) -> crate::error::Result<Option<String>>;
    async fn save_raw(&self, key: &str, cursor: &str) -> crate::error::Result<()>;
    async fn get_conversation_cursor(
        &self,
        user_id: &str,
        conversation_id: &str,
    ) -> crate::error::Result<Option<SyncCursorVo>>;
    async fn save_conversation_cursor(&self, cursor: &SyncCursorVo) -> crate::error::Result<()>;
}

/// 存储提供者 — 统一持有各 Store；待发/用户资料使用 domain Reader/Writer 分开持有
pub struct StoreProvider {
    pub messages: Arc<dyn MessageStore>,
    pub conversations: Arc<dyn ConversationStore>,
    pub cursors: Arc<dyn SyncCursorStore>,
    pub pending_send_reader: Option<Arc<dyn PendingSendReader>>,
    pub pending_send_writer: Option<Arc<dyn PendingSendWriter>>,
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
            cursors: self.cursors.clone(),
            pending_send_reader: self.pending_send_reader.clone(),
            pending_send_writer: self.pending_send_writer.clone(),
            user_profiles_reader: self.user_profiles_reader.clone(),
            user_profiles_writer: self.user_profiles_writer.clone(),
        }
    }
}
