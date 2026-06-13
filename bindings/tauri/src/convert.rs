//! Core SDK event to Tauri IPC payload conversion.
//!
//! Keep this layer mechanical: it maps typed `SdkEvent` variants to contract
//! `im://*` channel names and snake_case JSON payloads.

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

pub fn sdk_event_to_tauri(ev: &SdkEvent) -> Option<(String, Value)> {
    let (channel, payload) = match ev {
        SdkEvent::Connection(ConnectionEvent::Connected) => ("im://connected", json!({})),
        SdkEvent::Connection(ConnectionEvent::Disconnected { reason }) => {
            ("im://disconnected", json!({ "reason": reason }))
        }
        SdkEvent::Connection(ConnectionEvent::StateChanged { state }) => {
            ("im://state", json!({ "state": format!("{state:?}") }))
        }
        SdkEvent::Connection(ConnectionEvent::ServerError { code, message }) => (
            "im://server_error",
            json!({ "code": code, "message": message }),
        ),
        SdkEvent::Connection(ConnectionEvent::Reconnecting { attempt }) => {
            ("im://reconnecting", json!({ "attempt": attempt }))
        }
        SdkEvent::Connection(ConnectionEvent::KickedOff { reason }) => {
            ("im://kicked_off", json!({ "reason": reason }))
        }
        SdkEvent::Connection(ConnectionEvent::TokenExpired { message }) => {
            ("im://token_expired", json!({ "message": message }))
        }
        SdkEvent::Connection(ConnectionEvent::SyncStateChanged { state }) => (
            "im://sync_state_changed",
            json!({ "state": format!("{state:?}") }),
        ),

        SdkEvent::Message(MessageEvent::Received { message }) => {
            ("im://message", message_json(message.as_ref()))
        }
        SdkEvent::Message(MessageEvent::ReceivedBatch { messages }) => (
            "im://message_batch",
            json!({ "messages": messages.iter().map(message_json).collect::<Vec<_>>() }),
        ),
        SdkEvent::Message(MessageEvent::SendAck { ack }) => {
            ("im://send_ack", send_ack_json(ack.as_ref()))
        }
        SdkEvent::Message(MessageEvent::SendFailed {
            client_msg_id,
            reason,
        }) => (
            "im://send_failed",
            json!({ "client_msg_id": client_msg_id, "reason": reason }),
        ),
        SdkEvent::Message(MessageEvent::Recalled {
            conversation_id,
            event,
        }) => (
            "im://message_recalled",
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
            "im://message_edited",
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
            "im://message_reaction_changed",
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
            "im://typing",
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
            "im://message_deleted",
            json!({
                "conversation_id": conversation_id,
                "message_id": event.server_msg_id
            }),
        ),
        SdkEvent::Message(MessageEvent::ReadReceipt {
            conversation_id,
            event,
        }) => (
            "im://message_read_receipt",
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
            "im://presence_changed",
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
            "im://message_custom_event",
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
            ("im://notification", message_json(message.as_ref()))
        }

        SdkEvent::Conversation(ConversationEvent::Synced { conversation_ids }) => (
            "im://conversations_synced",
            json!({ "conversation_ids": conversation_ids }),
        ),
        SdkEvent::Conversation(ConversationEvent::Created { conversation_id }) => (
            "im://conversation_created",
            json!({ "conversation_id": conversation_id }),
        ),
        SdkEvent::Conversation(ConversationEvent::Updated { conversation_id }) => (
            "im://conversation_updated",
            json!({ "conversation_id": conversation_id }),
        ),
        SdkEvent::Conversation(ConversationEvent::Deleted { conversation_id }) => (
            "im://conversation_deleted",
            json!({ "conversation_id": conversation_id }),
        ),
        SdkEvent::Conversation(ConversationEvent::UnreadCountChanged {
            conversation_id,
            unread_count,
        }) => (
            "im://unread_count_changed",
            json!({ "conversation_id": conversation_id, "unread_count": unread_count }),
        ),

        SdkEvent::Sync(SyncNotify::ResyncNeeded {
            scope,
            reason,
            dropped_events,
        }) => (
            "im://resync_needed",
            json!({
                "scope": scope,
                "reason": reason,
                "dropped_events": dropped_events
            }),
        ),
        SdkEvent::Sync(SyncNotify::Started { .. }) => ("im://sync_started", json!({})),
        SdkEvent::Sync(SyncNotify::Finished { phase, .. }) => {
            let phase = match phase {
                SyncPhase::Init => "Init",
                SyncPhase::Background => "Background",
            };
            ("im://sync_finished", json!({ "phase": phase }))
        }
        SdkEvent::Sync(SyncNotify::Failed { message, .. }) => {
            ("im://sync_failed", json!({ "error": message }))
        }
        SdkEvent::Sync(SyncNotify::Progress {
            task,
            progress,
            detail,
            ..
        }) => (
            "im://sync_progress",
            json!({
                "task": task,
                "progress": progress,
                "detail": detail
            }),
        ),
        _ => return None,
    };

    Some((channel.to_string(), payload))
}
