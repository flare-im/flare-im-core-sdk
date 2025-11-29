//! 消息处理器
//!
//! 处理从连接层收到的 Frame，解析并分发给 MessageService

use crate::event::EventBus;
use prost::Message as ProstMessage;
use crate::service::MessageService;
use anyhow::{Context, Result};
use flare_core::common::protocol::Frame;
use flare_core::common::protocol::flare::core::commands::message_command::Type as MessageCommandType;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// 消息帧处理器
/// 
/// 处理从连接层收到的 Frame，解析为 Message 并分发给 MessageService
pub struct MessageFrameHandler {
    /// 消息服务
    message_service: Arc<MessageService>,
    
    /// 事件总线
    event_bus: Arc<EventBus>,
    
    /// 连接管理器（用于更新连接状态）
    connection_manager: Option<Arc<crate::connection::ConnectionManager>>,
}

impl MessageFrameHandler {
    /// 创建新的消息帧处理器
    pub fn new(
        message_service: Arc<MessageService>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            message_service,
            event_bus,
            connection_manager: None,
        }
    }
    
    /// 设置连接管理器（用于更新连接状态）
    pub fn with_connection_manager(mut self, connection_manager: Arc<crate::connection::ConnectionManager>) -> Self {
        self.connection_manager = Some(connection_manager);
        self
    }

    /// 处理收到的 Frame
    /// 
    /// # 参数
    /// - `frame`: 收到的 Frame
    pub async fn handle_frame(&self, frame: Frame) -> Result<()> {
        debug!("Handling incoming frame");
        
        // 1. 检查 Frame 类型
        let command = frame.command.as_ref()
            .and_then(|cmd| cmd.r#type.as_ref())
            .context("Frame has no command")?;
        
        // 2. 处理不同类型的命令
        match command {
            flare_core::common::protocol::flare::core::commands::command::Type::Message(msg_cmd) => {
                // 消息命令
                self.handle_message_command(msg_cmd, &frame).await
            }
            flare_core::common::protocol::flare::core::commands::command::Type::System(sys_cmd) => {
                // 系统命令（ACK、错误等）
                self.handle_system_command(sys_cmd, &frame).await
            }
            flare_core::common::protocol::flare::core::commands::command::Type::Custom(custom_cmd) => {
                // 自定义命令（同步响应等）
                self.handle_custom_command(custom_cmd, &frame).await
            }
            flare_core::common::protocol::flare::core::commands::command::Type::Notification(notif_cmd) => {
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
                // 优化：并行执行解密和后续处理（如果可能）
                let decrypted = self.message_service.decrypt_payload(&msg_cmd.payload[..]).await?;
                // 优化：使用 decode 的引用版本，避免不必要的克隆
                let message = flare_proto::Message::decode(decrypted.as_slice())
                    .context("Failed to decode message")?;
                self.message_service.on_message_received(message).await
                    .context("Failed to handle received message")?;
            }
            MessageCommandType::Ack => {
                // 根据metadata判断送达级别；默认更新为Sent
                use flare_proto::MessageStatus;
                let status = if let Some(delivered_flag) = msg_cmd.metadata.get("delivered") {
                    if let Ok(s) = String::from_utf8(delivered_flag.clone()) { if s == "1" { MessageStatus::Delivered } else { MessageStatus::Sent } } else { MessageStatus::Sent }
                } else { MessageStatus::Sent };
                self.message_service.update_message_status(&msg_cmd.message_id, status).await?;
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
                    if let Err(e) = connection_manager.set_state(crate::connection::StateTransition::Authenticated).await {
                        warn!(error = %e, "Failed to transition to Authenticated state");
                    } else {
                        debug!("ConnectionManager state updated to Authenticated");
                    }
                }
                
                // 发布 Authenticated 事件（确保事件被发布）
                self.event_bus.publish(crate::event::Event::Connection(
                    crate::event::ConnectionEvent::Authenticated
                ));
                debug!("Authenticated event published to event bus");
            }
            x if x == SystemCommandType::Pong as i32 => {
                debug!("Received PONG");
                // 心跳响应，无需处理
            }
            x if x == SystemCommandType::Error as i32 => {
                error!(message = %sys_cmd.message, "Received error from server");
                self.event_bus.publish(crate::event::Event::Connection(
                    crate::event::ConnectionEvent::Error(sys_cmd.message.clone())
                ));
                if let Some(code_bytes) = sys_cmd.metadata.get("code") {
                    if let Ok(code_str) = String::from_utf8(code_bytes.clone()) {
                        if let Ok(code) = code_str.parse::<i32>() {
                            self.event_bus.publish(crate::event::Event::Connection(
                                crate::event::ConnectionEvent::ErrorWithCode { code, message: sys_cmd.message.clone() }
                            ));
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
                        let session_id = sys_cmd.metadata
                            .get("session_id")
                            .and_then(|v| String::from_utf8(v.clone()).ok())
                            .unwrap_or_default();
                        
                        // 发布消息撤回事件（MessageService 会处理本地状态更新）
                        self.event_bus.publish(crate::event::Event::Message(
                            crate::event::MessageEvent::MessageRecalled {
                                message_id,
                                session_id,
                            }
                        ));
                        info!("Message recalled event published");
                    }
                }
                if sys_cmd.message == "read" {
                    if let Some(message_id) = String::from_utf8(sys_cmd.data.clone()).ok() {
                        let session_id = sys_cmd.metadata.get("session_id").and_then(|v| String::from_utf8(v.clone()).ok()).unwrap_or_default();
                        let state = crate::storage::MessageState::new().mark_as_read();
                        let user = sys_cmd.metadata.get("user_id").and_then(|v| String::from_utf8(v.clone()).ok()).unwrap_or_default();
                        if !user.is_empty() {
                            if let Err(e) = self.message_service.save_read_state(&user, &message_id, state).await {
                                tracing::error!(error = %e, "Failed to apply read receipt");
                            }
                        }
                        self.event_bus.publish(crate::event::Event::Message(crate::event::MessageEvent::MessageReceived { message_id, session_id }));
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
