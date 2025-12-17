//! 消息接收器
//!
//! 处理服务端推送的消息，这是服务端消息的入口
//!
//! ## 职责
//!
//! 1. 接收服务端推送的消息（从 infrastructure 层调用）
//! 2. 解析和验证消息
//! 3. 调用 MessageService 处理消息
//! 4. 发布领域事件

use crate::application::services::MessageService;
use crate::domain::message::Message;
use anyhow::Result;
use std::sync::Arc;

/// 消息接收器
///
/// 处理从服务端推送的消息
pub struct MessageReceiver {
    message_service: Arc<MessageService>,
}

impl MessageReceiver {
    pub fn new(message_service: Arc<MessageService>) -> Self {
        Self { message_service }
    }

    /// 接收服务端推送的消息
    ///
    /// 这是服务端消息的入口，由 infrastructure 层调用
    pub async fn receive(&self, message: Message) -> Result<()> {
        // 转换为 ProtoMessage
        let proto_message = message.to_proto();

        // 委托给 MessageService 处理
        self.message_service
            .on_message_received(proto_message)
            .await
    }

    /// 接收批量消息
    pub async fn receive_batch(&self, messages: Vec<Message>) -> Result<()> {
        for message in messages {
            if let Err(e) = self.receive(message).await {
                tracing::warn!(error = %e, "Failed to receive message in batch");
            }
        }
        Ok(())
    }
}
