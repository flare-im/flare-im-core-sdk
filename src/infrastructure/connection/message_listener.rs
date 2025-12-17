//! SDK 消息监听器
//!
//! 实现 FlareClientBuilder 的 MessageListener trait，处理来自服务器的消息
//!
//! 参照 flare_chat_client.rs 的 ChatListener 实现，提供更好的事件处理

use crate::application::handlers::SyncCommandHandler;
use crate::infrastructure::event::{ConnectionEvent, Event, EventBus};
use crate::infrastructure::handler::MessageFrameHandler;
use async_trait::async_trait;
use flare_core::client::builder::flare::MessageListener;
use flare_core::common::error::Result;
use flare_core::common::protocol::Frame;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// SDK 消息监听器
///
/// 将收到的消息分发给 MessageFrameHandler 和 SyncService
/// 参照 flare_chat_client.rs 的 ChatListener 实现
pub struct SDKMessageListener {
    /// 消息帧处理器
    message_handler: Arc<MessageFrameHandler>,

    /// 同步命令处理器（用于处理自定义命令响应）
    sync_command_handler: Arc<SyncCommandHandler>,

    /// 事件总线（用于发布连接事件）
    event_bus: Arc<EventBus>,
}

impl SDKMessageListener {
    /// 创建新的 SDK 消息监听器
    pub fn new(
        message_handler: Arc<MessageFrameHandler>,
        sync_command_handler: Arc<SyncCommandHandler>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            message_handler,
            sync_command_handler,
            event_bus,
        }
    }
}

#[async_trait]
impl MessageListener for SDKMessageListener {
    async fn on_message(&self, frame: &Frame) -> Result<Option<Frame>> {
        debug!(
            message_id = %frame.message_id,
            "Message received"
        );

        // 检查是否是自定义命令或带有 request_id 的系统响应
        let is_custom_command = frame
            .command
            .as_ref()
            .and_then(|c| c.r#type.as_ref())
            .map(|t| {
                matches!(
                    t,
                    flare_core::common::protocol::flare::core::commands::command::Type::Custom(_)
                )
            })
            .unwrap_or(false);

        let has_request_id_meta = frame.metadata.contains_key("request_id");

        if is_custom_command || has_request_id_meta {
            // 自定义命令或带有 request_id 的系统响应，交由 SyncCommandHandler 完成请求
            if let Err(e) = self
                .sync_command_handler
                .handle_response(frame.clone())
                .await
            {
                tracing::error!(
                    error = %e,
                    message_id = %frame.message_id,
                    "Failed to handle sync response"
                );
            }
            tracing::debug!(
                "Custom command or response frame received, handling via sync command handler"
            );
        } else {
            // 其他命令由 MessageFrameHandler 处理
            if let Err(e) = self.message_handler.handle_frame(frame.clone()).await {
                tracing::error!(
                    error = %e,
                    message_id = %frame.message_id,
                    "Failed to handle frame"
                );
            }
        }

        Ok(None)
    }

    async fn on_connect(&self) -> Result<()> {
        info!("SDK connected to server");
        debug!("Client automatically sent CONNECT message with negotiation info");

        // 注意：不在这里发布 Authenticated 事件
        // Authenticated 事件应该在收到 CONNECT_ACK 后由 MessageFrameHandler 发布
        // 这里只发布 Connected 事件，表示连接已建立但还未认证
        self.event_bus
            .publish(Event::Connection(ConnectionEvent::Connected {
                protocol: None,
            }));

        Ok(())
    }

    async fn on_disconnect(&self, reason: Option<&str>) -> Result<()> {
        if let Some(reason) = reason {
            // 判断是否是设备冲突导致的断开（参照 flare_chat_client.rs）
            if reason.contains("设备冲突")
                || reason.contains("被踢")
                || reason.contains("device conflict")
            {
                tracing::error!(
                    reason = %reason,
                    "Connection kicked due to device conflict"
                );
                info!(
                    "Tip: Only one device per user per platform can be online. Please close the current client or use a different platform"
                );

                // 发布 Kicked 事件，重连逻辑会检查此事件并停止重连
                self.event_bus
                    .publish(Event::Connection(ConnectionEvent::Kicked {
                        reason: reason.to_string(),
                    }));
                return Ok(());
            } else {
                info!(
                    reason = %reason,
                    "Connection disconnected"
                );
            }
        } else {
            info!("Connection disconnected");
        }

        // 发布断开事件到事件总线（非设备冲突的情况）
        self.event_bus
            .publish(Event::Connection(ConnectionEvent::Disconnected));

        Ok(())
    }

    async fn on_error(&self, error: &str) -> Result<()> {
        // 判断错误类型，给出更友好的提示（参照 flare_chat_client.rs）
        if error.contains("connection lost") || error.contains("connection closed") {
            warn!(
                error = %error,
                "Connection lost, may be network issue or server closed connection"
            );
            info!("   如果启用了自动重连，客户端会尝试重新连接");
        } else if error.contains("timeout") {
            warn!(
                error = %error,
                "Connection timeout, please check network or server response time"
            );
        } else {
            error!(
                error = %error,
                "Connection error"
            );
        }

        // 发布错误事件到事件总线
        self.event_bus
            .publish(Event::Connection(ConnectionEvent::Error(error.to_string())));

        Ok(())
    }
}
