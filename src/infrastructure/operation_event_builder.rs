//! 将领域 MessageOperation 转为长连接 Event（send_event 上行）
//!
//! 协议：ClientPacket.send_event = Event，操作统一走事件流

use anyhow::Result;
use flare_proto::common::{
    Event, EventType,
    MessageRecallEvent, MessageEditEvent, MessageDeleteEvent, ReadReceiptEvent,
    ReactionEvent, PinEvent, UnpinEvent, MarkEvent, UnmarkEvent,
};
use flare_proto::common::event::Payload as EventPayload;
use prost_types::Timestamp;
use crate::domain::message::{MessageOperation, OperationType, OperationData, DeleteType, ReactionAction, MarkType};

/// 将领域 MessageOperation 转为 proto Event（用于 send_event 上行）
pub fn operation_to_event(operation: &MessageOperation, conversation_id: &str) -> Result<Event> {
    let event_type = operation_type_to_event_type(operation.operation_type);
    let payload = operation_data_to_payload(operation, conversation_id)?;
    let created_at = Some(Timestamp {
        seconds: operation.timestamp.timestamp(),
        nanos: operation.timestamp.timestamp_subsec_nanos() as i32,
    });
    Ok(Event {
        tenant_id: String::new(),
        conversation_id: conversation_id.to_string(),
        seq: 0,
        r#type: event_type,
        created_at,
        operator_id: operation.operator_id.clone(),
        event_seq: None,
        request_id: None,
        payload: Some(payload),
    })
}

fn operation_type_to_event_type(ot: OperationType) -> i32 {
    use EventType::*;
    let t = match ot {
        OperationType::Recall => EventMessageRecall,
        OperationType::Edit => EventMessageEdit,
        OperationType::Delete => EventMessageDelete,
        OperationType::Read => EventReadReceipt,
        OperationType::ReactionAdd | OperationType::ReactionRemove => EventReaction,
        OperationType::Pin => EventPin,
        OperationType::Unpin => EventUnpin,
        OperationType::Mark => EventMark,
        OperationType::Unmark => EventUnmark,
        OperationType::Forward => return EventType::EventMessage as i32, // 不应走 send_event
    };
    t as i32
}

fn operation_data_to_payload(operation: &MessageOperation, conversation_id: &str) -> Result<EventPayload> {
    let server_msg_id = operation.target_message_id.clone();
    let payload = match &operation.operation_data {
        OperationData::Recall { reason, time_limit_seconds, allow_admin_recall } => {
            EventPayload::Recall(MessageRecallEvent {
                server_msg_id,
                reason: reason.clone().unwrap_or_default(),
                time_limit_seconds: *time_limit_seconds,
                allow_admin_recall: Some(*allow_admin_recall),
            })
        }
        OperationData::Edit { new_content, edit_version, reason, show_edited_mark } => {
            EventPayload::Edit(MessageEditEvent {
                server_msg_id,
                new_content: new_content.clone(),
                edit_version: *edit_version,
                reason: reason.clone().unwrap_or_default(),
                show_edited_mark: *show_edited_mark,
            })
        }
        OperationData::Delete { delete_type, reason, notify_others } => {
            let delete_type_proto = match delete_type {
                DeleteType::Soft => 1i32,
                DeleteType::Hard => 2i32,
            };
            EventPayload::Delete(MessageDeleteEvent {
                server_msg_id,
                delete_type: Some(delete_type_proto),
                reason: reason.clone(),
                notify_others: Some(*notify_others),
            })
        }
        OperationData::Read { message_ids, read_at, burn_after_read } => {
            let read_at_proto = read_at.map(|dt| Timestamp {
                seconds: dt.timestamp(),
                nanos: dt.timestamp_subsec_nanos() as i32,
            });
            EventPayload::Read(ReadReceiptEvent {
                conversation_id: conversation_id.to_string(),
                read_seq: 0,
                user_id: operation.operator_id.clone(),
                message_ids: message_ids.clone(),
                read_at: read_at_proto,
                burn_after_read: Some(*burn_after_read),
            })
        }
        OperationData::Reaction { emoji, action, .. } => {
            let action_proto = match action {
                ReactionAction::Add => 1i32,
                ReactionAction::Remove => 2i32,
            };
            EventPayload::Reaction(ReactionEvent {
                server_msg_id,
                user_id: operation.operator_id.clone(),
                emoji: emoji.clone(),
                action: action_proto,
            })
        }
        OperationData::Pin { reason, expire_at } => {
            let expire_at_proto = expire_at.map(|dt| Timestamp {
                seconds: dt.timestamp(),
                nanos: dt.timestamp_subsec_nanos() as i32,
            });
            EventPayload::Pin(PinEvent {
                server_msg_id,
                pinned_by: operation.operator_id.clone(),
                reason: reason.clone(),
                expire_at: expire_at_proto,
            })
        }
        OperationData::Unpin => EventPayload::Unpin(UnpinEvent { server_msg_id }),
        OperationData::Mark { mark_type, color } => {
            let mark_type_proto = mark_type_to_proto(*mark_type);
            EventPayload::Mark(MarkEvent {
                server_msg_id,
                user_id: operation.operator_id.clone(),
                mark_type: mark_type_proto,
                color: color.clone().unwrap_or_default(),
            })
        }
        OperationData::Unmark { mark_type } => {
            let mark_type_proto = mark_type.map(mark_type_to_proto).unwrap_or(0);
            EventPayload::Unmark(UnmarkEvent {
                server_msg_id,
                user_id: operation.operator_id.clone(),
                mark_type: mark_type_proto,
            })
        }
        OperationData::Forward { .. } => {
            return Err(anyhow::anyhow!("Forward should not be sent as Event"));
        }
    };
    Ok(payload)
}

fn mark_type_to_proto(mt: MarkType) -> i32 {
    use MarkType::*;
    match mt {
        Important => 1,
        Todo => 2,
        Done => 3,
        Custom => 4,
    }
}

/// 从 proto Event 解析为领域 MessageOperation（用于同步/推送下来的事件）
pub fn event_to_operation(event: &Event) -> Result<MessageOperation> {
    use chrono::Utc;
    let operator_id = event.operator_id.clone();
    let timestamp = event
        .created_at
        .as_ref()
        .and_then(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32))
        .unwrap_or_else(Utc::now);
    let (operation_type, target_message_id, operation_data) = match &event.payload {
        Some(EventPayload::Recall(r)) => (
            OperationType::Recall,
            r.server_msg_id.clone(),
            OperationData::Recall {
                reason: Some(r.reason.clone()),
                time_limit_seconds: r.time_limit_seconds,
                allow_admin_recall: r.allow_admin_recall.unwrap_or(false),
            },
        ),
        Some(EventPayload::Edit(e)) => (
            OperationType::Edit,
            e.server_msg_id.clone(),
            OperationData::Edit {
                new_content: e.new_content.clone(),
                edit_version: e.edit_version,
                reason: Some(e.reason.clone()),
                show_edited_mark: e.show_edited_mark,
            },
        ),
        Some(EventPayload::Delete(d)) => {
            let delete_type = match d.delete_type {
                Some(1) => DeleteType::Soft,
                Some(2) => DeleteType::Hard,
                _ => DeleteType::Soft,
            };
            (
                OperationType::Delete,
                d.server_msg_id.clone(),
                OperationData::Delete {
                    delete_type,
                    reason: d.reason.clone(),
                    notify_others: d.notify_others.unwrap_or(false),
                },
            )
        }
        Some(EventPayload::Read(r)) => {
            let read_at = r.read_at.as_ref().and_then(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32));
            (
                OperationType::Read,
                r.message_ids.first().cloned().unwrap_or_default(),
                OperationData::Read {
                    message_ids: r.message_ids.clone(),
                    read_at,
                    burn_after_read: r.burn_after_read.unwrap_or(false),
                },
            )
        }
        Some(EventPayload::Reaction(r)) => {
            let action = if r.action == 1 {
                ReactionAction::Add
            } else {
                ReactionAction::Remove
            };
            (
                if r.action == 1 {
                    OperationType::ReactionAdd
                } else {
                    OperationType::ReactionRemove
                },
                r.server_msg_id.clone(),
                OperationData::Reaction {
                    emoji: r.emoji.clone(),
                    action,
                    count: 0,
                },
            )
        }
        Some(EventPayload::Pin(p)) => {
            let expire_at = p.expire_at.as_ref().and_then(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32));
            (
                OperationType::Pin,
                p.server_msg_id.clone(),
                OperationData::Pin {
                    reason: p.reason.clone(),
                    expire_at,
                },
            )
        }
        Some(EventPayload::Unpin(u)) => (
            OperationType::Unpin,
            u.server_msg_id.clone(),
            OperationData::Unpin,
        ),
        Some(EventPayload::Mark(m)) => (
            OperationType::Mark,
            m.server_msg_id.clone(),
            OperationData::Mark {
                mark_type: proto_to_mark_type(m.mark_type),
                color: Some(m.color.clone()),
            },
        ),
        Some(EventPayload::Unmark(u)) => (
            OperationType::Unmark,
            u.server_msg_id.clone(),
            OperationData::Unmark {
                mark_type: Some(proto_to_mark_type(u.mark_type)),
            },
        ),
        _ => return Err(anyhow::anyhow!("Unsupported event payload for operation")),
    };
    Ok(MessageOperation {
        operation_type,
        target_message_id,
        operator_id,
        timestamp,
        show_notice: false,
        notice_text: None,
        target_user_id: None,
        operation_data,
        metadata: std::collections::HashMap::new(),
    })
}

fn proto_to_mark_type(v: i32) -> MarkType {
    use MarkType::*;
    match v {
        1 => Important,
        2 => Todo,
        3 => Done,
        4 => Custom,
        _ => Important,
    }
}
