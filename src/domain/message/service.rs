//! 消息领域服务接口和实现
//!
//! 封装复杂的业务逻辑，不依赖基础设施

use crate::domain::message::model::{Message, MessageId, SessionId, UserId};
use crate::domain::{MessageContent, MessageType};
use crate::shared::generate_unique_message_id;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;

/// 消息领域服务接口
///
/// 封装消息相关的复杂业务逻辑
#[async_trait]
pub trait MessageDomainService: Send + Sync {
    /// 创建消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `content`: 消息内容
    /// - `sender_id`: 发送者 ID
    /// - `message_type`: 消息类型
    ///
    /// # 返回
    /// - `Message`: 创建的消息聚合根
    async fn create_message(
        &self,
        session_id: SessionId,
        content: MessageContent,
        sender_id: UserId,
        message_type: MessageType,
    ) -> Result<Message>;

    /// 验证消息
    ///
    /// # 参数
    /// - `message`: 要验证的消息
    ///
    /// # 返回
    /// - `Result<()>`: 验证结果
    async fn validate_message(&self, message: &Message) -> Result<()>;

    /// 生成消息 ID
    ///
    /// # 返回
    /// - `MessageId`: 生成的消息 ID
    async fn generate_message_id(&self) -> Result<MessageId>;

    /// 创建转发消息
    ///
    /// # 参数
    /// - `original_message`: 原始消息
    /// - `target_session_id`: 目标会话 ID
    /// - `forwarder_id`: 转发者 ID
    ///
    /// # 返回
    /// - `Message`: 创建的转发消息
    async fn create_forward_message(
        &self,
        original_message: &Message,
        target_session_id: SessionId,
        forwarder_id: UserId,
    ) -> Result<Message> {
        // 默认实现：创建新消息，内容与原消息相同
        self.create_message(
            target_session_id,
            original_message.content().clone(),
            forwarder_id,
            original_message.message_type().clone(),
        )
        .await
    }
}

/// 消息领域服务实现
pub struct MessageDomainServiceImpl {
    /// 存储后端（用于生成唯一 ID）
    storage: Option<Arc<dyn crate::infrastructure::storage::StorageBackend>>,
}

impl MessageDomainServiceImpl {
    pub fn new() -> Self {
        Self { storage: None }
    }

    pub fn with_storage(storage: Arc<dyn crate::infrastructure::storage::StorageBackend>) -> Self {
        Self {
            storage: Some(storage),
        }
    }
}

#[async_trait]
impl MessageDomainService for MessageDomainServiceImpl {
    async fn create_message(
        &self,
        session_id: SessionId,
        content: MessageContent,
        sender_id: UserId,
        message_type: MessageType,
    ) -> Result<Message> {
        // 生成消息 ID
        let message_id = self.generate_message_id().await?;

        // 创建消息聚合根
        let message = Message::new(message_id, session_id, sender_id, content, message_type);

        // 验证消息
        self.validate_message(&message).await?;

        Ok(message)
    }

    async fn validate_message(&self, message: &Message) -> Result<()> {
        message.validate()
    }

    async fn generate_message_id(&self) -> Result<MessageId> {
        if let Some(ref storage) = self.storage {
            let id = generate_unique_message_id(storage.as_ref(), Some(10))
                .await
                .context("Failed to generate unique message ID")?;
            Ok(MessageId::from(id))
        } else {
            // 如果没有存储后端，使用 UUID
            Ok(MessageId::new(uuid::Uuid::new_v4().to_string()))
        }
    }
}
