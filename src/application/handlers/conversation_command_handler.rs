//! 会话命令处理器
//!
//! 职责：编排会话相关的写操作，调用领域服务处理业务逻辑

use std::sync::Arc;
use crate::application::fsm::FsmManager;
use crate::domain::repository::{EventStore, ReadStore};
use crate::domain::conversation::Conversation;
use crate::domain::service::ConversationDomainService;
use crate::application::commands::*;

/// 会话命令处理器
pub struct ConversationCommandHandler {
    fsm: Arc<FsmManager>,
    event_store: Arc<dyn EventStore>,
    read_store: Arc<dyn ReadStore>,
    domain_service: ConversationDomainService,
}

impl ConversationCommandHandler {
    pub fn new(
        fsm: Arc<FsmManager>,
        event_store: Arc<dyn EventStore>,
        read_store: Arc<dyn ReadStore>,
    ) -> Self {
        Self {
            fsm,
            event_store,
            read_store,
            domain_service: ConversationDomainService::new(),
        }
    }
    
    /// 处理标记会话已读命令
    pub async fn handle_mark_read(&self, cmd: MarkConversationReadCommand) -> anyhow::Result<()> {
        let mut conversation = self.load_conversation(&cmd.conversation_id).await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", cmd.conversation_id))?;
        
        // 使用领域服务处理业务逻辑
        self.domain_service.mark_as_read(&mut conversation, &cmd.user_id)?;
        
        self.save_conversation(&conversation).await?;
        
        // 发布领域事件
        use crate::domain::event::{DomainEvent, conversation_events};
        let event = DomainEvent::new(
            conversation_events::MARKED_AS_READ,
            &cmd.conversation_id,
            conversation.version,
            serde_json::json!({
                "conversation_id": cmd.conversation_id,
                "user_id": cmd.user_id,
                "unread_count": conversation.unread_count,
            }),
        );
        self.event_store.append(event).await?;
        
        Ok(())
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
    
    /// 处理免打扰会话命令
    pub async fn handle_mute(&self, cmd: MuteConversationCommand) -> anyhow::Result<()> {
        let mut conversation = self.load_conversation(&cmd.conversation_id).await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", cmd.conversation_id))?;
        
        // 使用领域服务处理业务逻辑
        self.domain_service.set_muted(&mut conversation, true)?;
        
        // 设置 mute_until（如果有）
        if let Some(mute_until) = cmd.mute_until {
            conversation.mute_until = Some(mute_until);
            conversation.version += 1;
            conversation.updated_at = chrono::Utc::now();
        }
        
        self.save_conversation(&conversation).await?;
        self.publish_conversation_event(conversation_events::MUTED, &cmd.conversation_id, conversation.version).await?;
        
        Ok(())
    }
    
    /// 处理取消免打扰会话命令
    pub async fn handle_unmute(&self, cmd: UnmuteConversationCommand) -> anyhow::Result<()> {
        let mut conversation = self.load_conversation(&cmd.conversation_id).await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", cmd.conversation_id))?;
        
        // 使用领域服务处理业务逻辑
        self.domain_service.set_muted(&mut conversation, false)?;
        
        // 清除 mute_until
        conversation.mute_until = None;
        conversation.version += 1;
        conversation.updated_at = chrono::Utc::now();
        
        self.save_conversation(&conversation).await?;
        self.publish_conversation_event(conversation_events::UNMUTED, &cmd.conversation_id, conversation.version).await?;
        
        Ok(())
    }
    
    /// 处理设置输入状态命令
    pub async fn handle_set_input_state(&self, cmd: SetInputStateCommand) -> anyhow::Result<()> {
        let mut conversation = self.load_conversation(&cmd.conversation_id).await?
            .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", cmd.conversation_id))?;
        
        // 使用领域服务处理业务逻辑
        self.domain_service.set_input_state(&mut conversation, cmd.user_id.clone(), cmd.state_type)?;
        
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
    
    // ============================================================================
    // 辅助方法
    // ============================================================================
    
    async fn load_conversation(&self, conversation_id: &str) -> anyhow::Result<Option<Conversation>> {
        use crate::domain::repository::{Query, QueryResult};
        let query = Query::ConversationDetail {
            conversation_id: conversation_id.to_string(),
        };
        
        match self.read_store.query(query).await? {
            QueryResult::ConversationDetail { item } => {
                if item.is_null() || item.get("conversation_id").is_none() {
                    Ok(None)
                } else {
                    Ok(serde_json::from_value::<Conversation>(item).ok())
                }
            }
            _ => Ok(None),
        }
    }
    
    async fn save_conversation(&self, conversation: &Conversation) -> anyhow::Result<()> {
        self.read_store.write_conversation(conversation).await
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
