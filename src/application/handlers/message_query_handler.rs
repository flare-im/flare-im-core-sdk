//! 消息查询处理器：执行 GetMessagesQuery / SearchMessagesQuery。

use std::sync::Arc;

use crate::application::queries::{GetMessagesQuery, SearchMessagesQuery};
use crate::error::Result;
use crate::model::IMMessage;
use crate::store::MessageStore;

/// 消息读侧处理器，持有 MessageStore，执行消息相关 Query。
pub struct MessageQueryHandler {
    pub message_store: Arc<dyn MessageStore>,
}

impl MessageQueryHandler {
    pub fn new(message_store: Arc<dyn MessageStore>) -> Self {
        Self { message_store }
    }

    pub async fn handle_get_messages(&self, query: GetMessagesQuery) -> Result<Vec<IMMessage>> {
        self.message_store
            .get_by_conversation(&query.conversation_id, query.before_seq, query.limit)
            .await
    }

    pub async fn handle_search_messages(
        &self,
        query: SearchMessagesQuery,
    ) -> Result<Vec<IMMessage>> {
        self.message_store.search(&query.keyword, query.limit).await
    }
}
