use crate::error::Result;
use crate::model::conversation::ConversationSummary;
use crate::store::ConversationStore;

/// 查询会话列表（本地存储）
pub struct GetConversationsQuery;

impl GetConversationsQuery {
    pub async fn execute(&self, store: &dyn ConversationStore) -> Result<Vec<ConversationSummary>> {
        store.list().await
    }
}

/// 查询单个会话
pub struct GetConversationQuery {
    pub conversation_id: String,
}

impl GetConversationQuery {
    pub async fn execute(&self, store: &dyn ConversationStore) -> Result<Option<ConversationSummary>> {
        store.get(&self.conversation_id).await
    }
}
