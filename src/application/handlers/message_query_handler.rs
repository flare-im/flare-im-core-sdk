//! 消息查询处理器
//!
//! 职责：处理消息相关的读操作，返回领域模型

use std::sync::Arc;
use crate::domain::repository::MessageRepository;
use crate::domain::message::Message;
use crate::application::queries::*;


/// 消息查询处理器
pub struct MessageQueryHandler {
    message_repository: Arc<dyn MessageRepository>,
}

impl MessageQueryHandler {
    pub fn new(message_repository: Arc<dyn MessageRepository>) -> Self {
        Self { message_repository }
    }
    
    /// 处理查询消息列表，返回领域模型
    pub async fn handle_list(&self, query: ListMessagesQuery) -> anyhow::Result<Vec<Message>> {
        let result = self.message_repository
            .find_by_conversation(
                &query.conversation_id,
                query.limit,
                query.cursor,
            )
            .await?;
        
        Ok(result.messages)
    }
    
    /// 处理查询消息详情，返回领域模型
    pub async fn handle_get(&self, query: GetMessageQuery) -> anyhow::Result<Message> {
        self.message_repository
            .find_by_id(&query.message_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Message not found: {}", query.message_id))
    }
    
    /// 处理搜索消息，返回领域模型
    pub async fn handle_search(&self, query: SearchMessagesQuery) -> anyhow::Result<Vec<Message>> {
        self.message_repository
            .search(
                query.conversation_id.as_deref(),
                &query.keyword,
                query.limit,
            )
            .await
    }
}
