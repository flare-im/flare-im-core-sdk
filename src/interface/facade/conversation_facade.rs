//! Conversation Facade
//!
//! Provides high-level APIs for conversation-related operations including:
//!
//! - Querying conversations
//! - Managing conversation state (read, draft, visibility)
//! - Conversation operations (hide, clear, set info)
//!
//! ## Example
//!
//! ```no_run
//! use flare_im_core_sdk::interface::facade::ConversationFacade;
//!
//! # async fn example(facade: &ConversationFacade) -> anyhow::Result<()> {
//! // Get all conversations
//! let conversations = facade.get_all_conversation_list().await?;
//!
//! // Mark conversation as read
//! facade.mark_conversation_message_as_read(
//!     "conv1".to_string(),
//!     "user1".to_string(),
//! ).await?;
//! # Ok(())
//! # }
//! ```

use std::sync::Arc;
use crate::application::handlers::{CommandHandler, QueryHandler};
use crate::domain::conversation::Conversation;

/// Conversation facade providing high-level conversation APIs
///
/// ## Architecture Design
///
/// Facade 层只关注业务语义，不处理用户身份相关逻辑。
/// 用户 ID 由 Application 层（CommandHandler）统一从 FSM 获取并填充到 Command。
///
/// 架构分层：
/// - **Interface Layer (Facade)**: 提供简洁的业务 API，隐藏复杂性
/// - **Application Layer**: 管理业务上下文（包括用户身份），编排领域服务
/// - **Domain Layer**: 业务逻辑，不关心用户身份如何获取
pub struct ConversationFacade {
    command_handler: Arc<CommandHandler>,
    query_handler: Arc<QueryHandler>,
}

impl ConversationFacade {
    /// Creates a new conversation facade
    ///
    /// # Arguments
    ///
    /// * `command_handler` - Command handler for conversation operations
    /// * `query_handler` - Query handler for conversation queries
    pub fn new(
        command_handler: Arc<CommandHandler>,
        query_handler: Arc<QueryHandler>,
    ) -> Self {
        Self {
            command_handler,
            query_handler,
        }
    }
    
    /// Gets all conversations
    ///
    /// # Returns
    ///
    /// Returns a vector of conversation JSON objects.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ConversationFacade;
    /// # async fn example(facade: &ConversationFacade) -> anyhow::Result<()> {
    /// let conversations = facade.get_all_conversation_list().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_all_conversation_list(&self) -> anyhow::Result<Vec<Conversation>> {
        self.query_handler.get_all_conversation_list().await
    }
    
    /// Gets conversations with pagination
    ///
    /// # Arguments
    ///
    /// * `page` - The page number (1-based)
    /// * `page_size` - The number of items per page
    ///
    /// # Returns
    ///
    /// Returns a tuple of (conversations, total_count).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ConversationFacade;
    /// # async fn example(facade: &ConversationFacade) -> anyhow::Result<()> {
    /// let (conversations, total) = facade.get_conversation_list_split(1, 20).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_conversation_list_split(
        &self,
        page: usize,
        page_size: usize,
    ) -> anyhow::Result<(Vec<Conversation>, usize)> {
        self.query_handler.get_conversation_list_split(page, page_size).await
    }
    
    /// Gets a single conversation by ID
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - The conversation ID
    ///
    /// # Returns
    ///
    /// Returns the conversation JSON object.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ConversationFacade;
    /// # async fn example(facade: &ConversationFacade) -> anyhow::Result<()> {
    /// let conversation = facade.get_one_conversation("conv1".to_string()).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_one_conversation(
        &self,
        conversation_id: String,
    ) -> anyhow::Result<Conversation> {
        self.query_handler.get_one_conversation(conversation_id).await
    }
    
    /// Gets multiple conversations by IDs
    ///
    /// # Arguments
    ///
    /// * `conversation_ids` - Vector of conversation IDs
    ///
    /// # Returns
    ///
    /// Returns a vector of conversation JSON objects.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ConversationFacade;
    /// # async fn example(facade: &ConversationFacade) -> anyhow::Result<()> {
    /// let conversations = facade.get_multiple_conversation(
    ///     vec!["conv1".to_string(), "conv2".to_string()]
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_multiple_conversation(
        &self,
        conversation_ids: Vec<String>,
    ) -> anyhow::Result<Vec<Conversation>> {
        self.query_handler.get_multiple_conversation(conversation_ids).await
    }
    
    /// Gets conversation IDs by session type
    ///
    /// # Arguments
    ///
    /// * `conversation_type` - The conversation type (e.g., "single", "group")
    /// * `user_id` - Optional user ID filter
    ///
    /// # Returns
    ///
    /// Returns a vector of conversation IDs.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ConversationFacade;
    /// # async fn example(facade: &ConversationFacade) -> anyhow::Result<()> {
    /// let conversation_ids = facade.get_conversation_id_by_session_type(
    ///     "single".to_string(),
    ///     Some("user1".to_string()),
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_conversation_id_by_session_type(
        &self,
        conversation_type: String,
        user_id: Option<String>,
    ) -> anyhow::Result<Vec<String>> {
        self.query_handler.get_conversation_id_by_session_type(conversation_type, user_id).await
    }
    
    /// Gets the total unread message count across all conversations
    ///
    /// # Returns
    ///
    /// Returns the total unread count.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ConversationFacade;
    /// # async fn example(facade: &ConversationFacade) -> anyhow::Result<()> {
    /// let unread_count = facade.get_total_unread_msg_count().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_total_unread_msg_count(&self) -> anyhow::Result<u32> {
        self.query_handler.get_total_unread_msg_count().await
    }
    
    /// Gets input states for a conversation
    ///
    /// Input states indicate which users are currently typing in the conversation.
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - The conversation ID
    ///
    /// # Returns
    ///
    /// Returns the input states JSON object if available.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ConversationFacade;
    /// # async fn example(facade: &ConversationFacade) -> anyhow::Result<()> {
    /// if let Some(states) = facade.get_input_states("conv1".to_string()).await? {
    ///     // Process input states...
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_input_states(
        &self,
        conversation_id: String,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        self.query_handler.get_input_states(conversation_id).await
    }
    
    /// Marks all messages in a conversation as read
    ///
    /// User ID is automatically obtained from the current session by Application layer.
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - The conversation ID
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails or the user is not logged in.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ConversationFacade;
    /// # async fn example(facade: &ConversationFacade) -> anyhow::Result<()> {
    /// facade.mark_conversation_message_as_read("conv1".to_string()).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn mark_conversation_message_as_read(
        &self,
        conversation_id: String,
    ) -> anyhow::Result<()> {
        use crate::application::commands::MarkConversationReadCommand;
        self.command_handler.mark_conversation_read(MarkConversationReadCommand {
            conversation_id,
        }).await
    }
    
    /// Marks all messages in all conversations as read
    ///
    /// This method iterates through all conversations and marks all unread messages
    /// in each conversation as read for the current logged in user.
    ///
    /// # Returns
    ///
    /// Returns the number of conversations that were marked as read.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails or the user is not logged in.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ConversationFacade;
    /// # async fn example(facade: &ConversationFacade) -> anyhow::Result<()> {
    /// let marked_count = facade.mark_all_conversations_as_read().await?;
    /// println!("Marked {} conversations as read", marked_count);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn mark_all_conversations_as_read(&self) -> anyhow::Result<usize> {
        use crate::application::commands::MarkAllConversationsReadCommand;
        self.command_handler.mark_all_conversations_read(MarkAllConversationsReadCommand {
        }).await
    }
    
    /// Sets or clears the conversation draft
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - The conversation ID
    /// * `draft` - The draft text, or `None` to clear the draft
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ConversationFacade;
    /// # async fn example(facade: &ConversationFacade) -> anyhow::Result<()> {
    /// // Set draft
    /// facade.set_conversation_draft(
    ///     "conv1".to_string(),
    ///     Some("Draft text".to_string()),
    /// ).await?;
    ///
    /// // Clear draft
    /// facade.set_conversation_draft("conv1".to_string(), None).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_conversation_draft(
        &self,
        conversation_id: String,
        draft: Option<String>,
    ) -> anyhow::Result<()> {
        use crate::application::commands::{SetConversationDraftCommand, ClearConversationDraftCommand};
        if let Some(draft_text) = draft {
            self.command_handler.set_conversation_draft(SetConversationDraftCommand {
                conversation_id,
                draft: draft_text,
            }).await
        } else {
            self.command_handler.clear_conversation_draft(ClearConversationDraftCommand {
                conversation_id,
            }).await
        }
    }
    
    /// Hides a conversation
    ///
    /// Hides the conversation from the conversation list without deleting it.
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - The conversation ID to hide
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ConversationFacade;
    /// # async fn example(facade: &ConversationFacade) -> anyhow::Result<()> {
    /// facade.hide_conversation("conv1".to_string()).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn hide_conversation(&self, conversation_id: String) -> anyhow::Result<()> {
        use crate::application::commands::HideConversationCommand;
        self.command_handler.hide_conversation(HideConversationCommand {
            conversation_id,
        }).await
    }
    
    /// Hides all conversations
    ///
    /// Hides all conversations from the conversation list.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails or the user is not logged in.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ConversationFacade;
    /// # async fn example(facade: &ConversationFacade) -> anyhow::Result<()> {
    /// facade.hide_all_conversation().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn hide_all_conversation(&self) -> anyhow::Result<()> {
        use crate::application::commands::HideAllConversationCommand;
        self.command_handler.hide_all_conversations(HideAllConversationCommand {
        }).await
    }
    
    /// Deletes a conversation and all its messages
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - The conversation ID to delete
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails or the user is not logged in.
    ///
    /// # Warning
    ///
    /// This operation is irreversible. All messages in the conversation will be deleted.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ConversationFacade;
    /// # async fn example(facade: &ConversationFacade) -> anyhow::Result<()> {
    /// facade.delete_conversation_and_delete_all_msg("conv1".to_string()).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn delete_conversation_and_delete_all_msg(
        &self,
        conversation_id: String,
    ) -> anyhow::Result<()> {
        use crate::application::commands::DeleteConversationCommand;
        self.command_handler.delete_conversation(DeleteConversationCommand {
            conversation_id,
        }).await
    }
    
    /// Clears all messages in a conversation
    ///
    /// Deletes all messages in the conversation but keeps the conversation itself.
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - The conversation ID
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails or the user is not logged in.
    ///
    /// # Warning
    ///
    /// This operation is irreversible. All messages will be deleted.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ConversationFacade;
    /// # async fn example(facade: &ConversationFacade) -> anyhow::Result<()> {
    /// facade.clear_conversation_and_delete_all_msg("conv1".to_string()).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn clear_conversation_and_delete_all_msg(
        &self,
        conversation_id: String,
    ) -> anyhow::Result<()> {
        use crate::application::commands::ClearConversationMessagesCommand;
        self.command_handler.clear_conversation_messages(ClearConversationMessagesCommand {
            conversation_id,
        }).await
    }
    
    /// Sets conversation information
    ///
    /// Updates conversation metadata such as display name, avatar, description, etc.
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - The conversation ID
    /// * `display_name` - Optional display name
    /// * `avatar_url` - Optional avatar URL
    /// * `description` - Optional description
    /// * `announcement` - Optional announcement
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails or the user is not logged in.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ConversationFacade;
    /// # async fn example(facade: &ConversationFacade) -> anyhow::Result<()> {
    /// facade.set_conversation(
    ///     "conv1".to_string(),
    ///     Some("Group Name".to_string()),
    ///     Some("https://example.com/avatar.jpg".to_string()),
    ///     None,
    ///     None,
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_conversation(
        &self,
        conversation_id: String,
        display_name: Option<String>,
        avatar_url: Option<String>,
        description: Option<String>,
        announcement: Option<String>,
    ) -> anyhow::Result<()> {
        use crate::application::commands::SetConversationInfoCommand;
        self.command_handler.set_conversation_info(SetConversationInfoCommand {
            conversation_id,
            display_name,
            avatar_url,
            description,
            announcement,
        }).await
    }
    
    /// Changes the input state for a conversation
    ///
    /// Input states indicate typing status (e.g., typing, stopped typing).
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - The conversation ID
    /// * `state_type` - The input state type
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails or the user is not logged in.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ConversationFacade;
    /// # use flare_im_core_sdk::domain::conversation::InputStateType;
    /// # async fn example(facade: &ConversationFacade) -> anyhow::Result<()> {
    /// facade.change_input_states(
    ///     "conv1".to_string(),
    ///     InputStateType::Typing,
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn change_input_states(
        &self,
        conversation_id: String,
        state_type: crate::domain::conversation::InputStateType,
    ) -> anyhow::Result<()> {
        use crate::application::commands::SetInputStateCommand;
        self.command_handler.set_input_state(SetInputStateCommand {
            conversation_id,
            state_type,
        }).await
    }
}
