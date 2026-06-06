//! SdkEvent → JS callback for browser hosts.

use std::cell::RefCell;
use std::rc::Rc;

use flare_im_core_sdk::core::event::SdkEvent;
use js_sys::Function;
use serde_json::{Value, json};
use wasm_bindgen::JsValue;

thread_local! {
    static EVENT_CALLBACK: RefCell<Option<Rc<Function>>> = RefCell::new(None);
}

pub fn set_event_callback(callback: Option<Function>) {
    EVENT_CALLBACK.with(|slot| {
        *slot.borrow_mut() = callback.map(Rc::new);
    });
}

pub fn clear_event_callback() {
    set_event_callback(None);
}

pub fn emit_sdk_event_to_js(ev: &SdkEvent) {
    let Some(payload) = sdk_event_to_web_payload(ev) else {
        return;
    };
    EVENT_CALLBACK.with(|slot| {
        let Some(callback) = slot.borrow().clone() else {
            return;
        };
        if let Ok(value) = serde_wasm_bindgen::to_value(&payload) {
            let _ = callback.call1(&JsValue::NULL, &value);
        }
    });
}

fn message_json(message: &flare_im_core_sdk::model::IMMessage) -> Value {
    serde_json::to_value(message).unwrap_or_else(|_| json!({}))
}

fn sdk_event_to_web_payload(ev: &SdkEvent) -> Option<Value> {
    use flare_im_core_sdk::core::event::{
        ConnectionEvent, ConversationEvent, MessageEvent, NotificationEvent, SdkEvent, SyncNotify,
        SyncPhase,
    };
    match ev {
        SdkEvent::Connection(ConnectionEvent::Connected) => Some(json!({
            "channel": "im://connected",
            "payload": {}
        })),
        SdkEvent::Connection(ConnectionEvent::Disconnected { reason }) => Some(json!({
            "channel": "im://disconnected",
            "payload": { "reason": reason }
        })),
        SdkEvent::Connection(ConnectionEvent::StateChanged { state }) => Some(json!({
            "channel": "im://state",
            "payload": { "state": format!("{state:?}") }
        })),
        SdkEvent::Connection(ConnectionEvent::ServerError { code, message }) => Some(json!({
            "channel": "im://server_error",
            "payload": { "code": code, "message": message }
        })),
        SdkEvent::Connection(ConnectionEvent::Reconnecting { attempt }) => Some(json!({
            "channel": "im://reconnecting",
            "payload": { "attempt": attempt }
        })),
        SdkEvent::Connection(ConnectionEvent::KickedOff { reason }) => Some(json!({
            "channel": "im://kicked_off",
            "payload": { "reason": reason }
        })),
        SdkEvent::Connection(ConnectionEvent::TokenExpired { message }) => Some(json!({
            "channel": "im://token_expired",
            "payload": { "message": message }
        })),
        SdkEvent::Connection(ConnectionEvent::SyncStateChanged { state }) => Some(json!({
            "channel": "im://sync_state_changed",
            "payload": { "state": format!("{state:?}") }
        })),

        SdkEvent::Message(MessageEvent::Received { message }) => Some(json!({
            "channel": "im://message",
            "payload": message_json(message.as_ref())
        })),
        SdkEvent::Message(MessageEvent::ReceivedBatch { messages }) => Some(json!({
            "channel": "im://message_batch",
            "payload": {
                "messages": messages.iter().map(message_json).collect::<Vec<_>>()
            }
        })),
        SdkEvent::Message(MessageEvent::SendAck { ack }) => {
            let ack = ack.as_ref();
            Some(json!({
                "channel": "im://send_ack",
                "payload": {
                    "client_msg_id": ack.client_msg_id,
                    "server_msg_id": ack.server_msg_id,
                    "seq": ack.seq,
                    "conversation_id": ack.conversation_id,
                    "success": ack.success,
                    "error_code": ack.error_code,
                    "error_message": ack.error_message,
                    "error_detail": ack.error_detail.as_ref().map(|detail| json!({
                        "code": detail.code,
                        "reason": detail.reason,
                        "message": detail.message,
                        "track": detail.track,
                    })),
                }
            }))
        }
        SdkEvent::Message(MessageEvent::SendFailed {
            client_msg_id,
            reason,
        }) => Some(json!({
            "channel": "im://send_failed",
            "payload": { "client_msg_id": client_msg_id, "reason": reason }
        })),
        SdkEvent::Message(MessageEvent::Recalled {
            conversation_id,
            event,
        }) => Some(json!({
            "channel": "im://message_recalled",
            "payload": {
                "conversation_id": conversation_id,
                "message_id": event.server_msg_id,
                "recaller_id": ""
            }
        })),
        SdkEvent::Message(MessageEvent::Edited {
            conversation_id,
            server_msg_id,
            edit_version,
        }) => Some(json!({
            "channel": "im://message_edited",
            "payload": {
                "conversation_id": conversation_id,
                "message_id": server_msg_id,
                "edit_version": edit_version
            }
        })),
        SdkEvent::Message(MessageEvent::ReactionChanged {
            conversation_id,
            server_msg_id,
            user_id,
            emoji,
            action,
        }) => Some(json!({
            "channel": "im://message_reaction_changed",
            "payload": {
                "conversation_id": conversation_id,
                "message_id": server_msg_id,
                "user_id": user_id,
                "emoji": emoji,
                "action": action
            }
        })),
        SdkEvent::Message(MessageEvent::Typing {
            conversation_id,
            event,
        }) => Some(json!({
            "channel": "im://typing",
            "payload": {
                "conversation_id": conversation_id,
                "user_id": event.user_id,
                "typing": event.typing
            }
        })),
        SdkEvent::Message(MessageEvent::Deleted {
            conversation_id,
            event,
        }) => Some(json!({
            "channel": "im://message_deleted",
            "payload": {
                "conversation_id": conversation_id,
                "message_id": event.server_msg_id
            }
        })),
        SdkEvent::Message(MessageEvent::ReadReceipt {
            conversation_id,
            event,
        }) => Some(json!({
            "channel": "im://message_read_receipt",
            "payload": {
                "conversation_id": conversation_id,
                "user_id": event.user_id,
                "read_seq": event.read_seq,
                "message_ids": event.message_ids,
                "burn_after_read": event.burn_after_read.unwrap_or(false)
            }
        })),
        SdkEvent::Message(MessageEvent::PresenceChanged {
            conversation_id,
            event,
        }) => Some(json!({
            "channel": "im://presence_changed",
            "payload": {
                "conversation_id": conversation_id,
                "user_id": event.user_id,
                "status": event.status,
                "extra": event.extra
            }
        })),
        SdkEvent::Message(MessageEvent::Custom {
            conversation_id,
            event,
        }) => Some(json!({
            "channel": "im://message_custom_event",
            "payload": {
                "conversation_id": conversation_id,
                "namespace": event.namespace,
                "name": event.name,
                "version": event.version,
                "payload": event.payload,
                "metadata": event.metadata
            }
        })),

        SdkEvent::Notification(NotificationEvent::Received { message }) => Some(json!({
            "channel": "im://notification",
            "payload": message_json(message.as_ref())
        })),

        SdkEvent::Conversation(ConversationEvent::Synced { conversation_ids }) => Some(json!({
            "channel": "im://conversations_synced",
            "payload": { "conversation_ids": conversation_ids }
        })),
        SdkEvent::Conversation(ConversationEvent::Created { conversation_id }) => Some(json!({
            "channel": "im://conversation_created",
            "payload": { "conversation_id": conversation_id }
        })),
        SdkEvent::Conversation(ConversationEvent::Updated { conversation_id }) => Some(json!({
            "channel": "im://conversation_updated",
            "payload": { "conversation_id": conversation_id }
        })),
        SdkEvent::Conversation(ConversationEvent::Deleted { conversation_id }) => Some(json!({
            "channel": "im://conversation_deleted",
            "payload": { "conversation_id": conversation_id }
        })),
        SdkEvent::Conversation(ConversationEvent::UnreadCountChanged {
            conversation_id,
            unread_count,
        }) => Some(json!({
            "channel": "im://unread_count_changed",
            "payload": { "conversation_id": conversation_id, "unread_count": unread_count }
        })),

        SdkEvent::Sync(SyncNotify::Started { .. }) => Some(json!({
            "channel": "im://sync_started",
            "payload": {}
        })),
        SdkEvent::Sync(SyncNotify::Finished { phase, .. }) => {
            let phase_str = match phase {
                SyncPhase::Init => "Init",
                SyncPhase::Background => "Background",
            };
            Some(json!({
                "channel": "im://sync_finished",
                "payload": { "phase": phase_str }
            }))
        }
        SdkEvent::Sync(SyncNotify::Failed { message, .. }) => Some(json!({
            "channel": "im://sync_failed",
            "payload": { "error": message }
        })),
        SdkEvent::Sync(SyncNotify::Progress {
            task,
            progress,
            detail,
            ..
        }) => Some(json!({
            "channel": "im://sync_progress",
            "payload": {
                "task": task,
                "progress": progress,
                "detail": detail
            }
        })),
        _ => None,
    }
}

pub async fn forward_event_rx_to_js(mut rx: tokio::sync::broadcast::Receiver<SdkEvent>) {
    loop {
        let ev = match rx.recv().await {
            Ok(event) => event,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(dropped = n, "wasm event forward lagged");
                continue;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        };
        emit_sdk_event_to_js(&ev);
    }
}
