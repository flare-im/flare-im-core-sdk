//! 事件流处理器：EventEnvelope → 入队 + 领域事件发布

use std::sync::Arc;
use flare_proto::common::{EventType, event::Payload as EventPayload};
use tracing::{debug, info, warn};

use crate::domain::event::{DomainEvent, message_events};
use crate::domain::event::message::{
    MessageRecalled, MessageEdited, MessageDeleted,
    MessageReactionAdded, MessageReactionRemoved, MessagePinned, MessageUnpinned,
    MessageMarked, MessageUnmarked, MessageRead,
};
use crate::domain::message_queue::MessageQueue;
use crate::infrastructure::converter::MessageConverter;
use crate::infrastructure::event_bus::EventBus;
use crate::infrastructure::operation_event_builder::event_to_operation;

/// 事件流处理器
///
/// 消费 proto EventEnvelope，按 EventType 分发：消息入队、操作转 DomainEvent 发布
pub struct EventStreamProcessor {
    message_queue: Arc<MessageQueue>,
    event_bus: Arc<EventBus>,
}

impl EventStreamProcessor {
    pub fn new(message_queue: Arc<MessageQueue>, event_bus: Arc<EventBus>) -> Self {
        Self {
            message_queue,
            event_bus,
        }
    }

    /// 处理单次事件批（推送或同步中的 envelope）
    pub async fn process(&self, envelope: &flare_proto::common::EventEnvelope) -> anyhow::Result<()> {
        for ev in &envelope.events {
            if let Err(e) = self.process_one_event(ev).await {
                warn!(event_seq = ?ev.seq, conversation_id = ?ev.conversation_id, error = %e, "Event stream process one event failed");
            }
        }
        Ok(())
    }

    async fn process_one_event(&self, ev: &flare_proto::common::Event) -> anyhow::Result<()> {
        let typ = EventType::try_from(ev.r#type).unwrap_or(EventType::Unspecified);
        match typ {
            EventType::EventMessage => self.handle_message(ev).await,
            EventType::EventMessageRecall => self.handle_recall(ev).await,
            EventType::EventMessageEdit => self.handle_edit(ev).await,
            EventType::EventMessageDelete => self.handle_delete(ev).await,
            EventType::EventReadReceipt => self.handle_read_receipt(ev).await,
            EventType::EventReaction => self.handle_reaction(ev).await,
            EventType::EventPin => self.handle_pin(ev).await,
            EventType::EventUnpin => self.handle_unpin(ev).await,
            EventType::EventMark => self.handle_mark(ev).await,
            EventType::EventUnmark => self.handle_unmark(ev).await,
            EventType::EventConversationUpdate | EventType::EventConversationDelete => {
                self.handle_conversation_event(ev).await
            }
            _ => {
                debug!(event_type = ?typ, "Unhandled event type in stream");
                Ok(())
            }
        }
    }

    async fn handle_message(&self, ev: &flare_proto::common::Event) -> anyhow::Result<()> {
        let Some(EventPayload::Message(proto_msg)) = &ev.payload else {
            return Ok(());
        };
        let message = match MessageConverter::from_proto(proto_msg) {
            Ok(m) => m,
            Err(e) => {
                warn!(
                    error = %e,
                    conversation_id = ?ev.conversation_id,
                    seq = ?ev.seq,
                    "EventStreamProcessor: from_proto failed, skipping event"
                );
                return Err(e);
            }
        };
        let message_id = message.server_id.as_deref().unwrap_or(&message.client_msg_id).to_string();
        let conversation_id = message.conversation_id.clone().unwrap_or_default();
        let priority = 10u8;
        let enqueued = self.message_queue.enqueue(message.clone(), priority).await;
        debug!(
            message_id = %message_id,
            conversation_id = %conversation_id,
            sender_id = %message.sender_id,
            enqueued = enqueued,
            "EventStreamProcessor: message enqueued from EventEnvelope"
        );

        // 不在此处发布 MessageCreated：由队列处理器 DefaultMessageHandler 在持久化后统一发布，
        // 保证事件中 content 为完整消息体，且仅发布一次。
        Ok(())
    }

    async fn handle_recall(&self, ev: &flare_proto::common::Event) -> anyhow::Result<()> {
        let op = event_to_operation(ev)?;
        let aggregate_id = ev.conversation_id.clone();
        let data = serde_json::to_value(MessageRecalled {
            message_id: op.target_message_id.clone(),
            recaller_id: op.operator_id.clone(),
        })?;
        let domain_event = DomainEvent::new(message_events::RECALLED, aggregate_id, 0, data);
        let _ = self.event_bus.publish(domain_event).await;
        Ok(())
    }

    async fn handle_edit(&self, ev: &flare_proto::common::Event) -> anyhow::Result<()> {
        let op = event_to_operation(ev)?;
        info!(
            message_id = %op.target_message_id,
            conversation_id = %ev.conversation_id,
            editor_id = %op.operator_id,
            "收到编辑操作（事件流）"
        );
        let aggregate_id = ev.conversation_id.clone();
        let data = serde_json::to_value(MessageEdited {
            message_id: op.target_message_id.clone(),
            editor_id: op.operator_id.clone(),
            new_content: serde_json::Value::Null,
        })?;
        let domain_event = DomainEvent::new(message_events::EDITED, aggregate_id, 0, data);
        let _ = self.event_bus.publish(domain_event).await;
        Ok(())
    }

    async fn handle_delete(&self, ev: &flare_proto::common::Event) -> anyhow::Result<()> {
        let op = event_to_operation(ev)?;
        let aggregate_id = ev.conversation_id.clone();
        let delete_type = match &op.operation_data {
            crate::domain::message::OperationData::Delete { delete_type, .. } => {
                format!("{:?}", delete_type).to_lowercase()
            }
            _ => "soft".to_string(),
        };
        let data = serde_json::to_value(MessageDeleted {
            message_id: op.target_message_id.clone(),
            operator_id: op.operator_id.clone(),
            delete_type,
        })?;
        let domain_event = DomainEvent::new(message_events::DELETED, aggregate_id, 0, data);
        let _ = self.event_bus.publish(domain_event).await;
        Ok(())
    }

    async fn handle_read_receipt(&self, ev: &flare_proto::common::Event) -> anyhow::Result<()> {
        let Some(EventPayload::Read(r)) = &ev.payload else {
            return Ok(());
        };
        let reader_id = r.user_id.clone();
        for msg_id in &r.message_ids {
            let aggregate_id = ev.conversation_id.clone();
            let data = serde_json::to_value(MessageRead {
                message_id: msg_id.clone(),
                reader_id: reader_id.clone(),
            })?;
            let domain_event = DomainEvent::new(message_events::READ, aggregate_id.clone(), 0, data);
            let _ = self.event_bus.publish(domain_event).await;
        }
        Ok(())
    }

    async fn handle_reaction(&self, ev: &flare_proto::common::Event) -> anyhow::Result<()> {
        let op = event_to_operation(ev)?;
        let aggregate_id = ev.conversation_id.clone();
        let (event_type, data) = match &op.operation_data {
            crate::domain::message::OperationData::Reaction { emoji, action, .. } => {
                let is_add = matches!(action, crate::domain::message::ReactionAction::Add);
                let (event_type, body) = if is_add {
                    (
                        message_events::REACTION_ADDED,
                        serde_json::to_value(MessageReactionAdded {
                            message_id: op.target_message_id.clone(),
                            emoji: emoji.clone(),
                            user_id: op.operator_id.clone(),
                        })?,
                    )
                } else {
                    (
                        message_events::REACTION_REMOVED,
                        serde_json::to_value(MessageReactionRemoved {
                            message_id: op.target_message_id.clone(),
                            emoji: emoji.clone(),
                            user_id: op.operator_id.clone(),
                        })?,
                    )
                };
                (event_type, body)
            }
            _ => return Ok(()),
        };
        let domain_event = DomainEvent::new(event_type, aggregate_id, 0, data);
        let _ = self.event_bus.publish(domain_event).await;
        Ok(())
    }

    async fn handle_pin(&self, ev: &flare_proto::common::Event) -> anyhow::Result<()> {
        let op = event_to_operation(ev)?;
        let aggregate_id = ev.conversation_id.clone();
        let data = serde_json::to_value(MessagePinned {
            message_id: op.target_message_id.clone(),
            operator_id: op.operator_id.clone(),
        })?;
        let domain_event = DomainEvent::new(message_events::PINNED, aggregate_id, 0, data);
        let _ = self.event_bus.publish(domain_event).await;
        Ok(())
    }

    async fn handle_unpin(&self, ev: &flare_proto::common::Event) -> anyhow::Result<()> {
        let op = event_to_operation(ev)?;
        let aggregate_id = ev.conversation_id.clone();
        let data = serde_json::to_value(MessageUnpinned {
            message_id: op.target_message_id.clone(),
            operator_id: op.operator_id.clone(),
        })?;
        let domain_event = DomainEvent::new(message_events::UNPINNED, aggregate_id, 0, data);
        let _ = self.event_bus.publish(domain_event).await;
        Ok(())
    }

    async fn handle_mark(&self, ev: &flare_proto::common::Event) -> anyhow::Result<()> {
        let op = event_to_operation(ev)?;
        let aggregate_id = ev.conversation_id.clone();
        let mark_type = match &op.operation_data {
            crate::domain::message::OperationData::Mark { mark_type, .. } => format!("{:?}", mark_type),
            _ => "Important".to_string(),
        };
        let data = serde_json::to_value(MessageMarked {
            message_id: op.target_message_id.clone(),
            user_id: op.operator_id.clone(),
            mark_type,
        })?;
        let domain_event = DomainEvent::new(message_events::MARKED, aggregate_id, 0, data);
        let _ = self.event_bus.publish(domain_event).await;
        Ok(())
    }

    async fn handle_unmark(&self, ev: &flare_proto::common::Event) -> anyhow::Result<()> {
        let op = event_to_operation(ev)?;
        let aggregate_id = ev.conversation_id.clone();
        let data = serde_json::to_value(MessageUnmarked {
            message_id: op.target_message_id.clone(),
            user_id: op.operator_id.clone(),
        })?;
        let domain_event = DomainEvent::new(message_events::UNMARKED, aggregate_id, 0, data);
        let _ = self.event_bus.publish(domain_event).await;
        Ok(())
    }

    async fn handle_conversation_event(&self, _ev: &flare_proto::common::Event) -> anyhow::Result<()> {
        // 会话更新/删除可由 ConversationSyncHandler 或单独订阅者处理；此处仅记录
        debug!("Conversation event in stream (update/delete)");
        Ok(())
    }
}
