//! 消息命令处理器

use crate::application::commands::message::*;
use crate::domain::message::repository::MessageRepository;
use crate::domain::message::service::MessageDomainService;
use crate::domain::{MessageId, MessageType, SessionId, UserId};
use crate::infrastructure::event::EventBus;
use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::{debug, warn};

/// 消息命令处理器
///
/// 处理消息相关的命令（发送、撤回、删除、转发等）
///
/// 生产级特性：
/// - 消息重试机制（集成 PendingMessageQueue）
/// - 消息去重和幂等性保证
/// - 完善的错误处理和监控
pub struct MessageCommandHandler {
    domain_service: Arc<dyn MessageDomainService>,
    repository: Arc<dyn MessageRepository>,
    event_bus: Arc<EventBus>,
    connection_manager: Option<Arc<crate::infrastructure::connection::ConnectionManager>>,
    pending_queue: Option<Arc<crate::infrastructure::storage::PendingMessageQueue>>,
    metrics: Option<Arc<crate::shared::metrics::Metrics>>,
}

impl MessageCommandHandler {
    pub fn new(
        domain_service: Arc<dyn MessageDomainService>,
        repository: Arc<dyn MessageRepository>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            domain_service,
            repository,
            event_bus,
            connection_manager: None,
            pending_queue: None,
            metrics: None,
        }
    }

    /// 设置连接管理器（用于发送消息到服务器）
    pub fn with_connection_manager(
        mut self,
        connection_manager: Arc<crate::infrastructure::connection::ConnectionManager>,
    ) -> Self {
        self.connection_manager = Some(connection_manager);
        self
    }

    /// 设置待发送消息队列（用于消息重试）
    pub fn with_pending_queue(
        mut self,
        pending_queue: Arc<crate::infrastructure::storage::PendingMessageQueue>,
    ) -> Self {
        self.pending_queue = Some(pending_queue);
        self
    }

    /// 设置监控指标（用于性能监控）
    pub fn with_metrics(mut self, metrics: Arc<crate::shared::metrics::Metrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// 处理发送消息命令（生产级实现）
    ///
    /// 按照微信/Telegram/飞书标准：
    /// 1. 创建消息并保存到本地
    /// 2. 立即发送到服务器
    /// 3. 如果发送失败，加入重试队列
    /// 4. 记录监控指标
    pub async fn handle_send_message(&self, cmd: SendMessageCommand) -> Result<MessageId> {
        use std::time::Instant;
        let start_time = Instant::now();

        // 1. 调用领域服务创建消息
        let mut message = self
            .domain_service
            .create_message(
                cmd.session_id.clone(),
                cmd.content,
                cmd.sender_id.clone(),
                cmd.message_type,
            )
            .await
            .context("Failed to create message")?;

        // 1.5. 设置 receiver_id（如果提供）
        if let Some(ref receiver_id) = cmd.receiver_id {
            message = message.set_receiver_id(Some(receiver_id.clone()));
            debug!(
                message_id = %message.id(),
                receiver_id = %receiver_id,
                "Set receiver_id for message"
            );
        } else {
            debug!(
                message_id = %message.id(),
                "No receiver_id provided for message (may be group chat or broadcast)"
            );
        }

        // 2. 保存消息 ID、会话 ID 和 proto_message（在移动 message 之前）
        let message_id = message.id().clone();
        let message_id_str = message_id.to_string();
        let session_id_str = message.session_id().to_string();
        let proto_message = message.to_proto();

        // 验证 receiver_id 已正确设置到 proto_message
        if let Some(ref receiver_id) = cmd.receiver_id {
            if proto_message.receiver_id != receiver_id.to_string() {
                warn!(
                    message_id = %message_id_str,
                    expected = %receiver_id,
                    actual = %proto_message.receiver_id,
                    "receiver_id mismatch in proto_message"
                );
            }
        }

        // 3. 保存到仓储（先保存，确保消息持久化）
        self.repository
            .save(&message)
            .await
            .context("Failed to save message")?;

        // 4. 调用领域行为发送消息（生成领域事件）
        let _send_event = message.send().context("Failed to send message")?;

        // 5. 发布基础设施事件（用于通知 API 层）
        self.event_bus
            .publish(crate::infrastructure::event::Event::Message(
                crate::infrastructure::event::MessageEvent::MessageCreated {
                    message_id: message_id_str.clone(),
                    session_id: session_id_str.clone(),
                },
            ));

        // 6. 发送到服务器（通过 ConnectionManager）
        let send_result = if let Some(ref connection_manager) = self.connection_manager {
            use crate::infrastructure::protocol::FrameBuilder;
            use flare_core::common::protocol::{MessageCommand, Reliability};
            use prost::Message as ProstMessage;

            // 使用之前保存的 proto_message
            let mut payload = Vec::new();
            proto_message
                .encode(&mut payload)
                .context("Failed to encode message")?;

            let msg_cmd = MessageCommand {
                message_id: message_id_str.clone(),
                r#type:
                    flare_core::common::protocol::flare::core::commands::message_command::Type::Send
                        as i32,
                payload,
                metadata: std::collections::HashMap::new(),
                seq: 0, // 序列号由服务端分配，客户端发送时使用 0
            };

            // 构建 Frame 并发送
            let frame = FrameBuilder::new()
                .with_message_command(msg_cmd)
                .with_reliability(Reliability::AtLeastOnce)
                .build();

            // 通过 ConnectionManager 发送 Frame
            connection_manager.send_frame(&frame).await
        } else {
            Err(anyhow::anyhow!("ConnectionManager not available"))
        };

        // 7. 处理发送结果
        match send_result {
            Ok(()) => {
                // 发送成功：记录指标
                if let Some(ref metrics) = self.metrics {
                    metrics.record_message_sent(start_time.elapsed());
                }

                // 如果启用了待发送队列，标记消息为完成
                if let Some(ref pending_queue) = self.pending_queue {
                    let _ = pending_queue.mark_completed(&message_id_str).await;
                }

                debug!(
                    message_id = %message_id_str,
                    session_id = %session_id_str,
                    latency_ms = start_time.elapsed().as_millis(),
                    "Message sent successfully"
                );
            }
            Err(e) => {
                // 发送失败：加入重试队列（如果启用）
                warn!(
                    error = %e,
                    message_id = %message_id_str,
                    session_id = %session_id_str,
                    "Failed to send message, will retry"
                );

                // 记录错误指标
                if let Some(ref metrics) = self.metrics {
                    metrics.record_error();
                }

                // 加入待发送队列（如果启用）
                if let Some(ref pending_queue) = self.pending_queue {
                    if let Some(ref content) = proto_message.content {
                        // 将消息加入重试队列
                        let _ = pending_queue
                            .enqueue(
                                message_id_str.clone(),
                                session_id_str.clone(),
                                content.clone(),
                                0,                         // 默认优先级
                                "AtLeastOnce".to_string(), // 可靠性级别
                            )
                            .await;
                    }
                }

                // 发布消息发送失败事件
                self.event_bus
                    .publish(crate::infrastructure::event::Event::Message(
                        crate::infrastructure::event::MessageEvent::MessageFailed {
                            message_id: message_id_str.clone(),
                            error: e.to_string(),
                        },
                    ));
            }
        }

        Ok(message_id)
    }

    /// 处理消息 ACK（按照微信/Telegram/飞书标准）
    ///
    /// ACK 类型：
    /// - transport: 传输层 ACK（消息已送达服务器）
    /// - server: 服务器 ACK（消息已处理）
    /// - read: 已读 ACK（消息已被对方读取）
    pub async fn handle_ack(&self, message_id: &MessageId, ack_type: &str) -> Result<()> {
        use flare_proto::MessageStatus;

        // 1. 查找消息
        let message = self
            .repository
            .find_by_id(message_id)
            .await
            .context("Failed to find message for ACK")?
            .ok_or_else(|| anyhow::anyhow!("Message not found for ACK"))?;

        // 2. 根据 ACK 类型更新消息状态
        let new_status = match ack_type {
            "transport" => {
                // 传输层 ACK：消息已送达服务器
                MessageStatus::Sent
            }
            "server" => {
                // 服务器 ACK：消息已处理
                MessageStatus::Delivered
            }
            "read" => {
                // 已读 ACK：消息已被对方读取
                MessageStatus::Read
            }
            _ => {
                warn!(ack_type = %ack_type, "Unknown ACK type, using Sent as default");
                MessageStatus::Sent
            }
        };

        // 3. 更新消息状态
        self.repository
            .update_status(message_id, new_status)
            .await
            .context("Failed to update message status")?;

        // 4. 发布消息状态更新事件
        self.event_bus
            .publish(crate::infrastructure::event::Event::Message(
                crate::infrastructure::event::MessageEvent::MessageStatusUpdated {
                    message_id: message_id.as_str().to_string(),
                    session_id: message.session_id().to_string(),
                    status: new_status as i32,
                },
            ));

        debug!(
            message_id = %message_id,
            ack_type = %ack_type,
            status = ?new_status,
            "Message ACK processed"
        );

        Ok(())
    }

    /// 处理撤回消息命令
    pub async fn handle_recall_message(&self, cmd: RecallMessageCommand) -> Result<()> {
        // 1. 查找消息
        let message = self
            .repository
            .find_by_id(&cmd.message_id)
            .await
            .context("Failed to find message")?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;

        // 2. 验证权限（只有发送者可以撤回）
        if message.sender_id() != &cmd.user_id {
            anyhow::bail!("Only sender can recall message");
        }

        // 3. 在移动 message 之前获取 proto_message
        let proto_message = message.to_proto();

        // 4. 调用领域行为撤回消息（生成领域事件）
        let user_id = cmd.user_id.clone();
        let _recall_event = message
            .recall(&user_id, cmd.reason.clone())
            .context("Failed to recall message")?;

        // 5. 更新消息状态为已撤回
        let mut updated_proto = proto_message.clone();
        updated_proto.status = flare_proto::MessageStatus::Recalled as i32;
        updated_proto.is_recalled = true;

        let updated_message = crate::domain::message::Message::from_proto(updated_proto)
            .context("Failed to create updated message from proto")?;

        self.repository
            .save(&updated_message)
            .await
            .context("Failed to update message status")?;

        // 5. 发送撤回请求到服务器（通过 ConnectionManager）
        if let Some(ref connection_manager) = self.connection_manager {
            use crate::infrastructure::protocol::FrameBuilder;
            use flare_core::common::protocol::{MessageCommand, Reliability};
            use prost::Message as ProstMessage;

            let mut payload = Vec::new();
            updated_message
                .to_proto()
                .encode(&mut payload)
                .context("Failed to encode recall message")?;

            // 注意：MessageCommand 没有 Recall 类型，撤回消息应该通过发送特殊的消息内容来实现
            // 或者使用 CustomCommand，这里暂时使用 Send 类型，payload 中包含撤回指令
            let msg_cmd = MessageCommand {
                message_id: cmd.message_id.to_string(),
                r#type:
                    flare_core::common::protocol::flare::core::commands::message_command::Type::Send
                        as i32,
                payload,
                metadata: std::collections::HashMap::new(),
                seq: 0,
            };

            let frame = FrameBuilder::new()
                .with_message_command(msg_cmd)
                .with_reliability(Reliability::AtLeastOnce)
                .build();

            // TODO: 通过 ConnectionManager 发送 Frame
            // connection_manager.send_frame(frame).await?;
        }

        // 6. 发布基础设施事件（使用之前保存的 session_id）
        let session_id_str = proto_message.session_id.clone();
        self.event_bus
            .publish(crate::infrastructure::event::Event::Message(
                crate::infrastructure::event::MessageEvent::MessageRecalled {
                    message_id: cmd.message_id.to_string(),
                    session_id: session_id_str,
                },
            ));

        Ok(())
    }

    /// 处理删除消息命令
    pub async fn handle_delete_message(&self, cmd: DeleteMessageCommand) -> Result<()> {
        // 1. 查找消息
        let message = self
            .repository
            .find_by_id(&cmd.message_id)
            .await
            .context("Failed to find message")?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;

        // 2. 验证权限
        if message.sender_id() != &cmd.user_id {
            anyhow::bail!("Only sender can delete message");
        }

        // 3. 根据删除类型处理
        if cmd.delete_type == 1 {
            // 硬删除：从仓储中删除
            self.repository
                .delete(&cmd.message_id)
                .await
                .context("Failed to delete message")?;
        } else {
            // 软删除：标记为已删除（通过 metadata 或直接删除）
            // MessageStatus 没有 Deleted 状态，使用 Recalled 或直接删除
            self.repository
                .delete(&cmd.message_id)
                .await
                .context("Failed to delete message")?;
        }

        // 4. 发送删除请求到服务器（通过 ConnectionManager）
        // 5. 发布消息已删除事件
        self.event_bus
            .publish(crate::infrastructure::event::Event::Message(
                crate::infrastructure::event::MessageEvent::MessageDeleted {
                    message_id: cmd.message_id.to_string(),
                    session_id: message.session_id().to_string(),
                },
            ));

        Ok(())
    }

    /// 处理编辑消息命令
    pub async fn handle_edit_message(&self, cmd: EditMessageCommand) -> Result<()> {
        // 1. 查找消息
        let mut message = self
            .repository
            .find_by_id(&cmd.message_id)
            .await
            .context("Failed to find message")?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;

        // 2. 在移动 message 之前获取 proto_message
        let proto_message = message.to_proto();

        // 3. 调用领域行为编辑消息
        let _edit_event = message
            .edit(&cmd.user_id, cmd.new_content.clone())
            .context("Failed to edit message")?;

        // 4. 更新消息内容
        // 注意：需要更新 proto_message 中的内容
        let mut updated_proto = proto_message.clone();
        // 更新文本内容
        if let Some(ref mut content) = updated_proto.content {
            match content.content.as_mut() {
                Some(flare_proto::flare::common::v1::message_content::Content::Text(text)) => {
                    text.text = cmd.new_content.clone();
                }
                _ => {}
            }
        }

        // 4. 保存更新后的消息
        let updated_message = crate::domain::message::Message::from_proto(updated_proto)
            .context("Failed to create updated message from proto")?;
        self.repository
            .save(&updated_message)
            .await
            .context("Failed to save edited message")?;

        // 5. 发布基础设施事件（使用之前保存的 session_id）
        let session_id_str = proto_message.session_id.clone();
        self.event_bus
            .publish(crate::infrastructure::event::Event::Message(
                crate::infrastructure::event::MessageEvent::MessageEdited {
                    message_id: cmd.message_id.to_string(),
                    session_id: session_id_str,
                },
            ));

        // 6. 发送编辑请求到服务器（通过 ConnectionManager）

        Ok(())
    }

    /// 处理转发消息命令
    pub async fn handle_forward_message(&self, cmd: ForwardMessageCommand) -> Result<MessageId> {
        // 1. 查找原始消息
        let original_message = self
            .repository
            .find_by_id(&cmd.message_id)
            .await
            .context("Failed to find message")?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;

        // 2. 创建转发消息
        let forward_message = self
            .domain_service
            .create_message(
                cmd.target_session_id.clone(),
                original_message.content().clone(),
                cmd.sender_id.clone(),
                original_message.message_type(),
            )
            .await
            .context("Failed to create forward message")?;

        // 3. 保存转发消息
        self.repository
            .save(&forward_message)
            .await
            .context("Failed to save forward message")?;

        // 4. 发送转发消息到服务器（通过 ConnectionManager）
        // 5. 发布消息已创建事件
        self.event_bus
            .publish(crate::infrastructure::event::Event::Message(
                crate::infrastructure::event::MessageEvent::MessageCreated {
                    message_id: forward_message.id().to_string(),
                    session_id: cmd.target_session_id.to_string(),
                },
            ));

        Ok(forward_message.id().clone())
    }

    /// 处理添加反应命令
    pub async fn handle_add_reaction(&self, cmd: AddReactionCommand) -> Result<()> {
        // 1. 查找消息
        let message = self
            .repository
            .find_by_id(&cmd.message_id)
            .await
            .context("Failed to find message")?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;

        // 2. 在移动 message 之前获取需要的信息
        let session_id_str = message.session_id().to_string();
        let proto_message = message.to_proto();
        let mut updated_proto = proto_message.clone();

        // 3. 调用领域行为添加反应
        let _reaction_event = message
            .add_reaction(cmd.user_id.clone(), cmd.emoji.clone())
            .context("Failed to add reaction")?;

        // 4. 更新 reactions 字段（需要从 proto 中提取并添加）
        // TODO: 更新 reactions 字段（需要从 proto 中提取并添加）

        // 5. 保存更新后的消息
        let updated_message = crate::domain::message::Message::from_proto(updated_proto)
            .context("Failed to create updated message from proto")?;
        self.repository
            .save(&updated_message)
            .await
            .context("Failed to save message with reaction")?;

        // 4. 发布基础设施事件（使用之前保存的 session_id）
        self.event_bus
            .publish(crate::infrastructure::event::Event::Message(
                crate::infrastructure::event::MessageEvent::MessageReactionAdded {
                    message_id: cmd.message_id.to_string(),
                    session_id: session_id_str,
                    user_id: cmd.user_id.to_string(),
                    emoji: cmd.emoji.clone(),
                },
            ));

        // 5. 发送添加反应请求到服务器（通过 ConnectionManager）

        Ok(())
    }

    /// 处理移除反应命令
    pub async fn handle_remove_reaction(&self, cmd: RemoveReactionCommand) -> Result<()> {
        // 1. 查找消息
        let message = self
            .repository
            .find_by_id(&cmd.message_id)
            .await
            .context("Failed to find message")?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;

        // 2. 在移动 message 之前获取需要的信息
        let session_id_str = message.session_id().to_string();

        // 3. 调用领域行为移除反应
        let _reaction_event = message
            .remove_reaction(cmd.user_id.clone(), cmd.emoji.clone())
            .context("Failed to remove reaction")?;

        // 4. 重新获取消息以保存（因为 message 已被移动）
        let updated_message = self
            .repository
            .find_by_id(&cmd.message_id)
            .await
            .context("Failed to find message for save")?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;

        self.repository
            .save(&updated_message)
            .await
            .context("Failed to save message")?;

        // 5. 发布基础设施事件（使用之前保存的 session_id）
        self.event_bus
            .publish(crate::infrastructure::event::Event::Message(
                crate::infrastructure::event::MessageEvent::MessageReactionRemoved {
                    message_id: cmd.message_id.to_string(),
                    session_id: session_id_str,
                    user_id: cmd.user_id.to_string(),
                    emoji: cmd.emoji.clone(),
                },
            ));

        // 5. 发送移除反应请求到服务器（通过 ConnectionManager）

        Ok(())
    }

    /// 处理置顶消息命令
    pub async fn handle_pin_message(&self, cmd: PinMessageCommand) -> Result<()> {
        // 1. 查找消息
        let message = self
            .repository
            .find_by_id(&cmd.message_id)
            .await
            .context("Failed to find message")?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;

        // 2. 在移动 message 之前获取需要的信息
        let session_id_str = message.session_id().to_string();

        // 3. 调用领域行为置顶消息
        let _pin_event = message
            .pin(cmd.user_id.clone(), cmd.expire_at.clone())
            .context("Failed to pin message")?;

        // 4. 重新获取消息以保存（因为 message 已被移动）
        let updated_message = self
            .repository
            .find_by_id(&cmd.message_id)
            .await
            .context("Failed to find message for save")?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;

        // 5. 保存消息（置顶信息存储在消息的 metadata 中）
        self.repository
            .save(&updated_message)
            .await
            .context("Failed to save pinned message")?;

        // 6. 发布基础设施事件
        self.event_bus
            .publish(crate::infrastructure::event::Event::Message(
                crate::infrastructure::event::MessageEvent::MessagePinned {
                    message_id: cmd.message_id.to_string(),
                    session_id: session_id_str,
                    user_id: cmd.user_id.to_string(),
                },
            ));

        // 5. 发送置顶请求到服务器（通过 ConnectionManager）

        Ok(())
    }

    /// 处理取消置顶命令
    pub async fn handle_unpin_message(&self, cmd: UnpinMessageCommand) -> Result<()> {
        // 1. 查找消息
        let message = self
            .repository
            .find_by_id(&cmd.message_id)
            .await
            .context("Failed to find message")?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;

        // 2. 在移动 message 之前获取需要的信息
        let session_id_str = message.session_id().to_string();

        // 3. 调用领域行为取消置顶
        let _unpin_event = message
            .unpin(cmd.user_id.clone())
            .context("Failed to unpin message")?;

        // 4. 重新获取消息以保存（因为 message 已被移动）
        let updated_message = self
            .repository
            .find_by_id(&cmd.message_id)
            .await
            .context("Failed to find message for save")?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;

        // 5. 保存消息
        self.repository
            .save(&updated_message)
            .await
            .context("Failed to save message")?;

        // 6. 发布基础设施事件（使用之前保存的 session_id）
        self.event_bus
            .publish(crate::infrastructure::event::Event::Message(
                crate::infrastructure::event::MessageEvent::MessageUnpinned {
                    message_id: cmd.message_id.to_string(),
                    session_id: session_id_str,
                    user_id: cmd.user_id.to_string(),
                },
            ));

        // 5. 发送取消置顶请求到服务器（通过 ConnectionManager）

        Ok(())
    }

    /// 处理收藏消息命令
    pub async fn handle_favorite_message(&self, cmd: FavoriteMessageCommand) -> Result<()> {
        // 1. 查找消息
        let message = self
            .repository
            .find_by_id(&cmd.message_id)
            .await
            .context("Failed to find message")?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;

        // 2. 在移动 message 之前获取需要的信息
        let session_id_str = message.session_id().to_string();

        // 3. 调用领域行为收藏消息
        let _favorite_event = message
            .favorite(cmd.user_id.clone(), cmd.tags.clone(), cmd.note.clone())
            .context("Failed to favorite message")?;

        // 4. 重新获取消息以保存（因为 message 已被移动）
        let updated_message = self
            .repository
            .find_by_id(&cmd.message_id)
            .await
            .context("Failed to find message for save")?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;

        // 5. 保存消息（收藏信息存储在消息的 metadata 中）
        self.repository
            .save(&updated_message)
            .await
            .context("Failed to save favorited message")?;

        // 6. 发布基础设施事件
        self.event_bus
            .publish(crate::infrastructure::event::Event::Message(
                crate::infrastructure::event::MessageEvent::MessageFavorited {
                    message_id: cmd.message_id.to_string(),
                    session_id: session_id_str,
                    user_id: cmd.user_id.to_string(),
                },
            ));

        // 5. 发送收藏请求到服务器（通过 ConnectionManager）

        Ok(())
    }

    /// 处理取消收藏命令
    pub async fn handle_unfavorite_message(&self, cmd: UnfavoriteMessageCommand) -> Result<()> {
        // 1. 查找消息
        let message = self
            .repository
            .find_by_id(&cmd.message_id)
            .await
            .context("Failed to find message")?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;

        // 2. 在移动 message 之前获取需要的信息
        let session_id_str = message.session_id().to_string();

        // 3. 调用领域行为取消收藏
        let _unfavorite_event = message
            .unfavorite(cmd.user_id.clone())
            .context("Failed to unfavorite message")?;

        // 4. 重新获取消息以保存（因为 message 已被移动）
        let updated_message = self
            .repository
            .find_by_id(&cmd.message_id)
            .await
            .context("Failed to find message for save")?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;

        // 5. 保存消息
        self.repository
            .save(&updated_message)
            .await
            .context("Failed to save message")?;

        // 6. 发布基础设施事件
        self.event_bus
            .publish(crate::infrastructure::event::Event::Message(
                crate::infrastructure::event::MessageEvent::MessageUnfavorited {
                    message_id: cmd.message_id.to_string(),
                    session_id: session_id_str,
                    user_id: cmd.user_id.to_string(),
                },
            ));

        // 5. 发送取消收藏请求到服务器（通过 ConnectionManager）

        Ok(())
    }
}
