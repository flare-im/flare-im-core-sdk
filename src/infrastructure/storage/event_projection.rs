use std::sync::Arc;
use tokio::sync::mpsc;
use crate::domain::event::DomainEvent;
use crate::domain::repository::{MessageRepository, ConversationRepository};
use tracing::{error, info, warn};

pub struct EventProjector {
    message_repository: Arc<dyn MessageRepository>,
    conversation_repository: Arc<dyn ConversationRepository>,
    event_rx: mpsc::UnboundedReceiver<DomainEvent>,
}

impl EventProjector {
    pub fn new(
        message_repository: Arc<dyn MessageRepository>,
        conversation_repository: Arc<dyn ConversationRepository>,
        event_rx: mpsc::UnboundedReceiver<DomainEvent>,
    ) -> Self {
        Self {
            message_repository,
            conversation_repository,
            event_rx,
        }
    }
    
    pub fn start(self) -> tokio::task::JoinHandle<()> {
        let message_repository = self.message_repository.clone();
        let conversation_repository = self.conversation_repository.clone();
        tokio::spawn(async move {
            let mut event_rx = self.event_rx;
            
            while let Some(event) = event_rx.recv().await {
                let projector = EventProjector {
                    message_repository: message_repository.clone(),
                    conversation_repository: conversation_repository.clone(),
                    event_rx: mpsc_unbounded_receiver(),
                };
                if let Err(e) = projector.project_event(&event).await {
                    error!("Failed to project event {}: {}", event.event_id, e);
                }
            }
        })
    }
    
    async fn project_event(&self, event: &DomainEvent) -> anyhow::Result<()> {
        match event.event_type.as_str() {
            "Session.LoggedIn" | "Session.LoggedOut" | "Session.Expired" => {
                Ok(())
            }
            
            "Connection.Connected" | "Connection.Disconnected" | "Connection.Reconnecting" => {
                Ok(())
            }
            
            // Message 事件
            "Message.Created" | "Message.Sent" | "Message.Delivered" | "Message.Read" | 
            "Message.SendFailed" | "Message.Recalled" => {
                self.project_message_event(event).await
            }
            
            // Conversation 事件
            "Conversation.Created" | "Conversation.UnreadUpdated" | "Conversation.LastMessageUpdated" => {
                self.project_conversation_event(event).await
            }
            
            // Sync 事件（不需要投影到存储）
            "Sync.BootstrapStarted" | "Sync.BootstrapCompleted" | "Sync.BootstrapFailed" |
            "Sync.AsyncStarted" | "Sync.AsyncCompleted" | "Sync.AsyncFailed" => {
                Ok(())
            }
            
            _ => {
                warn!("Unknown event type: {}, skipping projection", event.event_type);
                Ok(())
            }
        }
    }
    
    async fn project_message_event(&self, event: &DomainEvent) -> anyhow::Result<()> {
        use crate::domain::message::Message;
        
        let message_id = event.data.get("message_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing message_id in event data"))?;
        
        if let Some(message_json) = event.data.get("message") {
            if let Ok(message) = serde_json::from_value::<Message>(message_json.clone()) {
                // 检查是否是操作消息（不应该被保存）
                if message.message_type != crate::domain::message::MessageType::Operation {
                    self.message_repository.save(&message).await?;
                    info!(message_id = message_id, "Projected message to MessageRepository");
                } else {
                    tracing::debug!(
                        message_id = %message_id,
                        "跳过保存操作消息（操作消息不应该被保存为普通消息）"
                    );
                }
                return Ok(());
            }
        }
        
        // 如果事件中没有完整消息，尝试从 MessageRepository 加载
        if let Some(mut message) = self.message_repository.find_by_id(message_id).await? {
            match event.event_type.as_str() {
                "Message.Delivered" => {
                    message.mark_delivered()?;
                }
                "Message.Read" => {
                    if let Some(user_id) = event.data.get("user_id").and_then(|v| v.as_str()) {
                        message.mark_read(user_id.to_string())?;
                    }
                }
                "Message.Recalled" => {
                    if let Some(operator_id) = event.data.get("operator_id").and_then(|v| v.as_str()) {
                        let reason = event.data.get("reason").and_then(|v| v.as_str()).map(|s| s.to_string());
                        message.recall(operator_id.to_string(), reason)?;
                    }
                }
                "Message.Edited" => {
                    // 编辑事件已经在事件数据中包含新内容，这里只更新版本
                    message.version += 1;
                    message.updated_at = chrono::Utc::now();
                }
                _ => {
                    // 其他状态变更事件
                    message.version += 1;
                    message.updated_at = chrono::Utc::now();
                }
            }
            
            // 写回 MessageRepository
            // 检查是否是操作消息（不应该被保存）
            if message.message_type != crate::domain::message::MessageType::Operation {
                self.message_repository.save(&message).await?;
            } else {
                tracing::debug!(
                    message_id = %message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("<none>"),
                    "跳过保存操作消息（操作消息不应该被保存为普通消息）"
                );
            }
            
            info!(
                message_id = message_id,
                event_type = %event.event_type,
                "Updated message state in MessageRepository"
            );
        }
        
        Ok(())
    }
    
    /// 投影会话事件
    async fn project_conversation_event(&self, event: &DomainEvent) -> anyhow::Result<()> {
        use crate::domain::conversation::Conversation;
        
        // 从事件数据中提取会话信息
        let conversation_id = event.data.get("conversation_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing conversation_id in event data"))?;
        
        // 尝试从事件数据中加载完整会话
        if let Some(conversation_json) = event.data.get("conversation") {
            // 尝试从 JSON 反序列化会话
            if let Ok(conversation) = serde_json::from_value::<Conversation>(conversation_json.clone()) {
                // 尝试查找现有会话，如果存在则更新，否则保存
                if self.conversation_repository.find_by_id(conversation_id).await.ok().flatten().is_some() {
                    self.conversation_repository.update(&conversation).await?;
                } else {
                    self.conversation_repository.save(&conversation).await?;
                }
                info!(conversation_id = conversation_id, "Projected conversation to ConversationRepository");
                return Ok(());
            }
        }
        
        // 如果事件中没有完整会话，尝试从 ConversationRepository 加载
        if let Some(mut conversation) = self.conversation_repository.find_by_id(conversation_id).await? {
            match event.event_type.as_str() {
                "Conversation.UnreadUpdated" => {
                    if let Some(unread_count) = event.data.get("unread_count").and_then(|v| v.as_u64()) {
                        conversation.unread_count = unread_count as u32;
                        conversation.updated_at = chrono::Utc::now();
                        conversation.version += 1;
                    }
                }
                "Conversation.LastMessageUpdated" => {
                    if let Some(last_msg_json) = event.data.get("last_message") {
                        if let Ok(last_message) = serde_json::from_value::<crate::domain::conversation::MessagePreview>(last_msg_json.clone()) {
                            conversation.last_message = Some(last_message);
                        }
                    }
                    conversation.updated_at = chrono::Utc::now();
                    conversation.version += 1;
                }
                "Conversation.MarkedAsRead" => {
                    conversation.unread_count = 0;
                    if let Some(last_read_seq) = event.data.get("last_read_seq").and_then(|v| v.as_u64()) {
                        conversation.last_read_seq = last_read_seq;
                    }
                    conversation.updated_at = chrono::Utc::now();
                    conversation.version += 1;
                }
                "Conversation.DraftUpdated" => {
                    if let Some(draft) = event.data.get("draft").and_then(|v| v.as_str()) {
                        conversation.draft = Some(draft.to_string());
                    } else {
                        conversation.draft = None;
                    }
                    conversation.updated_at = chrono::Utc::now();
                    conversation.version += 1;
                }
                "Conversation.Hidden" => {
                    // 隐藏会话
                    use crate::domain::conversation::ConversationVisibility;
                    conversation.visibility = ConversationVisibility::Private;
                    conversation.updated_at = chrono::Utc::now();
                    conversation.version += 1;
                }
                "Conversation.Deleted" => {
                    // 删除会话
                    use crate::domain::conversation::ConversationLifecycleState;
                    conversation.lifecycle_state = ConversationLifecycleState::Deleted;
                    conversation.updated_at = chrono::Utc::now();
                    conversation.version += 1;
                }
                "Conversation.Updated" => {
                    // 更新会话信息
                    if let Some(display_name) = event.data.get("display_name").and_then(|v| v.as_str()) {
                        conversation.display_name = display_name.to_string();
                    }
                    if let Some(avatar_url) = event.data.get("avatar_url").and_then(|v| v.as_str()) {
                        conversation.avatar_url = Some(avatar_url.to_string());
                    }
                    if let Some(description) = event.data.get("description").and_then(|v| v.as_str()) {
                        conversation.description = Some(description.to_string());
                    }
                    if let Some(announcement) = event.data.get("announcement").and_then(|v| v.as_str()) {
                        conversation.announcement = Some(announcement.to_string());
                    }
                    conversation.updated_at = chrono::Utc::now();
                    conversation.version += 1;
                }
                _ => {
                    // 其他事件类型，只更新时间戳
                    conversation.updated_at = chrono::Utc::now();
                    conversation.version += 1;
                }
            }
            
            // 写回 ConversationRepository
            self.conversation_repository.update(&conversation).await?;
            
            info!(
                conversation_id = conversation_id,
                event_type = %event.event_type,
                "Updated conversation from event"
            );
        } else {
            // 会话不存在，可能需要创建新会话
            // 这种情况通常发生在 Conversation.Created 事件
            // 如果事件数据中包含完整会话，则创建
            if let Some(conversation_json) = event.data.get("conversation") {
                if let Ok(conversation) = serde_json::from_value::<Conversation>(conversation_json.clone()) {
                    self.conversation_repository.save(&conversation).await?;
                    info!(
                        conversation_id = conversation_id,
                        "Created conversation from event"
                    );
                } else {
                    warn!(
                        conversation_id = conversation_id,
                        "Failed to deserialize conversation from event data"
                    );
                }
            } else {
                warn!(
                    conversation_id = conversation_id,
                    "Conversation not found in ConversationRepository and no conversation data in event"
                );
            }
        }
        
        Ok(())
    }
}

/// 事件投影器构建器
pub struct EventProjectorBuilder {
    message_repository: Option<Arc<dyn MessageRepository>>,
    conversation_repository: Option<Arc<dyn ConversationRepository>>,
}

impl EventProjectorBuilder {
    pub fn new() -> Self {
        Self {
            message_repository: None,
            conversation_repository: None,
        }
    }
    
    pub fn with_message_repository(mut self, message_repository: Arc<dyn MessageRepository>) -> Self {
        self.message_repository = Some(message_repository);
        self
    }
    
    pub fn with_conversation_repository(mut self, conversation_repository: Arc<dyn ConversationRepository>) -> Self {
        self.conversation_repository = Some(conversation_repository);
        self
    }
    
    pub fn build(self) -> (EventProjector, mpsc::UnboundedSender<DomainEvent>) {
        let message_repository = self.message_repository
            .expect("MessageRepository is required");
        let conversation_repository = self.conversation_repository
            .expect("ConversationRepository is required");
        
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        
        let projector = EventProjector::new(message_repository, conversation_repository, event_rx);
        
        (projector, event_tx)
    }
}

impl Default for EventProjectorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// 临时占位函数
fn mpsc_unbounded_receiver<T>() -> mpsc::UnboundedReceiver<T> {
    let (_tx, rx) = mpsc::unbounded_channel();
    rx
}
