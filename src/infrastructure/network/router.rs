//! 消息路由器
//!
//! 负责根据协议解析结果，将消息分发到正确的处理通道
//!
//! # 设计原则
//!
//! 1. **无状态设计**：路由器不保存状态，纯函数式，易于测试和并发
//! 2. **路由表模式**：使用模式匹配实现路由表，清晰的路由规则
//! 3. **降级策略**：未知类型自动降级，转发原始 Frame
//!
//! # 路由策略
//!
//! - **事件批** (event_envelope) → 转发原始 Frame，由消息处理器解析
//! - **同步响应** (sync_resp) → 转发 SyncResponse，由 SyncHandler 从 envelope 抽消息
//! - **自定义数据** (custom_push) → 转发结构化数据
//! - **ACK** → 不处理（已由 send_frame_and_wait 处理）

use flare_core::common::protocol::Frame;
use tracing::{debug, warn};

use super::types::NetworkMessage;
use super::packet::PacketParseResult;

/// 消息路由器
///
/// 根据协议解析结果，将消息分发到正确的处理通道
pub struct MessageRouter;

impl MessageRouter {
    /// 路由 ServerPacket 解析结果
    pub fn route_packet(
        packet_result: PacketParseResult,
        frame: &Frame,
    ) -> anyhow::Result<Option<NetworkMessage>> {
        match packet_result {
            // 事件批：直接传递 EventEnvelope，由事件流处理器统一处理（避免重复解析）
            PacketParseResult::EventEnvelope(env) => {
                debug!(
                    frame_id = %frame.message_id,
                    event_count = env.events.len(),
                    "Routing EventEnvelope to event stream"
                );
                Ok(Some(NetworkMessage::EventEnvelope(env)))
            }

            // 同步响应：转发 SyncResponse，由 SyncHandler 从 envelope.events 抽消息
            PacketParseResult::SyncResponse(sync_resp) => {
                debug!(
                    frame_id = %frame.message_id,
                    has_envelope = sync_resp.envelope.is_some(),
                    "Routing SyncResponse to sync handler"
                );
                Ok(Some(NetworkMessage::SyncMessages(sync_resp)))
            }

            // 会话增量同步响应：转发结构化数据
            PacketParseResult::SyncConversationsResponse(resp) => {
                debug!(
                    frame_id = %frame.message_id,
                    patch_count = resp.patches.len(),
                    has_more = resp.has_more,
                    "Routing SyncConversationsResponse to conversation sync handler"
                );
                Ok(Some(NetworkMessage::SyncConversations(resp)))
            }
            
            // 全量会话同步响应：转发结构化数据
            PacketParseResult::ConversationSyncAllResponse(resp) => {
                debug!(
                    frame_id = %frame.message_id,
                    conversation_count = resp.conversations.len(),
                    "Routing ConversationSyncAllResponse to conversation sync handler"
                );
                Ok(Some(NetworkMessage::ConversationSyncAll(resp)))
            }
            
            // 会话详情响应：转发结构化数据
            PacketParseResult::GetConversationDetailResponse(resp) => {
                debug!(
                    frame_id = %frame.message_id,
                    has_detail = resp.detail.is_some(),
                    "Routing GetConversationDetailResponse to conversation handler"
                );
                Ok(Some(NetworkMessage::ConversationDetail(resp)))
            }
            
            // 自定义推送数据：转发结构化数据
            PacketParseResult::CustomPush(custom) => {
                debug!(
                    frame_id = %frame.message_id,
                    data_type = %custom.r#type,
                    payload_len = custom.payload.len(),
                    "Routing CustomPush to custom data handler"
                );
                Ok(Some(NetworkMessage::CustomPushData {
                    data_type: custom.r#type,
                    payload: custom.payload,
                    metadata: custom.metadata,
                }))
            }

            // ACK / OperationAck：不处理（已由 send_frame_and_wait 处理）
            PacketParseResult::SendAck(_) => {
                debug!(
                    frame_id = %frame.message_id,
                    "SendAck handled by send_frame_and_wait, skipping routing"
                );
                Ok(None)
            }
            PacketParseResult::OperationAck(_) => {
                debug!(frame_id = %frame.message_id, "OperationAck handled by send_frame_and_wait, skipping");
                Ok(None)
            }

            PacketParseResult::Error(err) => Ok(Some(NetworkMessage::Error(format!(
                "server_error(code={}): {}",
                err.code, err.message
            )))),
            
            // 未知类型：转发原始 Frame，由应用层决定如何处理
            PacketParseResult::Unknown => {
                warn!(
                    frame_id = %frame.message_id,
                    "Unknown ServerPacket payload type, forwarding raw frame"
                );
                Ok(Some(NetworkMessage::Received(frame.clone())))
            }
        }
    }
}
