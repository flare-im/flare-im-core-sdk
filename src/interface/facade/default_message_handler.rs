//! 默认消息处理器
//!
//! 处理从消息队列接收到的消息，实现核心的消息处理流程
//!
//! ## 职责
//!
//! 1. **消息持久化**: 保存消息到 ReadStore
//! 2. **会话管理**: 更新或创建会话，处理未读数
//! 3. **事件发布**: 发布领域事件到 EventStore 和 EventBus
//!
//! ## 设计原则
//!
//! 1. **单一职责**: 只负责消息接收后的处理逻辑
//! 2. **无状态**: 处理器本身不保存状态，所有状态通过依赖注入
//! 3. **可测试**: 通过依赖注入，方便单元测试

use std::sync::Arc;
use async_trait::async_trait;
use crate::application::fsm::FsmManager;
use crate::domain::message_queue::MessageHandler;
use crate::domain::repository::{EventStore, MessageRepository, ConversationRepository};
use crate::infrastructure::event_bus::EventBus;
use tracing::{info, debug};

/// 默认消息处理器
///
/// 将接收到的消息通过标准流程处理：
/// 1. 保存消息到 ReadStore
/// 2. 更新或创建会话
/// 3. 发布领域事件
///
/// 对标微信、Telegram、飞书的消息接收处理流程
pub struct DefaultMessageHandler {
    message_repository: Arc<dyn MessageRepository>,
    conversation_repository: Arc<dyn ConversationRepository>,
    #[allow(dead_code)]
    event_store: Arc<dyn EventStore>,
    event_bus: Arc<EventBus>,
    #[allow(dead_code)]
    fsm: Arc<FsmManager>,
}

impl DefaultMessageHandler {
    /// 创建新的默认消息处理器
    pub fn new(
        message_repository: Arc<dyn MessageRepository>,
        conversation_repository: Arc<dyn ConversationRepository>,
        event_store: Arc<dyn EventStore>,
        event_bus: Arc<EventBus>,
        fsm: Arc<FsmManager>,
    ) -> Self {
        Self {
            message_repository,
            conversation_repository,
            event_store,
            event_bus,
            fsm,
        }
    }
}

#[async_trait]
impl MessageHandler for DefaultMessageHandler {
    async fn handle_message(&self, message: &crate::domain::message::Message) -> anyhow::Result<()> {
        use crate::domain::event::{DomainEvent, message_events, conversation_events};
        use crate::domain::service::ConversationDomainService;
        
        // 性能优化：并行执行独立操作
        let (current_user_id, conversation_opt) = tokio::join!(
            // 1. 获取当前用户ID（用于判断是否是自己发送的消息）
            self.fsm.current_user_id(),
            // 2. 并行查询会话（不阻塞消息保存）
            async {
                // 如果消息没有 conversation_id，直接返回 None
                let conversation_id = match message.conversation_id.clone() {
                    Some(id) => id,
                    None => return None,
                };
                self.conversation_repository.find_by_id(&conversation_id).await.ok().flatten()
            }
        );
        
        // 3. 保存消息到 MessageRepository（与查询并行，但需要等待完成）
        self.message_repository.save(message).await?;
        info!("Message saved to repository");
        // 4. 更新会话或创建新会话
        let (conversation, is_update) = if let Some(mut conv) = conversation_opt {
            // 更新现有会话
            let domain_service = ConversationDomainService::new();
            domain_service.update_last_message(&mut conv, message)?;
            
            // 增加未读数（如果消息不是自己发送的）
            if current_user_id.as_ref().map(|id| id.as_str()) != Some(&message.sender_id) {
                conv.unread_count += 1;
            }
            conv.max_seq = message.seq.unwrap_or(conv.max_seq);
            conv.updated_at = chrono::Utc::now();
            conv.version += 1;
            (conv, true)
        } else {
            // 创建新会话
            let mut conv = ConversationDomainService::new().create_conversation_from_message(message)?;
            if current_user_id.as_ref().map(|id| id.as_str()) == Some(&message.sender_id) {
                conv.unread_count = 0;
            }
            (conv, false)
        };
        
        // 5. 保存会话到 ConversationRepository
        if is_update {
            self.conversation_repository.update(&conversation).await?;
        } else {
            self.conversation_repository.save(&conversation).await?;
        }
        info!("Conversation updated or created");
        // 6. 性能优化：并行发布事件（不阻塞主流程，减少克隆）
        let message_id = message.server_id.clone().unwrap_or_default();
        let conversation_id = conversation.conversation_id.clone();
        let sender_id = message.sender_id.clone();
        let message_version = message.version;
        let conversation_version = conversation.version;
        let unread_count = conversation.unread_count;
        let message_content = message.content.clone(); // 克隆消息内容用于事件
        let last_message_json = conversation.last_message.as_ref().map(|m| serde_json::json!({
            "message_id": m.message_id,
            "sender_id": m.sender_id,
            "text": m.text,
        }));
        
        // 异步发布事件，不阻塞消息处理流程
        let event_store = self.event_store.clone();
        let event_bus = self.event_bus.clone();
        tokio::spawn(async move {
            // 6.1 发布消息接收事件到 EventStore（持久化）
            let message_event = DomainEvent::new(
                message_events::DELIVERED,
                &message_id,
                message_version,
                serde_json::json!({
                    "message_id": &message_id,
                    "conversation_id": &conversation_id,
                    "sender_id": &sender_id,
                }),
            );
            let _ = event_store.append(message_event).await;
            
            // 6.2 发布消息接收事件到 EventBus（实时通知 UI 层）
            // 注意：这里发布 MessageCreated 事件，让订阅者能看到消息内容
            use crate::domain::event::MessageCreated;
            let message_created = MessageCreated {
                message_id: message_id.clone(),
                conversation_id: Some(conversation_id.clone()),
                sender_id: sender_id.clone(),
                content: serde_json::json!(message_content), // 使用克隆的消息内容
            };
            
            // 发布 MessageCreated 事件（用于 UI 显示）
            let message_created_event = DomainEvent::new(
                message_events::CREATED,
                &message_id,
                message_version,
                serde_json::to_value(&message_created).unwrap_or_else(|_| serde_json::json!({
                    "message_id": &message_id,
                    "conversation_id": &conversation_id,
                    "sender_id": &sender_id,
                })),
            );
            let _ = event_bus.publish(message_created_event).await;
            
            // 发布 MessageDelivered 事件（用于状态跟踪）
            let message_delivered_event = DomainEvent::new(
                message_events::DELIVERED,
                &message_id,
                message_version,
                serde_json::json!({
                    "message_id": &message_id,
                    "conversation_id": &conversation_id,
                    "sender_id": &sender_id,
                }),
            );
            let _ = event_bus.publish(message_delivered_event).await;
            
            // 6.3 发布会话更新事件
            let conversation_event = DomainEvent::new(
                conversation_events::LAST_MESSAGE_UPDATED,
                &conversation_id,
                conversation_version,
                serde_json::json!({
                    "conversation_id": &conversation_id,
                    "unread_count": unread_count,
                    "last_message": last_message_json,
                }),
            );
            let conversation_event_clone = conversation_event.clone();
            let _ = event_store.append(conversation_event_clone).await;
            
            // 6.4 发布会话更新事件到 EventBus
            let _ = event_bus.publish(conversation_event).await;
        });
        
        debug!(
            message_id = %message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("<none>"),
            conversation_id = %conversation.conversation_id,
            "Message processed successfully"
        );
        Ok(())
    }
    
    async fn handle_error(&self, message: &crate::domain::message::Message, error: &anyhow::Error) {
        tracing::error!(
            message_id = %message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("<none>"),
            error = %error,
            "Failed to process received message"
        );
        
        // 可以在这里实现重试逻辑或错误上报
        // 例如：将失败的消息加入重试队列
    }
}
