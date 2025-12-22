//! 会话 Facade
//!
//! 职责：薄薄的一层，只负责调用 Application 层
//! 所有业务逻辑都在领域服务中实现

use std::sync::Arc;
use crate::application::handlers::{CommandHandler, QueryHandler};

/// 会话 Facade
///
/// 提供会话查询、操作等完整 API
pub struct ConversationFacade {
    command_handler: Arc<CommandHandler>,
    query_handler: Arc<QueryHandler>,
}

impl ConversationFacade {
    pub fn new(
        command_handler: Arc<CommandHandler>,
        query_handler: Arc<QueryHandler>,
    ) -> Self {
        Self {
            command_handler,
            query_handler,
        }
    }
    
    // ============================================================================
    // 会话查询 API
    // ============================================================================
    
    /// 获取所有会话列表
    pub async fn get_all_conversation_list(&self) -> anyhow::Result<Vec<serde_json::Value>> {
        self.query_handler.get_all_conversation_list().await
    }
    
    /// 分页获取会话列表
    pub async fn get_conversation_list_split(
        &self,
        page: usize,
        page_size: usize,
    ) -> anyhow::Result<(Vec<serde_json::Value>, usize)> {
        self.query_handler.get_conversation_list_split(page, page_size).await
    }
    
    /// 获取一个会话
    pub async fn get_one_conversation(
        &self,
        conversation_id: String,
    ) -> anyhow::Result<serde_json::Value> {
        self.query_handler.get_one_conversation(conversation_id).await
    }
    
    /// 根据会话 ID 获取多个会话
    pub async fn get_multiple_conversation(
        &self,
        conversation_ids: Vec<String>,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        self.query_handler.get_multiple_conversation(conversation_ids).await
    }
    
    /// 根据会话类型获取会话 ID
    pub async fn get_conversation_id_by_session_type(
        &self,
        conversation_type: String,
        user_id: Option<String>,
    ) -> anyhow::Result<Vec<String>> {
        self.query_handler.get_conversation_id_by_session_type(conversation_type, user_id).await
    }
    
    /// 获取消息总未读数
    pub async fn get_total_unread_msg_count(&self) -> anyhow::Result<u32> {
        self.query_handler.get_total_unread_msg_count().await
    }
    
    /// 获取输入状态
    pub async fn get_input_states(
        &self,
        conversation_id: String,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        self.query_handler.get_input_states(conversation_id).await
    }
    
    // ============================================================================
    // 会话命令 API
    // ============================================================================
    
    /// 清空会话未读数
    pub async fn mark_conversation_message_as_read(
        &self,
        conversation_id: String,
        user_id: String,
    ) -> anyhow::Result<()> {
        use crate::application::commands::MarkConversationReadCommand;
        self.command_handler.mark_conversation_read(MarkConversationReadCommand {
            conversation_id,
            user_id,
        }).await
    }
    
    /// 设置会话草稿
    pub async fn set_conversation_draft(
        &self,
        conversation_id: String,
        draft: Option<String>,
    ) -> anyhow::Result<()> {
        use crate::application::commands::{SetConversationDraftCommand, ClearConversationDraftCommand};
        if let Some(draft_text) = draft {
            self.command_handler.set_conversation_draft(SetConversationDraftCommand {
                conversation_id,
                user_id: "current_user".to_string(), // TODO: 从上下文获取
                draft: draft_text,
            }).await
        } else {
            self.command_handler.clear_conversation_draft(ClearConversationDraftCommand {
                conversation_id,
                user_id: "current_user".to_string(), // TODO: 从上下文获取
            }).await
        }
    }
    
    /// 隐藏会话
    pub async fn hide_conversation(&self, conversation_id: String) -> anyhow::Result<()> {
        // TODO: 实现 HideConversationCommand
        Err(anyhow::anyhow!("hide_conversation not implemented with new CommandHandler"))
    }
    
    /// 隐藏所有会话
    pub async fn hide_all_conversation(&self) -> anyhow::Result<()> {
        // TODO: 实现 HideAllConversationCommand
        Err(anyhow::anyhow!("hide_all_conversation not implemented with new CommandHandler"))
    }
    
    /// 删除会话及会话中消息
    pub async fn delete_conversation_and_delete_all_msg(
        &self,
        conversation_id: String,
    ) -> anyhow::Result<()> {
        use crate::application::commands::DeleteConversationCommand;
        self.command_handler.delete_conversation(DeleteConversationCommand {
            conversation_id,
            user_id: "current_user".to_string(), // TODO: 从上下文获取
        }).await
    }
    
    /// 删除会话中的消息（清空消息）
    pub async fn clear_conversation_and_delete_all_msg(
        &self,
        conversation_id: String,
    ) -> anyhow::Result<()> {
        // TODO: 实现 ClearConversationMessagesCommand
        Err(anyhow::anyhow!("clear_conversation_and_delete_all_msg not implemented with new CommandHandler"))
    }
    
    /// 设置会话信息
    pub async fn set_conversation(
        &self,
        conversation_id: String,
        display_name: Option<String>,
        avatar_url: Option<String>,
        description: Option<String>,
        announcement: Option<String>,
    ) -> anyhow::Result<()> {
        // TODO: 实现 SetConversationInfoCommand
        Err(anyhow::anyhow!("set_conversation not implemented with new CommandHandler"))
    }
    
    /// 改变输入状态
    pub async fn change_input_states(
        &self,
        conversation_id: String,
        user_id: String,
        state_type: crate::domain::conversation::InputStateType,
    ) -> anyhow::Result<()> {
        use crate::application::commands::SetInputStateCommand;
        self.command_handler.set_input_state(SetInputStateCommand {
            conversation_id,
            user_id,
            state_type,
        }).await
    }
}
