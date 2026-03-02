//! 同步消息处理器
//!
//! 处理 SyncResponse（长连接线缆）：通过 EventStreamProcessor 统一处理 envelope，
//! 与推送路径一致（入队 + 领域事件发布），并发布 sync.messages.completed。

use std::sync::Arc;
use flare_proto::common::SyncResponse;
use crate::domain::message_queue::MessageQueue;
use crate::infrastructure::event_bus::EventBus;
use crate::infrastructure::converter::MessageConverter;
use crate::infrastructure::event_stream::EventStreamProcessor;
use tracing::{info, warn, debug};

/// 同步消息处理器
pub struct SyncHandler {
    message_queue: Arc<MessageQueue>,
    event_bus: Arc<EventBus>,
    event_stream_processor: Arc<EventStreamProcessor>,
}

impl SyncHandler {
    pub fn new(message_queue: Arc<MessageQueue>, event_bus: Arc<EventBus>) -> Self {
        let event_stream_processor = Arc::new(EventStreamProcessor::new(message_queue.clone(), event_bus.clone()));
        Self {
            message_queue,
            event_bus,
            event_stream_processor,
        }
    }

    /// 处理消息列表（用于 Bootstrap 或其他来源）
    pub async fn handle_messages(
        &self,
        proto_messages: Vec<flare_proto::common::Message>,
    ) -> anyhow::Result<()> {
        let message_count = proto_messages.len();
        if message_count == 0 {
            return Ok(());
        }

        info!("Processing {} messages", message_count);

        let mut messages = Vec::with_capacity(message_count);
        for proto_msg in proto_messages {
            match MessageConverter::from_proto(&proto_msg) {
                Ok(message) => messages.push(message),
                Err(e) => {
                    warn!(
                        message_id = %proto_msg.server_id,
                        error = %e,
                        "Failed to parse message"
                    );
                }
            }
        }

        if messages.is_empty() {
            return Ok(());
        }

        let priority = 5u8;
        let messages_with_priority: Vec<_> = messages
            .into_iter()
            .map(|msg| (msg, priority))
            .collect();
        self.message_queue.enqueue_batch(messages_with_priority).await;
        Ok(())
    }

    /// 处理同步响应（SyncResponse）：统一走 EventStreamProcessor，与推送路径一致
    pub async fn handle_sync_messages_response(
        &self,
        resp: SyncResponse,
    ) -> anyhow::Result<()> {
        info!("Processing SyncResponse");

        let envelope = match &resp.envelope {
            Some(e) => e,
            None => {
                warn!("SyncResponse has no envelope");
                return Ok(());
            }
        };

        debug!(
            event_count = envelope.events.len(),
            has_more = envelope.has_more,
            "Processing SyncResponse envelope via EventStreamProcessor"
        );
        self.event_stream_processor.process(envelope).await?;

        let event_bus = self.event_bus.clone();
        let has_more = envelope.has_more;
        let next_cursor = envelope.next_cursor.clone();
        let max_seq = envelope.max_seq;
        let event_count = envelope.events.len();

        tokio::spawn(async move {
            use crate::domain::event::DomainEvent;
            let sync_event = DomainEvent::new(
                "sync.messages.completed",
                "sync",
                1,
                serde_json::json!({
                    "event_count": event_count,
                    "max_seq": max_seq,
                    "has_more": has_more,
                    "next_cursor": next_cursor,
                }),
            );
            if let Err(e) = event_bus.publish(sync_event).await {
                warn!("Failed to publish sync completion event: {}", e);
            }
        });

        Ok(())
    }
}
