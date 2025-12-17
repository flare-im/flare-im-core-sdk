//! 消息重试任务执行器
//!
//! 按照微信/Telegram/飞书标准实现消息自动重试机制

use crate::application::handlers::MessageCommandHandler;
use crate::infrastructure::connection::ConnectionManager;
use crate::infrastructure::event::EventBus;
use crate::infrastructure::protocol::FrameBuilder;
use crate::infrastructure::storage::PendingMessageQueue;
use anyhow::{Context, Result};
use flare_core::common::protocol::{MessageCommand, Reliability};
use flare_proto::Message as ProtoMessage;
use prost::Message as ProstMessage;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;
use tracing::{debug, error, info, warn};

/// 消息重试任务执行器
///
/// 按照微信/Telegram/飞书标准：
/// - 指数退避重试（避免服务器压力）
/// - 最大重试次数限制（避免无限重试）
/// - 优先级队列（重要消息优先重试）
pub struct MessageRetryTask {
    pending_queue: Arc<PendingMessageQueue>,
    message_command_handler: Arc<MessageCommandHandler>,
    connection_manager: Arc<ConnectionManager>,
    event_bus: Arc<EventBus>,
    /// 重试间隔（毫秒）
    retry_interval_ms: u64,
    /// 是否启用
    enabled: Arc<tokio::sync::RwLock<bool>>,
}

impl MessageRetryTask {
    /// 创建新的消息重试任务
    pub fn new(
        pending_queue: Arc<PendingMessageQueue>,
        message_command_handler: Arc<MessageCommandHandler>,
        connection_manager: Arc<ConnectionManager>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            pending_queue,
            message_command_handler,
            connection_manager,
            event_bus,
            retry_interval_ms: 1000, // 默认 1 秒
            enabled: Arc::new(tokio::sync::RwLock::new(true)),
        }
    }

    /// 设置重试间隔
    pub fn with_retry_interval(mut self, interval_ms: u64) -> Self {
        self.retry_interval_ms = interval_ms;
        self
    }

    /// 启动重试任务（后台运行）
    pub fn start(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(self.retry_interval_ms));

            loop {
                interval.tick().await;

                // 检查是否启用
                if !*self.enabled.read().await {
                    continue;
                }

                // 检查连接状态
                let connection_state = self.connection_manager.state().await;
                if !matches!(
                    connection_state,
                    crate::infrastructure::connection::ConnectionState::Authenticated
                ) {
                    // 未连接，跳过重试
                    continue;
                }

                // 从队列中获取待重试的消息
                match self.pending_queue.dequeue().await {
                    Ok(Some(pending_msg)) => {
                        let message_id = pending_msg.message_id.clone();
                        let session_id = pending_msg.session_id.clone();

                        debug!(
                            message_id = %message_id,
                            session_id = %session_id,
                            retry_count = pending_msg.retry_count,
                            "Retrying message"
                        );

                        // 反序列化消息内容
                        match pending_msg.deserialize_content() {
                            Ok(content) => {
                                // 构建完整的 ProtoMessage
                                let proto_message = ProtoMessage {
                                    id: message_id.clone(),
                                    session_id: session_id.clone(),
                                    content: Some(content),
                                    ..Default::default()
                                };

                                // 尝试重新发送
                                match self.retry_send_message(&proto_message, &message_id).await {
                                    Ok(()) => {
                                        // 发送成功，标记为完成
                                        if let Err(e) =
                                            self.pending_queue.mark_completed(&message_id).await
                                        {
                                            warn!(error = %e, message_id = %message_id, "Failed to mark message as completed");
                                        } else {
                                            info!(message_id = %message_id, "Message retry succeeded");
                                        }
                                    }
                                    Err(e) => {
                                        // 发送失败，标记为失败并重试
                                        warn!(
                                            error = %e,
                                            message_id = %message_id,
                                            retry_count = pending_msg.retry_count,
                                            "Message retry failed"
                                        );

                                        if let Err(mark_err) = self
                                            .pending_queue
                                            .mark_failed_and_retry(&message_id, e.to_string())
                                            .await
                                        {
                                            error!(error = %mark_err, message_id = %message_id, "Failed to mark message as failed");
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!(
                                    error = %e,
                                    message_id = %message_id,
                                    "Failed to deserialize message content"
                                );
                                // 反序列化失败，标记为失败
                                let _ = self
                                    .pending_queue
                                    .mark_failed_and_retry(
                                        &message_id,
                                        format!("Deserialize error: {}", e),
                                    )
                                    .await;
                            }
                        }
                    }
                    Ok(None) => {
                        // 没有待重试的消息
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to dequeue pending message");
                    }
                }
            }
        })
    }

    /// 重试发送消息
    async fn retry_send_message(
        &self,
        proto_message: &ProtoMessage,
        message_id: &str,
    ) -> Result<()> {
        let mut payload = Vec::new();
        proto_message
            .encode(&mut payload)
            .context("Failed to encode message")?;

        let msg_cmd = MessageCommand {
            message_id: message_id.to_string(),
            r#type: flare_core::common::protocol::flare::core::commands::message_command::Type::Send
                as i32,
            payload,
            metadata: std::collections::HashMap::new(),
            seq: 0,
        };

        let frame = FrameBuilder::new()
            .with_message_command(msg_cmd)
            .with_reliability(Reliability::AtLeastOnce)
            .build();

        self.connection_manager
            .send_frame(&frame)
            .await
            .context("Failed to send retry frame")
    }

    /// 停止重试任务
    pub async fn stop(&self) {
        *self.enabled.write().await = false;
    }

    /// 启用重试任务
    pub async fn enable(&self) {
        *self.enabled.write().await = true;
    }
}
