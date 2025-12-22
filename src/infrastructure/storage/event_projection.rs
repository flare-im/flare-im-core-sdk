//! 事件投影（Event Projection）
//!
//! 将领域事件投影到 ReadStore，更新读模型
//! 对标微信、Telegram、飞书的生产级别实现

use std::sync::Arc;
use tokio::sync::mpsc;
use crate::domain::event::DomainEvent;
use crate::domain::repository::ReadStore;
use tracing::{error, info, warn};

/// 事件投影器
pub struct EventProjector {
    read_store: Arc<dyn ReadStore>,
    event_rx: mpsc::UnboundedReceiver<DomainEvent>,
}

impl EventProjector {
    /// 创建新的事件投影器
    pub fn new(
        read_store: Arc<dyn ReadStore>,
        event_rx: mpsc::UnboundedReceiver<DomainEvent>,
    ) -> Self {
        Self {
            read_store,
            event_rx,
        }
    }
    
    /// 启动事件投影（后台任务）
    pub fn start(self) -> tokio::task::JoinHandle<()> {
        let read_store = self.read_store.clone();
        tokio::spawn(async move {
            let mut event_rx = self.event_rx;
            
            while let Some(event) = event_rx.recv().await {
                // 创建新的投影器实例来处理事件
                let projector = EventProjector {
                    read_store: read_store.clone(),
                    event_rx: mpsc_unbounded_receiver(), // 占位，不会使用
                };
                if let Err(e) = projector.project_event(&event).await {
                    error!("Failed to project event {}: {}", event.event_id, e);
                }
            }
        })
    }
    
    /// 投影单个事件到 ReadStore
    async fn project_event(&self, event: &DomainEvent) -> anyhow::Result<()> {
        match event.event_type.as_str() {
            // Session 事件（不需要投影到 ReadStore）
            "Session.LoggedIn" | "Session.LoggedOut" | "Session.Expired" => {
                // Session 事件不需要投影
                Ok(())
            }
            
            // Connection 事件（不需要投影到 ReadStore）
            "Connection.Connected" | "Connection.Disconnected" | "Connection.Reconnecting" => {
                // Connection 事件不需要投影
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
            
            // Sync 事件（不需要投影到 ReadStore）
            "Sync.BootstrapStarted" | "Sync.BootstrapCompleted" | "Sync.BootstrapFailed" |
            "Sync.AsyncStarted" | "Sync.AsyncCompleted" | "Sync.AsyncFailed" => {
                // Sync 事件不需要投影
                Ok(())
            }
            
            _ => {
                warn!("Unknown event type: {}, skipping projection", event.event_type);
                Ok(())
            }
        }
    }
    
    /// 投影消息事件
    async fn project_message_event(&self, event: &DomainEvent) -> anyhow::Result<()> {
        use crate::domain::message::Message;
        use crate::infrastructure::converter::MessageConverter;
        
        // 从事件数据中提取消息信息
        let message_id = event.data.get("message_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing message_id in event data"))?;
        
        // 尝试从 EventStore 加载完整消息（如果事件中包含消息数据）
        // 如果事件类型是 Message.Created 或 Message.Sent，事件数据中可能包含完整消息
        if let Some(message_json) = event.data.get("message") {
            // 尝试从 JSON 反序列化消息
            if let Ok(message) = serde_json::from_value::<Message>(message_json.clone()) {
                // 写入 ReadStore
                self.read_store.write_message(&message).await?;
                info!(
                    message_id = message_id,
                    "Projected message to ReadStore"
                );
                return Ok(());
            }
        }
        
        // 如果事件数据中没有完整消息，需要从 ReadStore 加载现有消息，更新状态，然后写回
        // 这种情况通常发生在状态变更事件（如 Message.Delivered, Message.Read）
        let query = crate::domain::repository::Query::MessageDetail {
            message_id: message_id.to_string(),
        };
        if let crate::domain::repository::QueryResult::MessageDetail { item } = self.read_store.query(query).await? {
            if !item.is_null() && item.get("message_id").is_some() {
                // 反序列化消息
                if let Ok(mut message) = serde_json::from_value::<Message>(item) {
                    // 根据事件类型更新消息状态
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
                    
                    // 写回 ReadStore
                    self.read_store.write_message(&message).await?;
                    
                    info!(
                        message_id = message_id,
                        event_type = %event.event_type,
                        "Updated message state in ReadStore"
                    );
                }
            }
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
        
        // 尝试从 EventStore 加载完整会话（如果事件中包含会话数据）
        if let Some(conversation_json) = event.data.get("conversation") {
            // 尝试从 JSON 反序列化会话
            if let Ok(conversation) = serde_json::from_value::<Conversation>(conversation_json.clone()) {
                // 写入或更新 ReadStore
                self.read_store.write_conversation(&conversation).await?;
                info!(
                    conversation_id = conversation_id,
                    "Projected conversation to ReadStore"
                );
                return Ok(());
            }
        }
        
        // 如果事件数据中没有完整会话，需要从 ReadStore 加载现有会话，更新，然后写回
        let query = crate::domain::repository::Query::ConversationDetail {
            conversation_id: conversation_id.to_string(),
        };
        if let crate::domain::repository::QueryResult::ConversationDetail { item } = self.read_store.query(query).await? {
            if !item.is_null() && item.get("conversation_id").is_some() {
                // 反序列化会话
                if let Ok(mut conversation) = serde_json::from_value::<Conversation>(item) {
                    // 根据事件类型更新会话字段
                    match event.event_type.as_str() {
                        "Conversation.UnreadUpdated" => {
                            if let Some(unread_count) = event.data.get("unread_count").and_then(|v| v.as_u64()) {
                                conversation.unread_count = unread_count as u32;
                                conversation.updated_at = chrono::Utc::now();
                                conversation.version += 1;
                            }
                        }
                        "Conversation.LastMessageUpdated" => {
                            // 更新最后一条消息（从事件数据中获取）
                            if let Some(last_msg_json) = event.data.get("last_message") {
                                if let Ok(last_message) = serde_json::from_value::<crate::domain::conversation::MessagePreview>(last_msg_json.clone()) {
                                    conversation.last_message = Some(last_message);
                                }
                            }
                            conversation.updated_at = chrono::Utc::now();
                            conversation.version += 1;
                        }
                        "Conversation.MarkedAsRead" => {
                            // 标记已读，清空未读数
                            conversation.unread_count = 0;
                            if let Some(last_read_seq) = event.data.get("last_read_seq").and_then(|v| v.as_u64()) {
                                conversation.last_read_seq = last_read_seq;
                            }
                            conversation.updated_at = chrono::Utc::now();
                            conversation.version += 1;
                        }
                        "Conversation.DraftUpdated" => {
                            // 更新草稿
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
                    
                    // 写回 ReadStore
                    self.read_store.write_conversation(&conversation).await?;
                    
                    info!(
                        conversation_id = conversation_id,
                        event_type = %event.event_type,
                        "Updated conversation from event"
                    );
                }
            } else {
                // 会话不存在，可能需要创建新会话
                // 这种情况通常发生在 Conversation.Created 事件
                // 如果事件数据中包含完整会话，则创建
                if let Some(conversation_json) = event.data.get("conversation") {
                    if let Ok(conversation) = serde_json::from_value::<Conversation>(conversation_json.clone()) {
                        self.read_store.write_conversation(&conversation).await?;
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
                        "Conversation not found in ReadStore and no conversation data in event"
                    );
                }
            }
        }
        
        Ok(())
    }
}

/// 事件投影器构建器
pub struct EventProjectorBuilder {
    read_store: Option<Arc<dyn ReadStore>>,
}

impl EventProjectorBuilder {
    pub fn new() -> Self {
        Self {
            read_store: None,
        }
    }
    
    pub fn with_read_store(mut self, read_store: Arc<dyn ReadStore>) -> Self {
        self.read_store = Some(read_store);
        self
    }
    
    pub fn build(self) -> (EventProjector, mpsc::UnboundedSender<DomainEvent>) {
        let read_store = self.read_store
            .expect("ReadStore is required");
        
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        
        let projector = EventProjector::new(read_store, event_rx);
        
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
