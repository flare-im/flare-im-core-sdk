//! 消息应用服务
//!
//! 编排消息相关的业务逻辑，提供给 API 层和服务端交互层使用

use crate::application::handlers::{MessageCommandHandler, MessageQueryHandler};
use crate::domain::MessageId;
use crate::domain::message::Message;
use crate::domain::message::repository::MessageRepository;
use anyhow::{Context, Result};
use flare_proto::Message as ProtoMessage;
use std::sync::Arc;
use tracing::debug;

/// 消息应用服务
///
/// 编排消息相关的业务逻辑，不包含具体业务规则
pub struct MessageService {
    command_handler: Arc<MessageCommandHandler>,
    query_handler: Arc<MessageQueryHandler>,
    repository: Arc<dyn MessageRepository>,
    event_bus: Arc<crate::infrastructure::event::EventBus>,
}

impl MessageService {
    pub fn new(
        command_handler: Arc<MessageCommandHandler>,
        query_handler: Arc<MessageQueryHandler>,
        repository: Arc<dyn MessageRepository>,
        event_bus: Arc<crate::infrastructure::event::EventBus>,
    ) -> Self {
        Self {
            command_handler,
            query_handler,
            repository,
            event_bus,
        }
    }

    /// 发送消息（提供给 API 层）
    pub async fn send_message(
        &self,
        session_id: crate::domain::SessionId,
        sender_id: crate::domain::UserId,
        receiver_id: Option<crate::domain::UserId>,
        channel_id: Option<String>,
        content: crate::domain::MessageContent,
        message_type: crate::domain::MessageType,
    ) -> Result<MessageId> {
        use crate::application::commands::message::SendMessageCommand;
        self.command_handler
            .handle_send_message(SendMessageCommand {
                session_id,
                sender_id,
                receiver_id,
                channel_id,
                content,
                message_type,
                seq: None, // 序列号由服务端分配
            })
            .await
    }

    /// 处理服务端推送的消息（提供给服务端交互层）
    ///
    /// 这是服务端推送消息的入口，由 MessageReceiver 调用
    ///
    /// 生产级特性：
    /// - 消息去重（基于 message_id）
    /// - 幂等性保证（重复消息不会重复处理）
    /// - 性能监控
    pub async fn on_message_received(&self, message: ProtoMessage) -> Result<()> {
        use crate::domain::{MessageId, SessionId, UserId};
        use std::time::Instant;
        let start_time = Instant::now();

        let message_id = MessageId::new(message.id.clone());

        // 1. 幂等性检查：检查消息是否已存在（去重）
        if let Ok(Some(_existing_message)) = self.repository.find_by_id(&message_id).await {
            debug!(
                message_id = %message_id,
                "Message already exists, skipping (idempotency)"
            );
            // 消息已存在，直接返回成功（幂等性保证）
            return Ok(());
        }

        // 2. 转换为 Domain Message
        let domain_message = Message::from_proto(message.clone())
            .context("Failed to convert proto message to domain message")?;

        // 3. 保存到仓储（事务性保存，确保原子性）
        self.repository
            .save(&domain_message)
            .await
            .context("Failed to save received message")?;

        // 3. 在移动 domain_message 之前保存需要的信息
        let message_id_str = domain_message.id().to_string();
        let session_id_str = domain_message.session_id().to_string();

        // 4. 调用领域行为接收消息（生成领域事件）
        // 注意：在单聊场景中，receiver_id 应该由服务端设置
        // 如果 receiver_id 为空，说明是群聊或广播消息，这里暂时使用 sender_id
        // 但实际上应该从当前用户上下文获取 receiver_id
        let receiver_id = if message.receiver_id.is_empty() {
            // 如果没有 receiver_id，说明可能是群聊或广播消息
            // 在单聊场景中，服务端应该已经设置了正确的 receiver_id
            // 这里暂时使用 sender_id 作为 fallback（不理想，但保持兼容性）
            debug!(
                message_id = %message_id_str,
                sender_id = %message.sender_id,
                session_id = %session_id_str,
                "Message has no receiver_id, using sender_id as fallback (may indicate group chat or broadcast)"
            );
            UserId::new(message.sender_id.clone())
        } else {
            debug!(
                message_id = %message_id_str,
                receiver_id = %message.receiver_id,
                sender_id = %message.sender_id,
                session_id = %session_id_str,
                "Received message with receiver_id"
            );
            UserId::new(message.receiver_id.clone())
        };
        let _receive_event = domain_message
            .receive(receiver_id.clone())
            .context("Failed to receive message")?;

        // 5. 发布基础设施事件（用于通知 API 层）
        // 克隆字符串以避免移动
        let message_id_for_event = message_id_str.clone();
        let session_id_for_message_event = session_id_str.clone();
        self.event_bus
            .publish(crate::infrastructure::event::Event::Message(
                crate::infrastructure::event::MessageEvent::MessageReceived {
                    message_id: message_id_for_event,
                    session_id: session_id_for_message_event,
                },
            ));

        // 5. 更新会话信息（未读数、最后消息等）
        // 按照微信/Telegram/飞书标准：收到消息后更新会话的未读数和最后消息
        // 注意：未读数应该由服务端推送更新，这里不自动增加
        // 如果消息不是自己发送的，未读数会在服务端推送的会话更新中更新

        // 调用 SessionCommandHandler 更新会话
        // 注意：这里需要访问 session_command_handler，但 MessageService 没有这个依赖
        // 实际应该通过事件总线通知 SessionService 更新，或者 MessageService 需要注入 SessionCommandHandler
        // 这里暂时通过事件总线发布事件，由 SessionService 监听并更新
        let session_id_for_session_event = session_id_str.clone();
        self.event_bus
            .publish(crate::infrastructure::event::Event::Session(
                crate::infrastructure::event::SessionEvent::SessionUpdated {
                    session_id: session_id_for_session_event,
                },
            ));

        // 6. 记录性能指标（如果启用）
        // 注意：这里需要 Metrics，但 MessageService 没有这个依赖
        // 实际应该通过事件总线或依赖注入获取 Metrics

        debug!(
            message_id = %message_id_str,
            session_id = %session_id_str,
            latency_ms = start_time.elapsed().as_millis(),
            "Message received and processed"
        );

        Ok(())
    }

    /// 获取消息列表（提供给 API 层）
    pub async fn get_messages(
        &self,
        session_id: crate::domain::SessionId,
        limit: usize,
        before_message_id: Option<crate::domain::MessageId>,
    ) -> Result<Vec<flare_proto::Message>> {
        use crate::application::queries::message::GetMessagesQuery;
        self.query_handler
            .handle_get_messages(GetMessagesQuery {
                session_id,
                limit,
                before_message_id,
            })
            .await
    }
}
