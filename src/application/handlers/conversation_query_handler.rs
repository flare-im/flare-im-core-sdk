//! 会话查询处理器
//!
//! 职责：处理会话相关的读操作，返回领域模型

use std::sync::Arc;
use crate::domain::repository::ConversationRepository;
use crate::domain::conversation::Conversation;
use crate::application::queries::*;


/// 会话查询处理器
pub struct ConversationQueryHandler {
    pub(crate) conversation_repository: Arc<dyn ConversationRepository>,
}

impl ConversationQueryHandler {
    pub fn new(conversation_repository: Arc<dyn ConversationRepository>) -> Self {
        Self { conversation_repository }
    }
    
    /// 处理查询会话列表，返回领域模型
    pub async fn handle_list(&self, query: ListConversationsQuery) -> anyhow::Result<Vec<Conversation>> {
        let result = self.conversation_repository
            .find_all(query.limit, query.cursor)
            .await?;
        
        Ok(result.conversations)
    }
    
    /// 处理查询会话详情，返回领域模型
    pub async fn handle_get(&self, query: GetConversationQuery) -> anyhow::Result<Conversation> {
        self.conversation_repository
            .find_by_id(&query.conversation_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", query.conversation_id))
    }
    
    /// 处理查询会话未读数
    pub async fn handle_unread_count(&self, query: GetConversationUnreadCountQuery) -> anyhow::Result<u32> {
        let conversation = self.handle_get(GetConversationQuery {
            conversation_id: query.conversation_id,
        }).await?;
        
        Ok(conversation.unread_count)
    }
    
    /// 处理查询所有会话的未读总数
    pub async fn handle_total_unread_count(&self, _query: GetTotalUnreadCountQuery) -> anyhow::Result<u32> {
        let conversations = self.handle_list(ListConversationsQuery {
            limit: None,
            cursor: None,
        }).await?;
        
        let total: u32 = conversations
            .iter()
            .map(|conv| conv.unread_count)
            .sum();
        
        Ok(total)
    }
    
    // ============================================================================
    // 便捷方法（返回领域模型）
    // ============================================================================
    
    /// 获取所有会话列表（返回领域模型）
    pub async fn get_all_conversation_list(&self) -> anyhow::Result<Vec<Conversation>> {
        self.handle_list(ListConversationsQuery {
            limit: None,
            cursor: None,
        }).await
    }
    
    /// 分页获取会话列表（返回领域模型）
    pub async fn get_conversation_list_split(
        &self,
        page: usize,
        page_size: usize,
    ) -> anyhow::Result<(Vec<Conversation>, usize)> {
        let offset = page * page_size;
        let result = self.conversation_repository
            .find_all(
                Some(page_size),
                Some(offset.to_string()),
            )
            .await?;
        
        let conversations = result.conversations;
        let total = conversations.len();
        let total_pages = if total < page_size {
            page + 1
        } else {
            // 如果还有下一页，总页数至少是当前页+1
            page + if result.next_cursor.is_some() { 2 } else { 1 }
        };
        
        Ok((conversations, total_pages))
    }
    
    /// 获取一个会话（返回领域模型）
    pub async fn get_one_conversation(
        &self,
        conversation_id: String,
    ) -> anyhow::Result<Conversation> {
        self.handle_get(GetConversationQuery { conversation_id }).await
    }
    
    /// 根据会话 ID 获取多个会话（返回领域模型）
    pub async fn get_multiple_conversation(
        &self,
        conversation_ids: Vec<String>,
    ) -> anyhow::Result<Vec<Conversation>> {
        let mut conversations = Vec::new();
        
        for conversation_id in conversation_ids {
            if let Some(conversation) = self.conversation_repository
                .find_by_id(&conversation_id)
                .await?
            {
                conversations.push(conversation);
            }
        }
        
        Ok(conversations)
    }
    
    /// 根据会话类型获取会话 ID
    pub async fn get_conversation_id_by_session_type(
        &self,
        conversation_type: String,
        user_id: Option<String>,
    ) -> anyhow::Result<Vec<String>> {
        // 获取所有会话
        let conversations = self.get_all_conversation_list().await?;
        
        let mut conversation_ids = Vec::new();
        
        for conv in conversations {
            if conv.conversation_type == conversation_type {
                // 如果指定了 user_id，需要进一步过滤（单聊场景）
                if let Some(ref uid) = user_id {
                    // 单聊会话 ID 格式通常是 "single-{user1_id}-{user2_id}" 或类似格式
                    // 这里简化处理，实际应该根据会话 ID 格式解析
                    if conv.conversation_id.contains(uid) {
                        conversation_ids.push(conv.conversation_id.clone());
                    }
                } else {
                    conversation_ids.push(conv.conversation_id.clone());
                }
            }
        }
        
        Ok(conversation_ids)
    }
    
    /// 获取输入状态
    pub async fn get_input_states(
        &self,
        conversation_id: String,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        let _conversation = self.get_one_conversation(conversation_id).await?;
        // TODO: 从 Conversation 领域模型中获取 input_state
        // 目前 Conversation 领域模型可能没有 input_state 字段，需要检查
        Ok(None)
    }
}
