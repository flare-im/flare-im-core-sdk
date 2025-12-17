//! 消息处理器
//!
//! 处理从连接层收到的 Frame，解析并分发给 MessageCommandHandler

use crate::application::crypto::CryptoService;
use crate::application::handlers::MessageCommandHandler;
use crate::application::receivers::MessageReceiver;
use crate::infrastructure::event::EventBus;
use anyhow::{Context, Result};
use flare_core::common::protocol::Frame;
use flare_core::common::protocol::flare::core::commands::message_command::Type as MessageCommandType;
use prost::Message as ProstMessage;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// 消息帧处理器
///
/// 处理从连接层收到的 Frame，解析为 Message 并分发给 MessageReceiver
pub struct MessageFrameHandler {
    /// 消息接收器（处理服务端推送的消息）
    message_receiver: Arc<MessageReceiver>,

    /// 消息命令处理器（用于处理 ACK 等）
    message_command_handler: Arc<MessageCommandHandler>,

    /// 加密服务
    crypto: Arc<RwLock<Arc<dyn CryptoService>>>,

    /// 事件总线
    event_bus: Arc<EventBus>,

    /// 连接管理器（用于更新连接状态）
    connection_manager: Option<Arc<crate::infrastructure::connection::ConnectionManager>>,
}

impl MessageFrameHandler {
    /// 创建新的消息帧处理器
    pub fn new(
        message_receiver: Arc<MessageReceiver>,
        message_command_handler: Arc<MessageCommandHandler>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            message_receiver,
            message_command_handler,
            crypto: Arc::new(RwLock::new(Arc::new(
                crate::application::crypto::NoopCrypto,
            ))),
            event_bus,
            connection_manager: None,
        }
    }

    /// 设置加密服务
    pub async fn with_crypto(mut self, crypto: Arc<dyn CryptoService>) -> Self {
        *self.crypto.write().await = crypto;
        self
    }

    /// 设置连接管理器（用于更新连接状态）
    pub fn with_connection_manager(
        mut self,
        connection_manager: Arc<crate::infrastructure::connection::ConnectionManager>,
    ) -> Self {
        self.connection_manager = Some(connection_manager);
        self
    }

    /// 获取消息命令处理器（用于访问 repository）
    pub fn message_command_handler(&self) -> &Arc<MessageCommandHandler> {
        &self.message_command_handler
    }

    /// 处理收到的 Frame
    ///
    /// # 参数
    /// - `frame`: 收到的 Frame
    pub async fn handle_frame(&self, frame: Frame) -> Result<()> {
        debug!("Handling incoming frame");

        // 1. 检查 Frame 类型
        let command = frame
            .command
            .as_ref()
            .and_then(|cmd| cmd.r#type.as_ref())
            .context("Frame has no command")?;

        // 2. 处理不同类型的命令
        match command {
            flare_core::common::protocol::flare::core::commands::command::Type::Message(
                msg_cmd,
            ) => {
                // 消息命令
                self.handle_message_command(msg_cmd, &frame).await
            }
            flare_core::common::protocol::flare::core::commands::command::Type::System(sys_cmd) => {
                // 系统命令（ACK、错误等）
                self.handle_system_command(sys_cmd, &frame).await
            }
            flare_core::common::protocol::flare::core::commands::command::Type::Custom(
                custom_cmd,
            ) => {
                // 自定义命令（同步响应等）
                self.handle_custom_command(custom_cmd, &frame).await
            }
            flare_core::common::protocol::flare::core::commands::command::Type::Notification(
                notif_cmd,
            ) => {
                // 通知命令
                self.handle_notification_command(notif_cmd, &frame).await
            }
        }
    }

    /// 处理消息命令
    async fn handle_message_command(
        &self,
        msg_cmd: &flare_core::common::protocol::MessageCommand,
        _frame: &Frame,
    ) -> Result<()> {
        debug!(message_id = %msg_cmd.message_id, r#type = msg_cmd.r#type, "Handling message command");
        match MessageCommandType::try_from(msg_cmd.r#type).unwrap_or(MessageCommandType::Send) {
            MessageCommandType::Send | MessageCommandType::Data => {
                // 1. 解密负载
                let crypto = self.crypto.read().await.clone();
                let decrypted = crypto
                    .decrypt(&msg_cmd.payload[..])
                    .context("Failed to decrypt message payload")?;

                // 2. 尝试解析为不同的格式（按照 transport.proto 规范）
                // 服务端可能发送：
                // - MessageEnvelope（推送/同步封装）
                // - ServerPacket（统一传输包）
                // - Message（直接消息，兼容旧格式）

                // 2.1 先尝试解析为 ServerPacket
                if let Ok(server_packet) =
                    flare_proto::common::ServerPacket::decode(decrypted.as_slice())
                {
                    debug!("Received ServerPacket, processing envelope");
                    match server_packet.payload {
                        Some(flare_proto::common::server_packet::Payload::Envelope(envelope)) => {
                            // 处理 MessageEnvelope
                            for message in envelope.messages {
                                let domain_message = crate::domain::message::Message::from_proto(
                                    message,
                                )
                                .context("Failed to convert proto message to domain message")?;

                                self.message_receiver
                                    .receive(domain_message)
                                    .await
                                    .context("Failed to handle received message")?;
                            }
                        }
                        Some(flare_proto::common::server_packet::Payload::SendAck(ack)) => {
                            // 处理发送 ACK
                            debug!(
                                message_id = %ack.message_id,
                                status = %ack.status,
                                "Received SendEnvelopeAck"
                            );
                            // 通过 MessageCommandHandler 处理 ACK
                            if let Err(e) = self
                                .message_command_handler
                                .handle_ack(
                                    &crate::domain::MessageId::new(ack.message_id.clone()),
                                    &format!("send_ack:{}", ack.status),
                                )
                                .await
                            {
                                error!(error = %e, "Failed to handle send ACK");
                            }
                        }
                        Some(flare_proto::common::server_packet::Payload::SyncMessagesResp(
                            sync_resp,
                        )) => {
                            // 处理消息同步响应（在 SyncCommandHandler 中处理）
                            debug!(
                                "Received SyncMessagesResponse, should be handled by SyncCommandHandler"
                            );
                            // 这里不处理，由 CustomCommand 处理器处理
                        }
                        Some(flare_proto::common::server_packet::Payload::SyncSessionsResp(_)) => {
                            debug!(
                                "Received SyncSessionsResponse, should be handled by SyncCommandHandler"
                            );
                        }
                        Some(flare_proto::common::server_packet::Payload::SyncSessionsAllResp(
                            _,
                        )) => {
                            debug!(
                                "Received SessionSyncAllResponse, should be handled by SyncCommandHandler"
                            );
                        }
                        Some(
                            flare_proto::common::server_packet::Payload::GetSessionDetailResp(_),
                        ) => {
                            debug!("Received GetSessionDetailResponse");
                        }
                        Some(flare_proto::common::server_packet::Payload::CustomPushData(_)) => {
                            debug!("Received CustomPushData");
                        }
                        None => {
                            warn!("ServerPacket has no payload");
                        }
                    }
                }
                // 2.2 尝试解析为 MessageEnvelope（直接格式）
                else if let Ok(envelope) =
                    flare_proto::common::MessageEnvelope::decode(decrypted.as_slice())
                {
                    debug!("Received MessageEnvelope directly");
                    for message in envelope.messages {
                        let domain_message =
                            crate::domain::message::Message::from_proto(message)
                                .context("Failed to convert proto message to domain message")?;

                        self.message_receiver
                            .receive(domain_message)
                            .await
                            .context("Failed to handle received message")?;
                    }
                }
                // 2.3 尝试解析为直接 Message（兼容旧格式）
                else if let Ok(proto_message) = flare_proto::Message::decode(decrypted.as_slice())
                {
                    debug!("Received Message directly (legacy format)");
                    let domain_message = crate::domain::message::Message::from_proto(proto_message)
                        .context("Failed to convert proto message to domain message")?;

                    self.message_receiver
                        .receive(domain_message)
                        .await
                        .context("Failed to handle received message")?;
                }
                // 2.4 所有格式都解析失败
                else {
                    // 尝试解析为 ServerPacket 以获取详细错误信息
                    let server_packet_err =
                        flare_proto::common::ServerPacket::decode(decrypted.as_slice())
                            .err()
                            .map(|e| format!("ServerPacket decode error: {}", e))
                            .unwrap_or_else(|| "Not ServerPacket".to_string());

                    let envelope_err =
                        flare_proto::common::MessageEnvelope::decode(decrypted.as_slice())
                            .err()
                            .map(|e| format!("MessageEnvelope decode error: {}", e))
                            .unwrap_or_else(|| "Not MessageEnvelope".to_string());

                    let message_err = flare_proto::Message::decode(decrypted.as_slice())
                        .err()
                        .map(|e| format!("Message decode error: {}", e))
                        .unwrap_or_else(|| "Not Message".to_string());

                    error!(
                        message_id = %msg_cmd.message_id,
                        payload_len = decrypted.len(),
                        server_packet_err = %server_packet_err,
                        envelope_err = %envelope_err,
                        message_err = %message_err,
                        "Failed to decode message payload in any format"
                    );

                    // 输出 payload 的前 100 字节用于调试
                    let preview = if decrypted.len() > 100 {
                        format!("{:?}...", &decrypted[..100])
                    } else {
                        format!("{:?}", decrypted)
                    };
                    debug!("Payload preview: {}", preview);

                    return Err(anyhow::anyhow!(
                        "Failed to decode message payload: not ServerPacket, MessageEnvelope, or Message. Payload length: {}. Errors: ServerPacket={}, MessageEnvelope={}, Message={}",
                        decrypted.len(),
                        server_packet_err,
                        envelope_err,
                        message_err
                    ));
                }
            }
            MessageCommandType::Ack => {
                // 处理 ACK（按照微信/Telegram/飞书标准）
                // 根据 metadata 判断 ACK 类型
                let ack_type = msg_cmd
                    .metadata
                    .get("ack_type")
                    .and_then(|v| String::from_utf8(v.clone()).ok())
                    .unwrap_or_else(|| "transport".to_string());

                debug!(
                    message_id = %msg_cmd.message_id,
                    ack_type = %ack_type,
                    "Received ACK"
                );

                // 通过 MessageCommandHandler 处理 ACK
                if let Err(e) = self
                    .message_command_handler
                    .handle_ack(
                        &crate::domain::MessageId::new(msg_cmd.message_id.clone()),
                        &ack_type,
                    )
                    .await
                {
                    error!(
                        error = %e,
                        message_id = %msg_cmd.message_id,
                        ack_type = %ack_type,
                        "Failed to handle ACK"
                    );
                }
            }
        }
        Ok(())
    }

    /// 处理系统命令
    async fn handle_system_command(
        &self,
        sys_cmd: &flare_core::common::protocol::SystemCommand,
        _frame: &Frame,
    ) -> Result<()> {
        use flare_core::common::protocol::flare::core::commands::system_command::Type as SystemCommandType;

        match sys_cmd.r#type {
            x if x == SystemCommandType::ConnectAck as i32 => {
                // 收到 CONNECT_ACK，表示认证成功
                info!("✅ 收到 CONNECT_ACK，认证成功");

                // 关键修复：更新 ConnectionManager 的状态为 Authenticated
                if let Some(ref connection_manager) = self.connection_manager {
                    if let Err(e) = connection_manager
                        .set_state(
                            crate::infrastructure::connection::StateTransition::Authenticated,
                        )
                        .await
                    {
                        warn!(error = %e, "Failed to transition to Authenticated state");
                    } else {
                        debug!("ConnectionManager state updated to Authenticated");
                    }
                }

                // 发布 Authenticated 事件（确保事件被发布）
                self.event_bus
                    .publish(crate::infrastructure::event::Event::Connection(
                        crate::infrastructure::event::ConnectionEvent::Authenticated,
                    ));
                debug!("Authenticated event published to event bus");
            }
            x if x == SystemCommandType::Pong as i32 => {
                debug!("Received PONG");
                // 心跳响应，无需处理
            }
            x if x == SystemCommandType::Error as i32 => {
                error!(message = %sys_cmd.message, "Received error from server");
                self.event_bus
                    .publish(crate::infrastructure::event::Event::Connection(
                        crate::infrastructure::event::ConnectionEvent::Error(
                            sys_cmd.message.clone(),
                        ),
                    ));
                if let Some(code_bytes) = sys_cmd.metadata.get("code") {
                    if let Ok(code_str) = String::from_utf8(code_bytes.clone()) {
                        if let Ok(code) = code_str.parse::<i32>() {
                            self.event_bus.publish(
                                crate::infrastructure::event::Event::Connection(
                                    crate::infrastructure::event::ConnectionEvent::ErrorWithCode {
                                        code,
                                        message: sys_cmd.message.clone(),
                                    },
                                ),
                            );
                        }
                    }
                }
            }
            x if x == SystemCommandType::Event as i32 => {
                debug!(
                    message = %sys_cmd.message,
                    "Received system event"
                );

                // 处理系统事件（如消息撤回通知等）
                if sys_cmd.message == "recall" {
                    // 消息撤回通知
                    if let Some(message_id) = String::from_utf8(sys_cmd.data.clone()).ok() {
                        // 从 metadata 中提取 session_id
                        let session_id = sys_cmd
                            .metadata
                            .get("session_id")
                            .and_then(|v| String::from_utf8(v.clone()).ok())
                            .unwrap_or_default();

                        // 发布消息撤回事件（MessageService 会处理本地状态更新）
                        self.event_bus
                            .publish(crate::infrastructure::event::Event::Message(
                                crate::infrastructure::event::MessageEvent::MessageRecalled {
                                    message_id,
                                    session_id,
                                },
                            ));
                        info!("Message recalled event published");
                    }
                }
                if sys_cmd.message == "read" {
                    // 处理已读回执（按照微信/Telegram/飞书标准）
                    // 已读回执通过系统事件通知，更新消息状态为已读
                    if let Some(message_id) = String::from_utf8(sys_cmd.data.clone()).ok() {
                        let session_id = sys_cmd
                            .metadata
                            .get("session_id")
                            .and_then(|v| String::from_utf8(v.clone()).ok())
                            .unwrap_or_default();
                        let user_id = sys_cmd
                            .metadata
                            .get("user_id")
                            .and_then(|v| String::from_utf8(v.clone()).ok())
                            .unwrap_or_default();

                        debug!(
                            message_id = %message_id,
                            session_id = %session_id,
                            user_id = %user_id,
                            "Received read receipt"
                        );

                        // 发布已读事件
                        self.event_bus
                            .publish(crate::infrastructure::event::Event::Message(
                                crate::infrastructure::event::MessageEvent::MessageRead {
                                    message_id,
                                    session_id,
                                    user_id,
                                },
                            ));
                    }
                }
            }
            _ => {
                debug!("Received system command: {}", sys_cmd.r#type);
            }
        }

        Ok(())
    }

    /// 处理自定义命令
    async fn handle_custom_command(
        &self,
        custom_cmd: &flare_core::common::protocol::CustomCommand,
        _frame: &Frame,
    ) -> Result<()> {
        debug!(
            name = %custom_cmd.name,
            "Handling custom command"
        );

        // 自定义命令可能是同步响应等
        // 注意：自定义命令的响应处理已经在 FlareIMClient 的消息接收监听器中完成
        // 这里主要是处理非同步响应的自定义命令（如通知等）
        // 同步响应（SessionBootstrap、SyncMessages、ListSessions）由 SyncService 处理

        Ok(())
    }

    /// 处理通知命令
    async fn handle_notification_command(
        &self,
        notif_cmd: &flare_core::common::protocol::NotificationCommand,
        _frame: &Frame,
    ) -> Result<()> {
        debug!("Handling notification command");

        // 处理通知命令（如用户状态变化、会话更新等）
        // 注意：具体的通知处理逻辑可以根据业务需求扩展
        // 这里暂时只记录日志，后续可以根据通知类型分发到不同的处理器

        info!(
            notification_type = notif_cmd.r#type,
            "Received notification command"
        );

        Ok(())
    }
}
