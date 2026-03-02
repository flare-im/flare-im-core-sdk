//! 网络消息监听器
//!
//! 实现 flare-core 的 MessageListener trait，处理网络层的事件回调
//!
//! # 协议处理
//!
//! 服务器推送的数据可能是：
//! - `ServerPacket`（统一传输包，含 event_envelope / send_ack / sync_resp 等）
//! - 非 ServerPacket 时转发原始 Frame 由应用层解析
//!
//! 根据 `transport.proto`，ServerPacket payload 包括：
//! - `event_envelope` - 事件批（推送消息等）
//! - `send_ack` - 发送回执（由 send_frame_and_wait 处理）
//! - `sync_resp` - 按会话同步响应
//! - `sync_conversations_resp` / `sync_conversations_all_resp` / `get_conversation_detail_resp`
//! - `custom_push` - 自定义推送

use async_trait::async_trait;
use flare_core::client::builder::flare::MessageListener;
use flare_core::common::protocol::Frame;
use flare_core::common::error::Result as FlareResult;
use tokio::sync::mpsc;
use tracing::{error, info, warn, debug};

use super::packet::parse_server_packet;
use super::router::MessageRouter;
use super::types::{ConnectionEvent, NetworkMessage};
use super::parser;

/// 网络消息监听器
///
/// 负责处理来自 flare-core 的网络事件，并将其转发到应用层
pub struct NetworkMessageListener {
    message_tx: mpsc::UnboundedSender<NetworkMessage>,
    connection_tx: mpsc::UnboundedSender<ConnectionEvent>,
}

impl NetworkMessageListener {
    /// 创建新的网络消息监听器
    pub fn new(
        message_tx: mpsc::UnboundedSender<NetworkMessage>,
        connection_tx: mpsc::UnboundedSender<ConnectionEvent>,
    ) -> Self {
        Self {
            message_tx,
            connection_tx,
        }
    }
}

#[async_trait]
impl MessageListener for NetworkMessageListener {
    /// 处理收到的消息 Frame
    ///
    /// # 协议处理流程
    ///
    /// 1. **ACK 消息处理**：ACK 消息（Type::Ack = 1）由 `send_frame_and_wait` 自动处理，
    ///    直接返回 `None`，让 flare-core 的 `ClientCore` 处理 ACK 匹配。
    ///
    /// 2. **ServerPacket 解析**：对于普通消息（Type::Send = 0），先尝试解析为 `ServerPacket`，
    ///    根据不同的 payload 类型进行处理：
    ///    - `event_envelope` / `sync_resp` → 转发 Frame 或 SyncResponse，由处理器解析
    ///    - `custom_push_data` → 转发自定义数据
    ///    - `send_ack` → 由 send_frame_and_wait 处理
    ///    - 其他响应类型 → 转发原始 Frame，由应用层处理
    ///
    /// 3. **向后兼容**：如果不是 ServerPacket，按原有逻辑处理（MessageEnvelope 或 Message）
    async fn on_message(&self, frame: &Frame) -> FlareResult<Option<Frame>> {
        // 检查是否是 ACK 消息
        // ACK 消息由 send_frame_and_wait 自动处理，不需要在这里处理
        if let Some(ref cmd) = frame.command {
            if let Some(ref cmd_type) = cmd.r#type {
                if let flare_core::common::protocol::flare::core::commands::command::Type::Message(msg_cmd) = cmd_type {
                    // MessageCommand Type 枚举值：Send=0, Ack=1, etc.
                    let msg_type = msg_cmd.r#type;
                    
                    // ACK 消息（Type::Ack = 1）由 send_frame_and_wait 自动处理
                    // 直接返回 None，让 flare-core 的 ClientCore 处理 ACK 匹配
                    if msg_type == 1 {
                        debug!(
                            frame_id = %frame.message_id,
                            message_id = %msg_cmd.message_id,
                            "ACK message received, handled by send_frame_and_wait"
                        );
                        return Ok(None);
                    }
                    
                    // 处理普通消息（Type::Send = 0 / Type::Data = 2）
                    // 新链路使用 DATA 承载 ServerPacket
                    if msg_type == 0 || msg_type == 2 {
                        // 先尝试解压 payload（与 parser 一致，避免压缩数据导致解析失败）
                        let payload = parser::ensure_decompressed_payload(msg_cmd.payload.as_slice());
                        match parse_server_packet(&payload) {
                            Ok(packet_result) => {
                                // 使用 MessageRouter 进行路由
                                match MessageRouter::route_packet(packet_result, frame) {
                                    Ok(Some(network_msg)) => {
                                        // 路由成功，转发到应用层
                                        if let Err(e) = self.message_tx.send(network_msg) {
                                            error!("Failed to route message: {}", e);
                                        }
                                    }
                                    Ok(None) => {
                                        // 不需要转发（如 ACK）
                                        debug!(
                                            frame_id = %frame.message_id,
                                            "Message routed but no forwarding needed"
                                        );
                                    }
                                    Err(e) => {
                                        warn!(
                                            frame_id = %frame.message_id,
                                            error = %e,
                                            "Failed to route packet, forwarding raw frame as fallback"
                                        );
                                        // 路由失败，转发原始 Frame（降级处理）
                                        if let Err(e) = self.message_tx.send(NetworkMessage::Received(frame.clone())) {
                                            error!(
                                                frame_id = %frame.message_id,
                                                error = %e,
                                                "Failed to send received message after routing failure"
                                            );
                                        }
                                    }
                                }
                                return Ok(None);
                            }
                            Err(e) => {
                                // 不是 ServerPacket，按向后兼容处理
                                // 转发原始 Frame，由消息处理器解析 MessageEnvelope/Message
                                let payload_len = payload.len();
                                let prefix_hex = payload
                                    .iter()
                                    .take(6)
                                    .map(|b| format!("{:02x}", b))
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                debug!(
                                    frame_id = %frame.message_id,
                                    message_type = msg_type,
                                    payload_len = payload_len,
                                    payload_prefix_hex = %prefix_hex,
                                    parse_error = %e,
                                    "Not a ServerPacket, forwarding raw frame"
                                );
                                if let Err(e) = self.message_tx.send(NetworkMessage::Received(frame.clone())) {
                                    error!("Failed to send received message: {}", e);
                                }
                                return Ok(None);
                            }
                        }
                    }
                }
            }
        }
        
        // 非 MessageCommand 类型的 Frame，转发到消息处理通道
        debug!(
            frame_id = %frame.message_id,
            "Forwarding non-MessageCommand frame to message processor"
        );
        if let Err(e) = self.message_tx.send(NetworkMessage::Received(frame.clone())) {
            error!("Failed to send received frame: {}", e);
        }
        Ok(None)
    }
    
    /// 处理连接建立事件
    async fn on_connect(&self) -> FlareResult<()> {
        info!("Network connection established");
        if let Err(e) = self.connection_tx.send(ConnectionEvent::Connected) {
            error!("Failed to send connection event: {}", e);
        }
        Ok(())
    }
    
    /// 处理连接断开事件
    async fn on_disconnect(&self, reason: Option<&str>) -> FlareResult<()> {
        let reason_str = reason.unwrap_or("Unknown").to_string();
        warn!("Network connection disconnected: {}", reason_str);
        if let Err(e) = self.connection_tx.send(ConnectionEvent::Disconnected) {
            error!("Failed to send disconnection event: {}", e);
        }
        if let Err(e) = self.message_tx.send(NetworkMessage::Disconnected(reason_str)) {
            error!("Failed to send disconnected message: {}", e);
        }
        Ok(())
    }
    
    /// 处理连接错误事件
    async fn on_error(&self, error: &str) -> FlareResult<()> {
        error!("Network connection error: {}", error);
        if let Err(e) = self.connection_tx.send(ConnectionEvent::Error(error.to_string())) {
            error!("Failed to send error event: {}", e);
        }
        if let Err(e) = self.message_tx.send(NetworkMessage::Error(error.to_string())) {
            error!("Failed to send error message: {}", e);
        }
        Ok(())
    }
}
