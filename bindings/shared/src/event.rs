use flare_im_core_sdk::event::{
    ConnectionEvent, ConversationEvent, MessageEvent, NotificationEvent, SdkEvent, SyncNotify,
    SyncPhase,
};
use serde_json::{Value, json};

fn message_json(message: &flare_im_core_sdk::model::IMMessage) -> Value {
    serde_json::to_value(message).unwrap_or_else(|_| json!({}))
}

fn send_ack_json(ack: &flare_im_core_sdk::model::SendAck) -> Value {
    let (server_msg_id, seq, timestamp, success, error_code, error_message, error_detail) =
        match ack.result.as_ref() {
            Some(flare_proto::common::send_ack::Result::Accepted(accepted)) => (
                accepted.server_msg_id.clone(),
                accepted.conversation_seq,
                accepted.server_time,
                true,
                0,
                String::new(),
                Value::Null,
            ),
            Some(flare_proto::common::send_ack::Result::Error(error)) => (
                String::new(),
                0,
                0,
                false,
                error.code,
                error.message.clone(),
                json!({
                    "code": error.code,
                    "reason": error.reason,
                    "message": error.message,
                    "track": error.track,
                }),
            ),
            None => (
                String::new(),
                0,
                0,
                false,
                0,
                "missing send ack result".to_string(),
                Value::Null,
            ),
        };
    json!({
        "client_msg_id": ack.client_msg_id,
        "server_msg_id": server_msg_id,
        "seq": seq,
        "conversation_id": ack.conversation_id,
        "ack_id": ack.ack_id,
        "timestamp": timestamp,
        "success": success,
        "error_code": error_code,
        "error_message": error_message,
        "error_detail": error_detail,
    })
}

pub fn sdk_event_payload(ev: &SdkEvent) -> Option<(&'static str, Value)> {
    let row = match ev {
        SdkEvent::Connection(ConnectionEvent::Connected) => ("connection.connected", json!({})),
        SdkEvent::Connection(ConnectionEvent::Disconnected { reason }) => {
            ("connection.disconnected", json!({ "reason": reason }))
        }
        SdkEvent::Connection(ConnectionEvent::StateChanged { state }) => (
            "connection.state_changed",
            json!({ "state": format!("{state:?}") }),
        ),
        SdkEvent::Connection(ConnectionEvent::ServerError { code, message }) => (
            "connection.server_error",
            json!({ "code": code, "message": message }),
        ),
        SdkEvent::Connection(ConnectionEvent::Reconnecting { attempt }) => {
            ("connection.reconnecting", json!({ "attempt": attempt }))
        }
        SdkEvent::Connection(ConnectionEvent::KickedOff { reason }) => {
            ("connection.kicked_off", json!({ "reason": reason }))
        }
        SdkEvent::Connection(ConnectionEvent::TokenExpired { message }) => {
            ("connection.token_expired", json!({ "message": message }))
        }
        SdkEvent::Connection(ConnectionEvent::SyncStateChanged { state }) => (
            "connection.sync_state_changed",
            json!({ "state": format!("{state:?}") }),
        ),

        SdkEvent::Message(MessageEvent::Received { message }) => {
            ("message.received", message_json(message.as_ref()))
        }
        SdkEvent::Message(MessageEvent::ReceivedBatch { messages }) => (
            "message.received_batch",
            json!({ "messages": messages.iter().map(message_json).collect::<Vec<_>>() }),
        ),
        SdkEvent::Message(MessageEvent::SendAck { ack }) => {
            ("message.send_ack", send_ack_json(ack.as_ref()))
        }
        SdkEvent::Message(MessageEvent::SendFailed {
            client_msg_id,
            reason,
        }) => (
            "message.send_failed",
            json!({ "client_msg_id": client_msg_id, "reason": reason }),
        ),
        SdkEvent::Message(MessageEvent::Recalled {
            conversation_id,
            event,
        }) => (
            "message.recalled",
            json!({
                "conversation_id": conversation_id,
                "message_id": event.server_msg_id,
                "recaller_id": ""
            }),
        ),
        SdkEvent::Message(MessageEvent::Edited {
            conversation_id,
            server_msg_id,
            edit_version,
        }) => (
            "message.edited",
            json!({
                "conversation_id": conversation_id,
                "message_id": server_msg_id,
                "edit_version": edit_version
            }),
        ),
        SdkEvent::Message(MessageEvent::ReactionChanged {
            conversation_id,
            server_msg_id,
            user_id,
            emoji,
            action,
        }) => (
            "message.reaction_changed",
            json!({
                "conversation_id": conversation_id,
                "message_id": server_msg_id,
                "user_id": user_id,
                "emoji": emoji,
                "action": action
            }),
        ),
        SdkEvent::Message(MessageEvent::Typing {
            conversation_id,
            event,
        }) => (
            "message.typing",
            json!({
                "conversation_id": conversation_id,
                "user_id": event.user_id,
                "typing": event.typing
            }),
        ),
        SdkEvent::Message(MessageEvent::Deleted {
            conversation_id,
            event,
        }) => (
            "message.deleted",
            json!({
                "conversation_id": conversation_id,
                "message_id": event.server_msg_id
            }),
        ),
        SdkEvent::Message(MessageEvent::ReadReceipt {
            conversation_id,
            event,
        }) => (
            "message.read_receipt",
            json!({
                "conversation_id": conversation_id,
                "user_id": event.user_id,
                "read_seq": event.read_seq,
                "message_ids": event.message_ids,
            }),
        ),
        SdkEvent::Message(MessageEvent::PresenceChanged {
            conversation_id,
            event,
        }) => (
            "message.presence_changed",
            json!({
                "conversation_id": conversation_id,
                "user_id": event.user_id,
                "status": event.status,
                "extra": event.attributes
            }),
        ),
        SdkEvent::Message(MessageEvent::Custom {
            conversation_id,
            event,
        }) => (
            "message.custom",
            json!({
                "conversation_id": conversation_id,
                "namespace": event.namespace,
                "name": event.name,
                "version": event.version,
                "payload": event.payload,
                "attributes": event.attributes
            }),
        ),

        SdkEvent::Notification(NotificationEvent::Received { message }) => {
            ("notification.received", message_json(message.as_ref()))
        }

        SdkEvent::Conversation(ConversationEvent::Synced { conversation_ids }) => (
            "conversation.synced",
            json!({ "conversation_ids": conversation_ids }),
        ),
        SdkEvent::Conversation(ConversationEvent::Created { conversation_id }) => (
            "conversation.created",
            json!({ "conversation_id": conversation_id }),
        ),
        SdkEvent::Conversation(ConversationEvent::Updated { conversation_id }) => (
            "conversation.updated",
            json!({ "conversation_id": conversation_id }),
        ),
        SdkEvent::Conversation(ConversationEvent::Deleted { conversation_id }) => (
            "conversation.deleted",
            json!({ "conversation_id": conversation_id }),
        ),
        SdkEvent::Conversation(ConversationEvent::UnreadCountChanged {
            conversation_id,
            unread_count,
        }) => (
            "conversation.unread_count_changed",
            json!({ "conversation_id": conversation_id, "unread_count": unread_count }),
        ),

        SdkEvent::Sync(SyncNotify::ResyncNeeded {
            scope,
            reason,
            dropped_events,
        }) => (
            "sync.resync_needed",
            json!({
                "scope": scope,
                "reason": reason,
                "dropped_events": dropped_events
            }),
        ),
        SdkEvent::Sync(SyncNotify::Started { .. }) => ("sync.started", json!({})),
        SdkEvent::Sync(SyncNotify::Finished { phase, .. }) => {
            let phase = match phase {
                SyncPhase::Init => "Init",
                SyncPhase::Background => "Background",
            };
            ("sync.finished", json!({ "phase": phase }))
        }
        SdkEvent::Sync(SyncNotify::Failed { message, .. }) => {
            ("sync.failed", json!({ "error": message }))
        }
        SdkEvent::Sync(SyncNotify::Progress {
            task,
            progress,
            detail,
            ..
        }) => (
            "sync.progress",
            json!({
                "task": task,
                "progress": progress,
                "detail": detail
            }),
        ),
        _ => return None,
    };

    Some(row)
}

pub fn sdk_event_code(ev: &SdkEvent) -> i32 {
    sdk_event_payload(ev)
        .and_then(|(id, _)| crate::find_event_by_id(id).map(|event| event.c_code))
        .unwrap_or(crate::generated::event_codes::FLARE_EVENT_UNKNOWN)
}

pub fn sdk_event_json(ev: &SdkEvent) -> String {
    sdk_event_payload(ev)
        .and_then(|(_, payload)| serde_json::to_string(&payload).ok())
        .unwrap_or_else(|| "{}".to_string())
}
