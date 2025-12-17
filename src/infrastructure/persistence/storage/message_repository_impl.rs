//! 消息仓储实现
//!
//! 实现 domain::message::repository::MessageRepository 接口

use crate::domain::message::model::{Message, MessageId, SessionId};
use crate::domain::message::repository::MessageRepository;
use anyhow::{Context, Result};
use async_trait::async_trait;
use flare_proto::MessageStatus;
use std::sync::Arc;

/// 消息仓储实现
pub struct MessageRepositoryImpl {
    /// 存储后端
    storage: Arc<dyn crate::infrastructure::storage::StorageBackend>,
}

impl MessageRepositoryImpl {
    pub fn new(storage: Arc<dyn crate::infrastructure::storage::StorageBackend>) -> Self {
        Self { storage }
    }
}

#[async_trait]
impl MessageRepository for MessageRepositoryImpl {
    async fn save(&self, message: &Message) -> Result<()> {
        // 转换为 ProtoMessage
        let proto_message = message.to_proto();

        // 保存到存储
        self.storage
            .save_message(&proto_message)
            .await
            .context("Failed to save message")
    }

    async fn find_by_id(&self, id: &MessageId) -> Result<Option<Message>> {
        let proto_message = self
            .storage
            .get_message(id.as_str())
            .await
            .context("Failed to get message")?;

        match proto_message {
            Some(msg) => Ok(Some(Message::from_proto(msg)?)),
            None => Ok(None),
        }
    }

    async fn find_by_session(
        &self,
        session_id: &SessionId,
        limit: usize,
        before: Option<&MessageId>,
    ) -> Result<Vec<Message>> {
        let proto_messages = self
            .storage
            .get_messages(session_id.as_str(), limit, before.map(|id| id.to_string()))
            .await
            .context("Failed to get messages")?;

        let mut messages = Vec::new();
        for proto_msg in proto_messages {
            match Message::from_proto(proto_msg) {
                Ok(msg) => messages.push(msg),
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to convert proto message to domain message");
                }
            }
        }

        Ok(messages)
    }

    async fn delete(&self, id: &MessageId) -> Result<()> {
        self.storage
            .delete_message(id.as_str())
            .await
            .context("Failed to delete message")
    }

    async fn delete_batch(&self, ids: Vec<MessageId>) -> Result<()> {
        for id in ids {
            self.delete(&id).await?;
        }
        Ok(())
    }

    async fn update_status(&self, id: &MessageId, status: MessageStatus) -> Result<()> {
        // 获取消息
        let mut message = self
            .find_by_id(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;

        // 更新状态（这里需要修改 Message 聚合根以支持状态更新）
        // TODO: 在 Message 聚合根中添加状态更新方法

        // 保存更新后的消息
        self.save(&message).await
    }
}
