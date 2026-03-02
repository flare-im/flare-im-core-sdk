//! 会话命令处理器
//!
//! 职责：编排会话相关的写操作，调用领域服务处理业务逻辑

use std::sync::Arc;
use crate::application::fsm::FsmManager;
use crate::domain::repository::{EventStore, ConversationRepository};
use crate::domain::conversation::Conversation;
use crate::domain::service::ConversationDomainService;
use crate::application::commands::*;

/// 会话命令处理器
pub struct ConversationCommandHandler {
    fsm: Arc<FsmManager>,
    event_store: Arc<dyn EventStore>,
    conversation_repository: Arc<dyn ConversationRepository>,
    domain_service: ConversationDomainService,
}

impl ConversationCommandHandler {
    pub fn new(
        fsm: Arc<FsmManager>,
        event_store: Arc<dyn EventStore>,
        conversation_repository: Arc<dyn ConversationRepository>,
    ) -> Self {
        Self {
            fsm,
            event_store,
            conversation_repository,
            domain_service: ConversationDomainService::new(),
        }
    }
    
    /// 处理标记会话已读命令
    pub async fn handle_mark_read(&self, cmd: MarkConversationReadCommand) -> anyhow::Result<()> {
        let user_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        let mut conversation = self.load_conversation(&cmd.conversation_id).await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", cmd.conversation_id))?;
        
        // 使用领域服务处理业务逻辑
        self.domain_service.mark_as_read(&mut conversation, &user_id)?;
        
        self.save_conversation(&conversation).await?;
        
        // 发布领域事件
        use crate::domain::event::{DomainEvent, conversation_events};
        let event = DomainEvent::new(
            conversation_events::MARKED_AS_READ,
            &cmd.conversation_id,
            conversation.version,
            serde_json::json!({
                "conversation_id": cmd.conversation_id,
                "user_id": user_id,
                "unread_count": conversation.unread_count,
            }),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// 处理标记所有会话已读命令
    pub async fn handle_mark_all_read(&self, _cmd: MarkAllConversationsReadCommand) -> anyhow::Result<usize> {
        let user_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 通过 ConversationRepository 查询所有会话
        let result = self.conversation_repository.find_all(None, None).await?;
        
        // 批量标记所有会话已读
        let mut marked_count = 0;
        for mut conversation in result.conversations {
            // 检查会话是否有未读消息
            if conversation.unread_count > 0 {
                // 使用领域服务处理业务逻辑
                self.domain_service.mark_as_read(&mut conversation, &user_id)?;
                
                // 保存会话
                self.save_conversation(&conversation).await?;
                
                // 发布领域事件
                self.publish_conversation_event(
                    conversation_events::MARKED_AS_READ,
                    &conversation.conversation_id,
                    conversation.version,
                ).await?;
                
                marked_count += 1;
            }
        }
        
        // 发布所有会话已读事件
        use crate::domain::event::DomainEvent;
        let event = DomainEvent::new(
            conversation_events::MARKED_AS_READ,
            "all",
            0,
            serde_json::json!({
                "user_id": user_id,
                "marked_count": marked_count,
            }),
        );
        self.event_store.append(event).await?;
        
        Ok(marked_count)
    }
    
    /// 处理设置会话草稿命令
    pub async fn handle_set_draft(&self, cmd: SetConversationDraftCommand) -> anyhow::Result<()> {
        let mut conversation = self.load_conversation(&cmd.conversation_id).await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", cmd.conversation_id))?;
        
        // 使用领域服务处理业务逻辑
        self.domain_service.set_draft(&mut conversation, Some(cmd.draft))?;
        
        self.save_conversation(&conversation).await?;
        self.publish_conversation_event(conversation_events::DRAFT_UPDATED, &cmd.conversation_id, conversation.version).await?;
        
        Ok(())
    }
    
    /// 处理清除会话草稿命令
    pub async fn handle_clear_draft(&self, cmd: ClearConversationDraftCommand) -> anyhow::Result<()> {
        let mut conversation = self.load_conversation(&cmd.conversation_id).await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", cmd.conversation_id))?;
        
        // 使用领域服务处理业务逻辑
        self.domain_service.set_draft(&mut conversation, None)?;
        
        self.save_conversation(&conversation).await?;
        self.publish_conversation_event(conversation_events::DRAFT_UPDATED, &cmd.conversation_id, conversation.version).await?;
        
        Ok(())
    }
    
    /// 处理置顶会话命令
    pub async fn handle_pin(&self, cmd: PinConversationCommand) -> anyhow::Result<()> {
        let mut conversation = self.load_conversation(&cmd.conversation_id).await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", cmd.conversation_id))?;
        
        // 使用领域服务处理业务逻辑
        self.domain_service.set_pinned(&mut conversation, true)?;
        
        // TODO: 处理 expire_at（暂时忽略）
        
        self.save_conversation(&conversation).await?;
        self.publish_conversation_event(conversation_events::PINNED, &cmd.conversation_id, conversation.version).await?;
        
        Ok(())
    }
    
    /// 处理取消置顶会话命令
    pub async fn handle_unpin(&self, cmd: UnpinConversationCommand) -> anyhow::Result<()> {
        let mut conversation = self.load_conversation(&cmd.conversation_id).await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", cmd.conversation_id))?;
        
        // 使用领域服务处理业务逻辑
        self.domain_service.set_pinned(&mut conversation, false)?;
        
        self.save_conversation(&conversation).await?;
        self.publish_conversation_event(conversation_events::UNPINNED, &cmd.conversation_id, conversation.version).await?;
        
        Ok(())
    }
    
    /// 处理免打扰会话命令（简化版：只负责编排）
    pub async fn handle_mute(&self, cmd: MuteConversationCommand) -> anyhow::Result<()> {
        // 1. 加载会话（基础设施层职责）
        let mut conversation = self.load_conversation(&cmd.conversation_id).await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", cmd.conversation_id))?;
        
        // 2. 调用领域服务执行操作（业务逻辑在领域层）
        self.domain_service.set_muted(&mut conversation, true, cmd.mute_until)?;
        
        // 3. 保存会话（基础设施层职责）
        self.save_conversation(&conversation).await?;
        
        // 4. 发布领域事件（应用层职责）
        self.publish_conversation_event(conversation_events::MUTED, &cmd.conversation_id, conversation.version).await?;
        
        Ok(())
    }
    
    /// 处理取消免打扰会话命令（简化版：只负责编排）
    pub async fn handle_unmute(&self, cmd: UnmuteConversationCommand) -> anyhow::Result<()> {
        // 1. 加载会话（基础设施层职责）
        let mut conversation = self.load_conversation(&cmd.conversation_id).await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", cmd.conversation_id))?;
        
        // 2. 调用领域服务执行操作（业务逻辑在领域层）
        self.domain_service.set_muted(&mut conversation, false, None)?;
        
        // 3. 保存会话（基础设施层职责）
        self.save_conversation(&conversation).await?;
        
        // 4. 发布领域事件（应用层职责）
        self.publish_conversation_event(conversation_events::UNMUTED, &cmd.conversation_id, conversation.version).await?;
        
        Ok(())
    }
    
    /// 处理设置输入状态命令
    pub async fn handle_set_input_state(&self, cmd: SetInputStateCommand) -> anyhow::Result<()> {
        let user_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        let mut conversation = self.load_conversation(&cmd.conversation_id).await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", cmd.conversation_id))?;
        
        // 使用领域服务处理业务逻辑
        self.domain_service.set_input_state(&mut conversation, user_id, cmd.state_type)?;
        
        self.save_conversation(&conversation).await?;
        self.publish_conversation_event(conversation_events::UPDATED, &cmd.conversation_id, conversation.version).await?;
        
        Ok(())
    }
    
    /// 处理清除输入状态命令
    pub async fn handle_clear_input_state(&self, cmd: ClearInputStateCommand) -> anyhow::Result<()> {
        let mut conversation = self.load_conversation(&cmd.conversation_id).await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", cmd.conversation_id))?;
        
        // 使用领域服务处理业务逻辑
        self.domain_service.clear_input_state(&mut conversation)?;
        
        self.save_conversation(&conversation).await?;
        self.publish_conversation_event(conversation_events::UPDATED, &cmd.conversation_id, conversation.version).await?;
        
        Ok(())
    }
    
    /// 处理删除会话命令
    pub async fn handle_delete(&self, cmd: DeleteConversationCommand) -> anyhow::Result<()> {
        let mut conversation = self.load_conversation(&cmd.conversation_id).await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", cmd.conversation_id))?;
        
        // 使用领域服务处理业务逻辑
        self.domain_service.delete(&mut conversation)?;
        
        self.save_conversation(&conversation).await?;
        self.publish_conversation_event(conversation_events::DELETED, &cmd.conversation_id, conversation.version).await?;
        
        Ok(())
    }
    
    /// 处理隐藏会话命令
    pub async fn handle_hide(&self, cmd: HideConversationCommand) -> anyhow::Result<()> {
        let mut conversation = self.load_conversation(&cmd.conversation_id).await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", cmd.conversation_id))?;
        
        // 使用领域服务处理业务逻辑
        self.domain_service.hide(&mut conversation)?;
        
        self.save_conversation(&conversation).await?;
        self.publish_conversation_event(conversation_events::HIDDEN, &cmd.conversation_id, conversation.version).await?;
        
        Ok(())
    }
    
    /// 处理隐藏所有会话命令
    pub async fn handle_hide_all(&self, _cmd: HideAllConversationCommand) -> anyhow::Result<()> {
        let user_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 通过 ConversationRepository 查询所有会话
        let result = self.conversation_repository.find_all(None, None).await?;
        
        // 批量隐藏所有会话
        let mut hidden_count = 0;
        for mut conversation in result.conversations {
            // 使用领域服务处理业务逻辑
            self.domain_service.hide(&mut conversation)?;
            
            // 保存会话
            self.save_conversation(&conversation).await?;
            
            // 发布领域事件
            self.publish_conversation_event(
                conversation_events::HIDDEN,
                &conversation.conversation_id,
                conversation.version,
            ).await?;
            
            hidden_count += 1;
        }
        
        // 发布所有会话隐藏事件
        use crate::domain::event::DomainEvent;
        let event = DomainEvent::new(
            conversation_events::ALL_HIDDEN,
            "all",
            0,
            serde_json::json!({
                "user_id": user_id,
                "hidden_count": hidden_count,
            }),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// 处理清空会话消息命令
    pub async fn handle_clear_messages(&self, cmd: ClearConversationMessagesCommand) -> anyhow::Result<()> {
        let mut conversation = self.load_conversation(&cmd.conversation_id).await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", cmd.conversation_id))?;
        
        // 使用领域服务处理业务逻辑
        self.domain_service.clear_messages(&mut conversation)?;
        
        self.save_conversation(&conversation).await?;
        self.publish_conversation_event(conversation_events::MESSAGES_CLEARED, &cmd.conversation_id, conversation.version).await?;
        
        Ok(())
    }
    
    /// 处理设置会话信息命令
    pub async fn handle_set_info(&self, cmd: SetConversationInfoCommand) -> anyhow::Result<()> {
        let user_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        let mut conversation = self.load_conversation(&cmd.conversation_id).await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", cmd.conversation_id))?;
        
        // 使用领域服务处理业务逻辑
        self.domain_service.update_info(
            &mut conversation,
            cmd.display_name,
            cmd.avatar_url,
            cmd.description,
            cmd.announcement,
            Some(user_id),
        )?;
        
        self.save_conversation(&conversation).await?;
        self.publish_conversation_event(conversation_events::UPDATED, &cmd.conversation_id, conversation.version).await?;
        
        Ok(())
    }
    
    // ============================================================================
    // 辅助方法
    // ============================================================================
    
    async fn load_conversation(&self, conversation_id: &str) -> anyhow::Result<Option<Conversation>> {
        self.conversation_repository.find_by_id(conversation_id).await
    }
    
    async fn save_conversation(&self, conversation: &Conversation) -> anyhow::Result<()> {
        // 尝试查找现有会话，如果存在则更新，否则保存
        if self.conversation_repository.find_by_id(&conversation.conversation_id).await?.is_some() {
            self.conversation_repository.update(conversation).await
        } else {
            self.conversation_repository.save(conversation).await
        }
    }
    
    async fn publish_conversation_event(
        &self,
        event_type: &'static str,
        conversation_id: &str,
        version: u64,
    ) -> anyhow::Result<()> {
        use crate::domain::event::DomainEvent;
        let event = DomainEvent::new(
            event_type,
            conversation_id,
            version,
            serde_json::json!({
                "conversation_id": conversation_id,
            }),
        );
        self.event_store.append(event).await?;
        Ok(())
    }
}

// 导入 conversation_events
use crate::domain::event::conversation_events;
