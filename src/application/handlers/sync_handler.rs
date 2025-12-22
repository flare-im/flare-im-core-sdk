//! 同步消息处理器
//!
//! 负责处理消息同步响应（SyncMessagesResponse）
//!
//! # 处理流程
//!
//! 1. 提取 MessageEnvelope（可能包含多条消息）
//! 2. 批量解析消息（避免逐条解析）
//! 3. 批量入队（使用批量接口）
//! 4. 更新同步游标（cursor / max_seq）
//! 5. 判断 has_more，触发后续同步
//! 6. 发布同步完成事件

use std::sync::Arc;
use flare_proto::common::SyncMessagesResponse;
use crate::domain::message_queue::MessageQueue;
use crate::domain::repository::ReadStore;
use crate::infrastructure::event_bus::EventBus;
use crate::application::fsm::FsmManager;
use crate::infrastructure::converter::MessageConverter;
use tracing::{info, warn, debug, error};

/// 同步消息处理器
pub struct SyncHandler {
    message_queue: Arc<MessageQueue>,
    read_store: Arc<dyn ReadStore>,
    event_bus: Arc<EventBus>,
    fsm: Arc<FsmManager>,
}

impl SyncHandler {
    /// 创建新的同步消息处理器
    pub fn new(
        message_queue: Arc<MessageQueue>,
        read_store: Arc<dyn ReadStore>,
        event_bus: Arc<EventBus>,
        fsm: Arc<FsmManager>,
    ) -> Self {
        Self {
            message_queue,
            read_store,
            event_bus,
            fsm,
        }
    }
    
    /// 处理同步消息响应
    ///
    /// # 参数
    ///
    /// * `resp` - 同步消息响应
    ///
    /// # 返回
    ///
    /// * `Ok(())` - 处理成功
    /// * `Err` - 处理失败
    pub async fn handle_sync_messages_response(
        &self,
        resp: SyncMessagesResponse,
    ) -> anyhow::Result<()> {
        info!("Processing SyncMessagesResponse");
        
        // 1. 提取 MessageEnvelope（可能包含多条消息）
        let envelope = match resp.envelope {
            Some(envelope) => envelope,
            None => {
                warn!("SyncMessagesResponse has no envelope");
                return Ok(());
            }
        };
        
        let message_count = envelope.messages.len();
        debug!(
            envelope_kind = envelope.kind,
            message_count = message_count,
            "Extracted MessageEnvelope from SyncMessagesResponse"
        );
        
        if message_count == 0 {
            debug!("MessageEnvelope is empty, nothing to process");
            return Ok(());
        }
        
        // 2. 批量解析消息（避免逐条解析）
        let mut messages = Vec::with_capacity(message_count);
        let mut failed_count = 0;
        let mut failed_message_ids = Vec::new();
        
        for proto_msg in envelope.messages {
            match MessageConverter::from_proto(&proto_msg) {
                Ok(message) => {
                    messages.push(message);
                }
                Err(e) => {
                    let msg_id = proto_msg.id.clone();
                    failed_message_ids.push(msg_id.clone());
                    warn!(
                        message_id = %msg_id,
                        error = %e,
                        "Failed to parse message from sync response"
                    );
                    failed_count += 1;
                }
            }
        }
        
        let success_count = messages.len();
        info!(
            total_count = message_count,
            success_count = success_count,
            failed_count = failed_count,
            "Parsed messages from sync response"
        );
        
        if messages.is_empty() {
            if failed_count > 0 {
                warn!(
                    failed_count = failed_count,
                    failed_message_ids = ?failed_message_ids,
                    "All messages failed to parse, sync response may be corrupted"
                );
            } else {
                debug!("No messages in sync response");
            }
            return Ok(());
        }
        
        // 3. 性能优化：批量入队（减少 await 次数和锁竞争）
        let priority = 5u8;
        let max_seq = messages.iter().filter_map(|m| m.seq).max();
        
        // 批量入队，一次性处理所有消息
        let messages_with_priority: Vec<_> = messages.into_iter()
            .map(|msg| (msg, priority))
            .collect();
        
        let enqueued_count = self.message_queue.enqueue_batch(messages_with_priority).await;
        
        if enqueued_count < message_count {
            let dropped = message_count - enqueued_count;
            warn!(
                enqueued = enqueued_count,
                total = message_count,
                dropped = dropped,
                "Some messages failed to enqueue (duplicates or queue full)"
            );
        }
        
        // 4. 更新同步游标（cursor / max_seq）
        if let Some(seq) = max_seq {
            // TODO: 更新 FSM 或 SyncCoordinator 中的游标
        }
        
        // 5. 判断 has_more，触发后续同步
        if let Some(has_more_str) = resp.metadata.get("has_more") {
            if has_more_str == "true" {
                // TODO: 触发后续同步
            }
        }
        
        // 6. 性能优化：异步发布同步完成事件（不阻塞主流程）
        let event_bus = self.event_bus.clone();
        let enqueued = enqueued_count;
        let total = message_count;
        let failed = failed_count;
        tokio::spawn(async move {
            use crate::domain::event::DomainEvent;
            let sync_event = DomainEvent::new(
                "sync.messages.completed",
                "sync",
                1,
                serde_json::json!({
                    "message_count": enqueued,
                    "total_count": total,
                    "failed_parse_count": failed,
                    "max_seq": max_seq,
                }),
            );
            
            if let Err(e) = event_bus.publish(sync_event).await {
                warn!("Failed to publish sync completion event: {}", e);
            }
        });
        
        Ok(())
    }
}
