use std::sync::Arc;
use async_trait::async_trait;
use crate::error::Result;

use super::message_store::MessageStore;
use super::conversation_store::ConversationStore;

/// 同步游标存储 trait — 记录每个会话已同步到的 seq
#[async_trait]
pub trait SyncCursorStore: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<String>>;
    async fn save(&self, key: &str, cursor: &str) -> Result<()>;
}

/// 存储提供者 — 聚合所有存储实例，由用户注入
pub struct StoreProvider {
    pub messages: Arc<dyn MessageStore>,
    pub conversations: Arc<dyn ConversationStore>,
    pub cursors: Arc<dyn SyncCursorStore>,
}
