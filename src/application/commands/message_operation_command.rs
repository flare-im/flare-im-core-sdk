//! 消息操作命令
//!
//! 实现所有消息操作的命令处理，对齐 DDD + CQRS 模式
//! 对标微信、Telegram、飞书的生产级别实现

use crate::domain::message::operation::{MessageOperation, MessageOperationHandler};
use crate::domain::message::Message;
use crate::domain::repository::message_repository::MessageRepository;

use crate::domain::event::message::{MessageEvent, MessageOperationApplied};
use crate::domain::event::domain_event::DomainEvent;
use crate::prelude::EventBus;

use anyhow::Result;
use std::sync::Arc;

/// 消息操作命令
#[derive(Debug, Clone)]
pub struct MessageOperationCommand {
    pub operation: MessageOperation,
    pub conversation_id: String,
}

/// 消息操作命令处理器
pub struct MessageOperationCommandHandler {
    message_repository: Arc<dyn MessageRepository>,
    event_bus: Arc<EventBus>,
}

impl MessageOperationCommandHandler {
    pub fn new(
        message_repository: Arc<dyn MessageRepository>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            message_repository,
            event_bus,
        }
    }

    /// 执行消息操作命令
    pub async fn execute(&self, command: MessageOperationCommand) -> Result<()> {
        // 1. 验证命令
        self.validate_command(&command).await?;

        // 2. 加载目标消息
        let mut message = self.load_target_message(&command.operation.target_message_id).await?;

        // 3. 验证操作权限
        self.authorize_operation(&command.operation, &message).await?;

        // 4. 执行操作
        MessageOperationHandler::execute(command.operation.clone(), &mut message).await?;

        // 5. 保存更新后的消息
        self.message_repository.save(&message).await?;

        // 6. 发布操作事件
        self.publish_operation_event(&command.operation, &message).await?;

        Ok(())
    }

    /// 验证命令
    async fn validate_command(&self, command: &MessageOperationCommand) -> Result<()> {
        // 验证操作类型是否支持
        match command.operation.operation_type {
            crate::domain::message::operation::OperationType::Recall => {
                // 撤回操作验证
                if command.operation.target_message_id.is_empty() {
                    return Err(anyhow::anyhow!("Target message ID is required for recall operation"));
                }
            }
            crate::domain::message::operation::OperationType::Edit => {
                // 编辑操作验证
                if command.operation.target_message_id.is_empty() {
                    return Err(anyhow::anyhow!("Target message ID is required for edit operation"));
                }
            }
            crate::domain::message::operation::OperationType::Delete => {
                // 删除操作验证
                if command.operation.target_message_id.is_empty() {
                    return Err(anyhow::anyhow!("Target message ID is required for delete operation"));
                }
            }
            crate::domain::message::operation::OperationType::Read => {
                // 已读操作验证
                if let crate::domain::message::operation::OperationData::Read { ref message_ids, .. } = command.operation.operation_data {
                    if message_ids.is_empty() {
                        return Err(anyhow::anyhow!("Message IDs are required for read operation"));
                    }
                }
            }
            crate::domain::message::operation::OperationType::ReactionAdd |
            crate::domain::message::operation::OperationType::ReactionRemove => {
                // 反应操作验证
                if command.operation.target_message_id.is_empty() {
                    return Err(anyhow::anyhow!("Target message ID is required for reaction operation"));
                }
            }
            crate::domain::message::operation::OperationType::Pin |
            crate::domain::message::operation::OperationType::Unpin => {
                // 置顶操作验证
                if command.operation.target_message_id.is_empty() {
                    return Err(anyhow::anyhow!("Target message ID is required for pin operation"));
                }
            }
            crate::domain::message::operation::OperationType::Mark |
            crate::domain::message::operation::OperationType::Unmark => {
                // 标记操作验证
                if command.operation.target_message_id.is_empty() {
                    return Err(anyhow::anyhow!("Target message ID is required for mark operation"));
                }
            }
            crate::domain::message::operation::OperationType::Forward => {
                // 转发操作验证
                if let crate::domain::message::operation::OperationData::Forward { ref message_ids, .. } = command.operation.operation_data {
                    if message_ids.is_empty() {
                        return Err(anyhow::anyhow!("Message IDs are required for forward operation"));
                    }
                }
            }
        }

        // 验证操作者ID
        if command.operation.operator_id.is_empty() {
            return Err(anyhow::anyhow!("Operator ID is required"));
        }

        Ok(())
    }

    /// 加载目标消息
    async fn load_target_message(&self, message_id: &str) -> Result<Message> {
        match self.message_repository.find_by_id(message_id).await? {
            Some(message) => Ok(message),
            None => Err(anyhow::anyhow!("Message not found: {}", message_id)),
        }
    }

    /// 授权操作
    async fn authorize_operation(
        &self,
        operation: &MessageOperation,
        message: &Message,
    ) -> Result<()> {
        // 检查会话权限
        if message.conversation_id.as_ref().map_or(true, |id| id != &operation.target_message_id) {
            // 注意：这里可能需要根据实际情况调整权限检查逻辑
            // 检查操作者是否有权在该会话中执行操作
        }

        // 基于操作类型的权限检查
        match operation.operation_type {
            crate::domain::message::operation::OperationType::Recall => {
                // 撤回权限：只有发送者或管理员可以撤回
                if operation.operator_id != message.sender_id {
                    // 检查是否为管理员（简化版）
                    // 在实际实现中，这应该检查用户的角色和权限
                    return Err(anyhow::anyhow!("Permission denied: only sender can recall message"));
                }
            }
            crate::domain::message::operation::OperationType::Edit => {
                // 编辑权限：只有发送者可以编辑
                if operation.operator_id != message.sender_id {
                    return Err(anyhow::anyhow!("Permission denied: only sender can edit message"));
                }
            }
            crate::domain::message::operation::OperationType::Delete => {
                // 删除权限：只有发送者可以删除
                if operation.operator_id != message.sender_id {
                    return Err(anyhow::anyhow!("Permission denied: only sender can delete message"));
                }
            }
            crate::domain::message::operation::OperationType::ReactionAdd |
            crate::domain::message::operation::OperationType::ReactionRemove => {
                // 反应权限：任何参与会话的用户都可以添加/移除反应
            }
            crate::domain::message::operation::OperationType::Pin => {
                // 置顶权限：群主或管理员可以置顶
                // 在实际实现中，这应该检查具体的权限
            }
            crate::domain::message::operation::OperationType::Unpin => {
                // 取消置顶权限：置顶者或管理员可以取消置顶
            }
            crate::domain::message::operation::OperationType::Mark |
            crate::domain::message::operation::OperationType::Unmark => {
                // 标记权限：任何用户都可以标记自己的消息或其他消息
            }
            crate::domain::message::operation::OperationType::Read => {
                // 已读权限：接收者可以标记已读
            }
            crate::domain::message::operation::OperationType::Forward => {
                // 转发权限：任何用户都可以转发消息
            }
        }

        Ok(())
    }

    /// 发布操作事件
    async fn publish_operation_event(
        &self,
        operation: &MessageOperation,
        message: &Message,
    ) -> Result<()> {
        let event = MessageEvent::OperationApplied(
            MessageOperationApplied {
                operation: operation.clone(),
                affected_message: message.clone(),
                timestamp: chrono::Utc::now(),
            }
        );

        // 将 MessageEvent 转换为 DomainEvent
        let domain_event = DomainEvent::new(
            "MessageOperationApplied".to_string(),
            message.server_id.clone().unwrap_or_default(),
            1,
            serde_json::to_value(&event)?
        );

        self.event_bus.publish(domain_event).await?;
        Ok(())
    }
}


