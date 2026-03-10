use async_trait::async_trait;
use crate::error::Result;
use crate::model::conversation::ConversationSummary;

/// 会话存储 trait
#[async_trait]
pub trait ConversationStore: Send + Sync {
    async fn save_batch(&self, conversations: &[ConversationSummary]) -> Result<()>;
    async fn get(&self, conversation_id: &str) -> Result<Option<ConversationSummary>>;
    async fn list(&self) -> Result<Vec<ConversationSummary>>;
    async fn update_unread(
        &self,
        conversation_id: &str,
        unread_count: u32,
        last_read_seq: u64,
    ) -> Result<()>;
    async fn delete(&self, conversation_id: &str) -> Result<()>;
}
