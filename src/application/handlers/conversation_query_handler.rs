//! 会话查询处理器
//!
//! 职责：处理会话相关的读操作

use std::sync::Arc;
use crate::domain::repository::ReadStore;
use crate::application::queries::*;

/// 会话查询处理器
pub struct ConversationQueryHandler {
    pub(crate) read_store: Arc<dyn ReadStore>,
}

impl ConversationQueryHandler {
    pub fn new(read_store: Arc<dyn ReadStore>) -> Self {
        Self { read_store }
    }
    
    /// 处理查询会话列表
    pub async fn handle_list(&self, query: ListConversationsQuery) -> anyhow::Result<Vec<serde_json::Value>> {
        use crate::domain::repository::{Query, QueryResult};
        let q = Query::ConversationList {
            limit: query.limit,
            cursor: query.cursor,
        };
        
        match self.read_store.query(q).await? {
            QueryResult::ConversationList { items, .. } => Ok(items),
            _ => Err(anyhow::anyhow!("Unexpected query result type")),
        }
    }
    
    /// 处理查询会话详情
    pub async fn handle_get(&self, query: GetConversationQuery) -> anyhow::Result<serde_json::Value> {
        use crate::domain::repository::{Query, QueryResult};
        let q = Query::ConversationDetail {
            conversation_id: query.conversation_id,
        };
        
        match self.read_store.query(q).await? {
            QueryResult::ConversationDetail { item } => Ok(item),
            _ => Err(anyhow::anyhow!("Unexpected query result type")),
        }
    }
    
    /// 处理查询会话未读数
    pub async fn handle_unread_count(&self, query: GetConversationUnreadCountQuery) -> anyhow::Result<u32> {
        let conversation = self.handle_get(GetConversationQuery {
            conversation_id: query.conversation_id,
        }).await?;
        
        Ok(conversation
            .get("unread_count")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(0))
    }
    
    /// 处理查询所有会话的未读总数
    pub async fn handle_total_unread_count(&self, _query: GetTotalUnreadCountQuery) -> anyhow::Result<u32> {
        let conversations = self.handle_list(ListConversationsQuery {
            limit: None,
            cursor: None,
        }).await?;
        
        let total: u32 = conversations
            .iter()
            .filter_map(|conv| {
                conv.get("unread_count")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
            })
            .sum();
        
        Ok(total)
    }
    
    // ============================================================================
    // 便捷方法（从旧的 ConversationQueryHandler 迁移）
    // ============================================================================
    
    /// 获取所有会话列表
    pub async fn get_all_conversation_list(&self) -> anyhow::Result<Vec<serde_json::Value>> {
        self.handle_list(ListConversationsQuery {
            limit: None,
            cursor: None,
        }).await
    }
    
    /// 分页获取会话列表
    pub async fn get_conversation_list_split(
        &self,
        page: usize,
        page_size: usize,
    ) -> anyhow::Result<(Vec<serde_json::Value>, usize)> {
        use crate::domain::repository::{Query, QueryResult};
        
        let offset = page * page_size;
        let q = Query::ConversationList {
            limit: Some(page_size),
            cursor: Some(offset.to_string()),
        };
        let result = self.read_store.query(q).await?;
        
        match result {
            QueryResult::ConversationList { items, next_cursor } => {
                // 计算总页数（需要知道总数，这里简化处理）
                let total = items.len();
                let total_pages = if total < page_size {
                    page + 1
                } else {
                    // 如果还有下一页，总页数至少是当前页+1
                    page + if next_cursor.is_some() { 2 } else { 1 }
                };
                Ok((items, total_pages))
            }
            _ => Err(anyhow::anyhow!("Unexpected query result type")),
        }
    }
    
    /// 获取一个会话
    pub async fn get_one_conversation(
        &self,
        conversation_id: String,
    ) -> anyhow::Result<serde_json::Value> {
        self.handle_get(GetConversationQuery { conversation_id }).await
    }
    
    /// 根据会话 ID 获取多个会话
    pub async fn get_multiple_conversation(
        &self,
        conversation_ids: Vec<String>,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        use crate::domain::repository::{Query, QueryResult};
        
        let mut conversations = Vec::new();
        
        for conversation_id in conversation_ids {
            let q = Query::ConversationDetail { conversation_id };
            let result = self.read_store.query(q).await?;
            
            if let QueryResult::ConversationDetail { item } = result {
                conversations.push(item);
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
            if let Some(conv_type) = conv.get("conversation_type").and_then(|v| v.as_str()) {
                if conv_type == conversation_type {
                    if let Some(conv_id) = conv.get("conversation_id").and_then(|v| v.as_str()) {
                        // 如果指定了 user_id，需要进一步过滤（单聊场景）
                        if let Some(ref uid) = user_id {
                            // 单聊会话 ID 格式通常是 "single-{user1_id}-{user2_id}" 或类似格式
                            // 这里简化处理，实际应该根据会话 ID 格式解析
                            if conv_id.contains(uid) {
                                conversation_ids.push(conv_id.to_string());
                            }
                        } else {
                            conversation_ids.push(conv_id.to_string());
                        }
                    }
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
        let conversation = self.get_one_conversation(conversation_id).await?;
        Ok(conversation.get("input_state").cloned())
    }
}
