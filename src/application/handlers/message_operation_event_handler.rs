//! 消息操作事件处理器
//!
//! 处理消息操作相关的领域事件，触发下游服务响应
//! 对标微信、Telegram、飞书的生产级别实现

use crate::domain::event::message::{MessageEvent, MessageOperationApplied, MessageRecallRequested, MessageEditRequested, MessageDeleteRequested, MessageReactionRequested, MessagePinRequested, MessageMarkRequested};
use crate::domain::message::operation::{OperationType, OperationData};
use crate::domain::message::Message;
use crate::domain::repository::message_repository::MessageRepository;
use crate::infrastructure::messaging::message_sender::MessageSender;
use crate::infrastructure::storage::media_cache::MediaCache;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;

/// 消息操作事件处理器
pub struct MessageOperationEventHandler {
    message_repository: Arc<dyn MessageRepository>,
    message_sender: Arc<dyn MessageSender>,
    media_cache: Arc<dyn MediaCache>,
    notification_sender: broadcast::Sender<String>, // 用于发送通知给客户端
}

impl MessageOperationEventHandler {
    pub fn new(
        message_repository: Arc<dyn MessageRepository>,
        message_sender: Arc<dyn MessageSender>,
        media_cache: Arc<dyn MediaCache>,
        notification_sender: broadcast::Sender<String>,
    ) -> Self {
        Self {
            message_repository,
            message_sender,
            media_cache,
            notification_sender,
        }
    }

    /// 处理消息操作事件
    pub async fn handle(&self, event: MessageEvent) -> Result<()> {
        match event {
            MessageEvent::OperationApplied(op_applied) => {
                self.handle_operation_applied(op_applied).await?;
            }
            MessageEvent::RecallRequested(recall_req) => {
                self.handle_recall_requested(recall_req).await?;
            }
            MessageEvent::EditRequested(edit_req) => {
                self.handle_edit_requested(edit_req).await?;
            }
            MessageEvent::DeleteRequested(delete_req) => {
                self.handle_delete_requested(delete_req).await?;
            }
            MessageEvent::ReactionRequested(reaction_req) => {
                self.handle_reaction_requested(reaction_req).await?;
            }
            MessageEvent::PinRequested(pin_req) => {
                self.handle_pin_requested(pin_req).await?;
            }
            MessageEvent::MarkRequested(mark_req) => {
                self.handle_mark_requested(mark_req).await?;
            }
            _ => {
                // 其他事件不需要在此处理器中处理
                return Ok(());
            }
        }

        Ok(())
    }

    /// 处理操作应用事件
    async fn handle_operation_applied(&self, event: MessageOperationApplied) -> Result<()> {
        // 根据操作类型触发不同的下游处理
        match event.operation.operation_type {
            OperationType::Recall => {
                // 发送撤回通知
                self.send_notification(
                    &event.affected_message.conversation_id.unwrap_or_default(),
                    &event.operation.operator_id,
                    "message_recalled",
                    &event.affected_message.server_id.unwrap_or_default(),
                ).await?;
            }
            OperationType::Edit => {
                info!(
                    message_id = %event.affected_message.server_id.as_deref().unwrap_or(""),
                    conversation_id = %event.affected_message.conversation_id.as_deref().unwrap_or(""),
                    editor_id = %event.operation.operator_id,
                    "收到编辑操作（操作已应用）"
                );
                // 发送编辑通知
                self.send_notification(
                    &event.affected_message.conversation_id.unwrap_or_default(),
                    &event.operation.operator_id,
                    "message_edited",
                    &event.affected_message.server_id.unwrap_or_default(),
                ).await?;
            }
            OperationType::Delete => {
                // 发送删除通知
                self.send_notification(
                    &event.affected_message.conversation_id.unwrap_or_default(),
                    &event.operation.operator_id,
                    "message_deleted",
                    &event.affected_message.server_id.unwrap_or_default(),
                ).await?;
            }
            OperationType::ReactionAdd | OperationType::ReactionRemove => {
                // 发送反应通知
                self.send_notification(
                    &event.affected_message.conversation_id.unwrap_or_default(),
                    &event.operation.operator_id,
                    "message_reaction_changed",
                    &event.affected_message.server_id.unwrap_or_default(),
                ).await?;
            }
            OperationType::Pin | OperationType::Unpin => {
                // 发送置顶/取消置顶通知
                self.send_notification(
                    &event.affected_message.conversation_id.unwrap_or_default(),
                    &event.operation.operator_id,
                    "message_pin_changed",
                    &event.affected_message.server_id.unwrap_or_default(),
                ).await?;
            }
            OperationType::Mark | OperationType::Unmark => {
                // 发送标记/取消标记通知
                self.send_notification(
                    &event.affected_message.conversation_id.unwrap_or_default(),
                    &event.operation.operator_id,
                    "message_mark_changed",
                    &event.affected_message.server_id.unwrap_or_default(),
                ).await?;
            }
            OperationType::Read => {
                // 发送已读通知
                self.send_notification(
                    &event.affected_message.conversation_id.unwrap_or_default(),
                    &event.operation.operator_id,
                    "message_read",
                    &event.affected_message.server_id.unwrap_or_default(),
                ).await?;
            }
        }

        Ok(())
    }

    /// 处理撤回请求事件
    async fn handle_recall_requested(&self, event: MessageRecallRequested) -> Result<()> {
        // 加载原始消息
        let mut message = self.load_message(&event.message_id).await?;
        
        // 应用撤回操作
        message.recall(event.operator_id.clone(), event.reason.clone())?;
        
        // 保存更新后的消息
        self.message_repository.save(&message).await?;
        
        // 发送撤回通知给会话中的其他成员
        self.broadcast_operation_to_conversation(
            &message.conversation_id.unwrap_or_default(),
            &event.operator_id,
            OperationType::Recall,
            &event.message_id,
        ).await?;

        Ok(())
    }

    /// 处理编辑请求事件
    async fn handle_edit_requested(&self, event: MessageEditRequested) -> Result<()> {
        info!(
            message_id = %event.message_id,
            editor_id = %event.operator_id,
            "收到编辑请求（本地应用）"
        );
        // 加载原始消息
        let mut message = self.load_message(&event.message_id).await?;
        
        // 应用编辑操作
        message.edit_with_details(
            event.new_content,
            event.operator_id.clone(),
            event.reason,
            true,
            0,
        )?;
        
        // 保存更新后的消息
        self.message_repository.save(&message).await?;
        
        // 发送编辑通知给会话中的其他成员
        self.broadcast_operation_to_conversation(
            &message.conversation_id.unwrap_or_default(),
            &event.operator_id,
            OperationType::Edit,
            &event.message_id,
        ).await?;

        Ok(())
    }

    /// 处理删除请求事件
    async fn handle_delete_requested(&self, event: MessageDeleteRequested) -> Result<()> {
        // 加载原始消息
        let mut message = self.load_message(&event.message_id).await?;
        
        // 根据删除类型应用操作
        match event.delete_type {
            crate::domain::message::operation::DeleteType::Soft => {
                // 软删除：设置可见性为隐藏
                message.visibility.insert(
                    event.operator_id.clone(),
                    crate::domain::message::VisibilityStatus::Hidden,
                );
            }
            crate::domain::message::operation::DeleteType::Hard => {
                // 硬删除：设置可见性为已删除
                message.visibility.insert(
                    event.operator_id.clone(),
                    crate::domain::message::VisibilityStatus::Deleted,
                );
            }
        }
        
        // 记录删除原因
        if let Some(reason) = event.reason {
            message.attributes.insert("delete_reason".to_string(), reason);
        }
        
        message.version += 1;
        message.updated_at = chrono::Utc::now();
        
        // 保存更新后的消息
        self.message_repository.save(&message).await?;
        
        // 发送删除通知给会话中的其他成员
        if event.notify_others {
            self.broadcast_operation_to_conversation(
                &message.conversation_id.unwrap_or_default(),
                &event.operator_id,
                OperationType::Delete,
                &event.message_id,
            ).await?;
        }

        Ok(())
    }

    /// 处理反应请求事件
    async fn handle_reaction_requested(&self, event: MessageReactionRequested) -> Result<()> {
        // 加载原始消息
        let mut message = self.load_message(&event.message_id).await?;
        
        // 应用反应操作
        match event.action {
            crate::domain::message::operation::ReactionAction::Add => {
                message.add_reaction(event.emoji.clone(), event.operator_id.clone());
            }
            crate::domain::message::operation::ReactionAction::Remove => {
                message.remove_reaction(event.emoji.clone(), event.operator_id.clone());
            }
        }
        
        // 保存更新后的消息
        self.message_repository.save(&message).await?;
        
        // 发送反应通知给会话中的其他成员
        self.broadcast_operation_to_conversation(
            &message.conversation_id.unwrap_or_default(),
            &event.operator_id,
            if event.action == crate::domain::message::operation::ReactionAction::Add {
                OperationType::ReactionAdd
            } else {
                OperationType::ReactionRemove
            },
            &event.message_id,
        ).await?;

        Ok(())
    }

    /// 处理置顶请求事件
    async fn handle_pin_requested(&self, event: MessagePinRequested) -> Result<()> {
        // 加载原始消息
        let mut message = self.load_message(&event.message_id).await?;
        
        // 应用置顶操作
        message.attributes.insert("pinned".to_string(), "true".to_string());
        message.attributes.insert("pinned_at".to_string(), chrono::Utc::now().to_rfc3339());
        message.attributes.insert("pinned_by".to_string(), event.operator_id.clone());
        if let Some(reason) = event.reason {
            message.attributes.insert("pin_reason".to_string(), reason);
        }
        if let Some(expire_at) = event.expire_at {
            message.attributes.insert("pin_expire_at".to_string(), expire_at.to_rfc3339());
        }
        message.version += 1;
        message.updated_at = chrono::Utc::now();
        
        // 保存更新后的消息
        self.message_repository.save(&message).await?;
        
        // 发送置顶通知给会话中的其他成员
        self.broadcast_operation_to_conversation(
            &message.conversation_id.unwrap_or_default(),
            &event.operator_id,
            OperationType::Pin,
            &event.message_id,
        ).await?;

        Ok(())
    }

    /// 处理标记请求事件
    async fn handle_mark_requested(&self, event: MessageMarkRequested) -> Result<()> {
        // 加载原始消息
        let mut message = self.load_message(&event.message_id).await?;
        
        // 应用标记操作
        message.attributes.insert(
            format!("mark_type_{:?}", event.mark_type),
            "true".to_string(),
        );
        message.attributes.insert("marked_at".to_string(), chrono::Utc::now().to_rfc3339());
        message.attributes.insert("marked_by".to_string(), event.operator_id.clone());
        if let Some(color) = event.color {
            message.attributes.insert("mark_color".to_string(), color);
        }
        message.version += 1;
        message.updated_at = chrono::Utc::now();
        
        // 保存更新后的消息
        self.message_repository.save(&message).await?;
        
        // 发送标记通知给会话中的其他成员
        self.broadcast_operation_to_conversation(
            &message.conversation_id.unwrap_or_default(),
            &event.operator_id,
            OperationType::Mark,
            &event.message_id,
        ).await?;

        Ok(())
    }

    /// 加载消息
    async fn load_message(&self, message_id: &str) -> Result<Message> {
        match self.message_repository.find_by_id(message_id).await? {
            Some(message) => Ok(message),
            None => Err(anyhow::anyhow!("Message not found: {}", message_id)),
        }
    }

    /// 发送通知
    async fn send_notification(
        &self,
        conversation_id: &str,
        operator_id: &str,
        operation_type: &str,
        target_message_id: &str,
    ) -> Result<()> {
        let notification = format!(
            r#"{{"conversation_id":"{}","operator_id":"{}","operation_type":"{}","target_message_id":"{}"}}"#,
            conversation_id, operator_id, operation_type, target_message_id
        );
        
        let _ = self.notification_sender.send(notification);
        Ok(())
    }

    /// 广播操作到会话中的其他成员
    async fn broadcast_operation_to_conversation(
        &self,
        conversation_id: &str,
        operator_id: &str,
        operation_type: OperationType,
        target_message_id: &str,
    ) -> Result<()> {
        // 构建操作通知消息
        let operation_notification = self.build_operation_notification(
            conversation_id,
            operator_id,
            operation_type,
            target_message_id,
        )?;
        
        // 发送到会话中的其他成员
        self.message_sender.send_to_conversation(conversation_id, &operation_notification).await?;
        
        Ok(())
    }

    /// 构建操作通知消息
    fn build_operation_notification(
        &self,
        conversation_id: &str,
        operator_id: &str,
        operation_type: OperationType,
        target_message_id: &str,
    ) -> Result<String> {
        let operation_str = match operation_type {
            OperationType::Recall => "recall",
            OperationType::Edit => "edit",
            OperationType::Delete => "delete",
            OperationType::Read => "read",
            OperationType::ReactionAdd => "reaction_add",
            OperationType::ReactionRemove => "reaction_remove",
            OperationType::Pin => "pin",
            OperationType::Unpin => "unpin",
            OperationType::Mark => "mark",
            OperationType::Unmark => "unmark",
        };
        
        Ok(format!(
            r#"{{"type":"operation_notification","conversation_id":"{}","operator_id":"{}","operation_type":"{}","target_message_id":"{}","timestamp":"{}"}}"#,
            conversation_id, operator_id, operation_str, target_message_id, chrono::Utc::now().to_rfc3339()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::message::{Message, MessageType};
    use crate::domain::repository::message_repository::MockMessageRepository;
    use crate::infrastructure::messaging::message_sender::MockMessageSender;
    use crate::infrastructure::storage::media_cache::MockMediaCache;
    use mockall::predicate::*;
    use tokio::sync::broadcast;

    #[tokio::test]
    async fn test_handle_operation_applied() {
        let (tx, _) = broadcast::channel(100);
        
        let mut mock_repo = MockMessageRepository::new();
        let mut mock_sender = MockMessageSender::new();
        let mock_cache = MockMediaCache::new();

        let test_message = Message::new(
            Some("test_msg_id".to_string()),
            "client_msg_id".to_string(),
            "sender_id".to_string(),
            MessageType::Text,
            b"test content".to_vec(),
        );

        mock_repo
            .expect_find_by_id()
            .returning(move |_| Ok(Some(test_message.clone())));
        mock_repo
            .expect_save()
            .returning(|_| Ok(()));

        mock_sender
            .expect_send_to_conversation()
            .returning(|_, _| Ok(()));

        let handler = MessageOperationEventHandler::new(
            Arc::new(mock_repo),
            Arc::new(mock_sender),
            Arc::new(mock_cache),
            tx,
        );

        // 测试撤回请求事件
        let event = MessageEvent::RecallRequested(
            MessageRecallRequested {
                message_id: "test_msg_id".to_string(),
                operator_id: "operator_id".to_string(),
                reason: Some("Test reason".to_string()),
                timestamp: chrono::Utc::now(),
            }
        );

        let result = handler.handle(event).await;
        assert!(result.is_ok());
    }
}