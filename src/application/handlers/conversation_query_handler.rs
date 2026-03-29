//! 会话查询处理器：执行 GetConversationsQuery / GetConversationQuery。

use std::sync::Arc;

use crate::application::queries::{GetConversationQuery, GetConversationsQuery};
use crate::error::Result;
use crate::model::Conversation;
use crate::store::ConversationStore;

/// 会话读侧处理器，持有 ConversationStore，执行会话相关 Query。
pub struct ConversationQueryHandler {
    pub conversation_store: Arc<dyn ConversationStore>,
}

impl ConversationQueryHandler {
    pub fn new(conversation_store: Arc<dyn ConversationStore>) -> Self {
        Self { conversation_store }
    }

    pub async fn handle_get_conversations(
        &self,
        _query: GetConversationsQuery,
    ) -> Result<Vec<Conversation>> {
        self.conversation_store.list().await
    }

    pub async fn handle_get_conversation(
        &self,
        query: GetConversationQuery,
    ) -> Result<Option<Conversation>> {
        self.conversation_store.get(&query.conversation_id).await
    }
}
