use crate::domain::MessageTransportAction;
use crate::model::event::{Event, EventType};
use flare_proto::common::event::Payload as EventPayload;
use flare_proto::common::{
    MarkEvent, MessageDeleteEvent, MessageEditEvent, MessageRecallEvent, PinEvent, ReactionEvent,
    ReadReceiptEvent, TypingEvent, UnmarkEvent, UnpinEvent,
};

pub fn event_from_transport_action(action: &MessageTransportAction) -> Event {
    match action {
        MessageTransportAction::Recall {
            conversation_id,
            server_msg_id,
        } => Event {
            conversation_id: conversation_id.clone(),
            r#type: EventType::EventMessageRecall as i32,
            payload: Some(EventPayload::Recall(MessageRecallEvent {
                server_msg_id: server_msg_id.clone(),
                reason: String::new(),
                time_limit_seconds: None,
                allow_admin_recall: None,
            })),
            ..Default::default()
        },
        MessageTransportAction::Edit {
            conversation_id,
            server_msg_id,
            new_content,
            edit_version,
            reason,
            show_edited_mark,
        } => Event {
            conversation_id: conversation_id.clone(),
            r#type: EventType::EventMessageEdit as i32,
            payload: Some(EventPayload::Edit(MessageEditEvent {
                server_msg_id: server_msg_id.clone(),
                new_content: new_content.clone(),
                edit_version: *edit_version,
                reason: reason.clone(),
                show_edited_mark: *show_edited_mark,
            })),
            ..Default::default()
        },
        MessageTransportAction::Delete {
            conversation_id,
            server_msg_id,
            delete_type,
            scope,
            reason,
            notify_others,
            target_user_id,
        } => Event {
            conversation_id: conversation_id.clone(),
            r#type: EventType::EventMessageDelete as i32,
            payload: Some(EventPayload::Delete(MessageDeleteEvent {
                server_msg_id: server_msg_id.clone(),
                delete_type: Some(*delete_type),
                scope: Some(*scope),
                reason: reason.clone(),
                notify_others: Some(*notify_others),
                target_user_id: target_user_id.clone(),
            })),
            ..Default::default()
        },
        MessageTransportAction::ReadReceipt {
            conversation_id,
            user_id,
            message_ids,
            read_seq,
            burn_after_read,
        } => Event {
            conversation_id: conversation_id.clone(),
            r#type: EventType::EventReadReceipt as i32,
            payload: Some(EventPayload::Read(ReadReceiptEvent {
                conversation_id: conversation_id.clone(),
                user_id: user_id.clone(),
                message_ids: message_ids.clone(),
                read_seq: *read_seq,
                burn_after_read: Some(*burn_after_read),
                ..Default::default()
            })),
            ..Default::default()
        },
        MessageTransportAction::Typing {
            conversation_id,
            user_id,
            typing,
        } => Event {
            conversation_id: conversation_id.clone(),
            r#type: EventType::EventTyping as i32,
            payload: Some(EventPayload::Typing(TypingEvent {
                conversation_id: conversation_id.clone(),
                user_id: user_id.clone(),
                typing: *typing,
            })),
            ..Default::default()
        },
        MessageTransportAction::Reaction {
            conversation_id,
            server_msg_id,
            user_id,
            emoji,
            action,
        } => Event {
            conversation_id: conversation_id.clone(),
            r#type: EventType::EventReaction as i32,
            payload: Some(EventPayload::Reaction(ReactionEvent {
                server_msg_id: server_msg_id.clone(),
                user_id: user_id.clone(),
                emoji: emoji.clone(),
                action: *action,
            })),
            ..Default::default()
        },
        MessageTransportAction::Pin {
            conversation_id,
            server_msg_id,
            pinned_by,
        } => Event {
            conversation_id: conversation_id.clone(),
            r#type: EventType::EventPin as i32,
            payload: Some(EventPayload::Pin(PinEvent {
                server_msg_id: server_msg_id.clone(),
                pinned_by: pinned_by.clone(),
                ..Default::default()
            })),
            ..Default::default()
        },
        MessageTransportAction::Unpin {
            conversation_id,
            server_msg_id,
        } => Event {
            conversation_id: conversation_id.clone(),
            r#type: EventType::EventUnpin as i32,
            payload: Some(EventPayload::Unpin(UnpinEvent {
                server_msg_id: server_msg_id.clone(),
            })),
            ..Default::default()
        },
        MessageTransportAction::Mark {
            conversation_id,
            server_msg_id,
            user_id,
            mark_type,
            color,
        } => Event {
            conversation_id: conversation_id.clone(),
            r#type: EventType::EventMark as i32,
            payload: Some(EventPayload::Mark(MarkEvent {
                server_msg_id: server_msg_id.clone(),
                user_id: user_id.clone(),
                mark_type: *mark_type,
                color: color.clone(),
            })),
            ..Default::default()
        },
        MessageTransportAction::Unmark {
            conversation_id,
            server_msg_id,
            user_id,
            mark_type,
        } => Event {
            conversation_id: conversation_id.clone(),
            r#type: EventType::EventUnmark as i32,
            payload: Some(EventPayload::Unmark(UnmarkEvent {
                server_msg_id: server_msg_id.clone(),
                user_id: user_id.clone(),
                mark_type: *mark_type,
            })),
            ..Default::default()
        },
    }
}
