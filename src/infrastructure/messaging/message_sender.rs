//! 消息发送服务
//!
//! 职责：
//! 1. 消息转换（Domain -> Proto）
//! 2. Frame 构建
//! 3. 网络发送（使用 flare-core 的 send_frame_and_wait）
//! 4. ACK 响应解析

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};
use flare_core::common::protocol::{Frame, Reliability};
use flare_core::common::protocol::builder::*;
use prost::Message as ProstMessage;
use flare_proto::common::SendEnvelopeAck;
use flare_proto::common::AckStatus;

use crate::domain::message::Message;
use crate::infrastructure::network::NetworkClient;
use crate::infrastructure::converter::MessageConverter;

/// 消息发送结果
#[derive(Debug, Clone)]
pub struct SendMessageResult {
    pub message_id: String,
    pub seq: u64,
    pub status: AckStatus,
    pub error_code: i32,
    pub error_message: String,
}

/// 消息发送服务
///
/// 基础设施层服务，负责消息的网络发送和 ACK 处理
pub struct MessageSender {
    network: Arc<Mutex<Option<NetworkClient>>>,
}

impl MessageSender {
    /// 创建新的消息发送服务
    pub fn new(network: Arc<Mutex<Option<NetworkClient>>>) -> Self {
        Self { network }
    }
    
    /// 发送消息并等待 ACK
    ///
    /// # 参数
    /// * `message` - 要发送的消息
    /// * `timeout` - 超时时间
    ///
    /// # 返回
    /// * `Ok(SendMessageResult)` - 发送成功，包含 ACK 信息
    /// * `Err` - 发送失败或超时
    pub async fn send_message_and_wait_ack(
        &self,
        message: &Message,
        timeout: Duration,
    ) -> Result<SendMessageResult> {
        // 1. 转换为 Proto Message
        let proto_message = MessageConverter::to_proto(message)
            .context("Failed to convert message to proto")?;
        
        // 2. 序列化消息
        let mut payload = Vec::new();
        proto_message.encode(&mut payload)
            .context("Failed to encode proto message")?;
        
        // 3. 构建 Frame
        let frame = self.build_message_frame(message, payload)?;

        // 4. 发送并等待响应（使用 flare-core 的 send_frame_and_wait）
        let network_guard = self.network.lock().await;
        let client = network_guard.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Network client is not connected"))?;
        
        if !client.is_connected() {
            return Err(anyhow::anyhow!("Network client is not connected"));
        }
        
        info!(
            message_id = %message.id,
            conversation_id = %message.conversation_id,
            frame_message_id = frame.message_id.clone(),
            "📤 发送消息到服务器"
        );

        // 发送并等待 ACK 响应
        let response_frame = match client.send_frame_and_wait(&frame, timeout).await {
            Ok(frame) => frame,
            Err(e) => {
                error!(
                    message_id = %message.id,
                    error = %e,
                    "发送消息或等待响应失败"
                );
                return Err(anyhow::anyhow!("发送消息或等待响应失败: {}", e));
            }
        };

        debug!(
            message_id = %message.id,
            conversation_id = %message.conversation_id,
            frame_message_id = frame.message_id.clone(),
            response_message_id = response_frame.message_id.clone(),
            "📨 收到服务器响应"
        );

        // 5. 解析 ACK 响应
        let result = self.parse_ack_response(&response_frame, &message.id)?;

        info!(
            message_id = %result.message_id,
            seq = result.seq,
            status = ?result.status,
            "✅ 收到服务器 ACK"
        );

        Ok(result)
    }
    
    /// 构建消息 Frame
    fn build_message_frame(&self, message: &Message, payload: Vec<u8>) -> Result<Frame> {
        // 构建 MessageCommand
        let mut metadata = std::collections::HashMap::new();
        // 添加会话 ID 到 metadata（用于路由）
        metadata.insert("conversation_id".to_string(), message.conversation_id.as_bytes().to_vec());
        
        let msg_cmd = send_message( message.id.clone(), payload, Some(metadata), None);
        // 构建 Frame（使用 AtLeastOnce 可靠性，确保消息不丢失）
        let frame = frame_with_message_command(msg_cmd, Reliability::AtLeastOnce);
        
        Ok(frame)
    }
    
    /// 解析 ACK 响应
    fn parse_ack_response(&self, frame: &Frame, expected_message_id: &str) -> Result<SendMessageResult> {
        // 从 Frame 中提取 MessageCommand
        let msg_cmd = frame.command.as_ref()
            .and_then(|cmd| {
                cmd.r#type.as_ref().and_then(|t| {
                    match t {
                        flare_core::common::protocol::flare::core::commands::command::Type::Message(msg_cmd) => {
                            Some(msg_cmd)
                        }
                        _ => None,
                    }
                })
            })
            .ok_or_else(|| anyhow::anyhow!("Frame does not contain MessageCommand"))?;
        
        // 检查是否是 ACK（Type::Ack = 1）
        if msg_cmd.r#type != 1 {
            return Err(anyhow::anyhow!(
                "Expected ACK response (type=1), got type={}",
                msg_cmd.r#type
            ));
        }
        
        // 解析 SendEnvelopeAck
        // 注意：ACK 可能包装在 ServerPacket 中，但通常直接是 SendEnvelopeAck
        // 先尝试直接解析 SendEnvelopeAck，如果失败再尝试解析 ServerPacket
        let ack = match SendEnvelopeAck::decode(msg_cmd.payload.as_slice()) {
            Ok(ack) => ack,
            Err(_) => {
                // 如果直接解析失败，可能是包装在 ServerPacket 中
                // 但这种情况很少见，先记录警告
                warn!(
                    payload_len = msg_cmd.payload.len(),
                    "Failed to decode SendEnvelopeAck directly, trying alternative format"
                );
                // 直接返回错误，让调用者处理
                return Err(anyhow::anyhow!("Failed to decode SendEnvelopeAck"));
            }
        };
        
        // 验证 message_id 匹配
        if ack.message_id.is_empty() {
            warn!(
                expected = expected_message_id,
                payload_len = msg_cmd.payload.len(),
                "ACK message_id 为空，使用期望的 message_id"
            );
            // 如果 ACK message_id 为空，使用期望的 message_id（可能是服务端 bug）
            // 但继续处理，不返回错误
        } else if ack.message_id != expected_message_id {
            warn!(
                expected = expected_message_id,
                actual = %ack.message_id,
                "ACK message_id 不匹配"
            );
        }
        
        // 转换 AckStatus（使用 TryFrom<i32>）
        use std::convert::TryFrom;
        let status = AckStatus::try_from(ack.status)
            .unwrap_or(AckStatus::Unspecified);
        
        Ok(SendMessageResult {
            message_id: ack.message_id,
            seq: msg_cmd.seq,
            status,
            error_code: ack.error_code,
            error_message: ack.error_message,
        })
    }
}
