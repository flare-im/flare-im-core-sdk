//! 主查询处理器（编排层）
//!
//! 职责：分发查询到具体的处理器，只负责编排，不包含业务逻辑

use std::sync::Arc;
use crate::domain::repository::ReadStore;
use crate::application::fsm::FsmManager;

use super::{
    MessageQueryHandler,
    ConversationQueryHandler,
    SessionQueryHandler,
};
use crate::application::queries::*;

/// 主查询处理器
pub struct QueryHandler {
    message_handler: Arc<MessageQueryHandler>,
    conversation_handler: Arc<ConversationQueryHandler>,
    session_handler: Arc<SessionQueryHandler>,
}

impl QueryHandler {
    pub fn new(read_store: Arc<dyn ReadStore>, fsm: Arc<FsmManager>) -> Self {
        let message_handler = Arc::new(MessageQueryHandler::new(read_store.clone()));
        let conversation_handler = Arc::new(ConversationQueryHandler::new(read_store.clone()));
        let session_handler = Arc::new(SessionQueryHandler::new(fsm));
        
        Self {
            message_handler,
            conversation_handler,
            session_handler,
        }
    }
    
    // ============================================================================
    // 消息查询（委托给 MessageQueryHandler）
    // ============================================================================
    
    pub async fn list_messages(&self, query: ListMessagesQuery) -> anyhow::Result<Vec<serde_json::Value>> {
        self.message_handler.handle_list(query).await
    }
    
    pub async fn get_message(&self, query: GetMessageQuery) -> anyhow::Result<serde_json::Value> {
        self.message_handler.handle_get(query).await
    }
    
    pub async fn search_messages(&self, query: SearchMessagesQuery) -> anyhow::Result<Vec<serde_json::Value>> {
        self.message_handler.handle_search(query).await
    }
    
    // ============================================================================
    // 会话查询（委托给 ConversationQueryHandler）
    // ============================================================================
    
    pub async fn list_conversations(&self, query: ListConversationsQuery) -> anyhow::Result<Vec<serde_json::Value>> {
        self.conversation_handler.handle_list(query).await
    }
    
    pub async fn get_conversation(&self, query: GetConversationQuery) -> anyhow::Result<serde_json::Value> {
        self.conversation_handler.handle_get(query).await
    }
    
    pub async fn get_conversation_unread_count(&self, query: GetConversationUnreadCountQuery) -> anyhow::Result<u32> {
        self.conversation_handler.handle_unread_count(query).await
    }
    
    pub async fn get_total_unread_count(&self, query: GetTotalUnreadCountQuery) -> anyhow::Result<u32> {
        self.conversation_handler.handle_total_unread_count(query).await
    }
    
    // ============================================================================
    // 向后兼容的便捷方法（直接传递参数，内部转换为 Query）
    // ============================================================================
    
    /// 获取所有会话列表（便捷方法）
    pub async fn get_all_conversation_list(&self) -> anyhow::Result<Vec<serde_json::Value>> {
        self.list_conversations(ListConversationsQuery {
            limit: None,
            cursor: None,
        }).await
    }
    
    /// 分页获取会话列表（便捷方法）
    pub async fn get_conversation_list_split(
        &self,
        page: usize,
        page_size: usize,
    ) -> anyhow::Result<(Vec<serde_json::Value>, usize)> {
        self.conversation_handler.get_conversation_list_split(page, page_size).await
    }
    
    /// 获取一个会话（便捷方法）
    pub async fn get_one_conversation(
        &self,
        conversation_id: String,
    ) -> anyhow::Result<serde_json::Value> {
        self.get_conversation(GetConversationQuery { conversation_id }).await
    }
    
    /// 根据会话 ID 获取多个会话（便捷方法）
    pub async fn get_multiple_conversation(
        &self,
        conversation_ids: Vec<String>,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        self.conversation_handler.get_multiple_conversation(conversation_ids).await
    }
    
    /// 根据会话类型获取会话 ID（便捷方法）
    pub async fn get_conversation_id_by_session_type(
        &self,
        conversation_type: String,
        user_id: Option<String>,
    ) -> anyhow::Result<Vec<String>> {
        self.conversation_handler.get_conversation_id_by_session_type(conversation_type, user_id).await
    }
    
    /// 获取消息总未读数（便捷方法）
    pub async fn get_total_unread_msg_count(&self) -> anyhow::Result<u32> {
        self.get_total_unread_count(GetTotalUnreadCountQuery).await
    }
    
    /// 获取输入状态（便捷方法）
    pub async fn get_input_states(
        &self,
        conversation_id: String,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        self.conversation_handler.get_input_states(conversation_id).await
    }
    
    // ============================================================================
    // 会话查询（委托给 SessionQueryHandler）
    // ============================================================================
    
    pub async fn get_session_state(&self, query: GetSessionStateQuery) -> anyhow::Result<crate::domain::session::SessionState> {
        self.session_handler.handle_session_state(query).await
    }
    
    pub async fn get_connection_state(&self, query: GetConnectionStateQuery) -> anyhow::Result<crate::domain::connection::ConnectionState> {
        self.session_handler.handle_connection_state(query).await
    }
    
    pub async fn get_sync_state(&self, query: GetSyncStateQuery) -> anyhow::Result<crate::domain::sync::SyncState> {
        self.session_handler.handle_sync_state(query).await
    }
}
