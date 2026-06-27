use flare_im_core_sdk::RawSdkEvent;
use flare_im_core_sdk::event::{
    ConnectionEvent, ConversationEvent, MessageEvent, NotificationEvent, SdkEvent, SyncNotify,
    SyncPhase,
};
use serde_json::{Value, json};
use std::fmt::Write as _;

fn message_json(message: &flare_im_core_sdk::model::IMMessage) -> Value {
    serde_json::to_value(message).unwrap_or_else(|_| json!({}))
}

fn send_ack_id(ack: &flare_im_core_sdk::model::SendAck) -> &str {
    ack.ack_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(ack.client_msg_id.as_str())
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
        "clientMsgId": ack.client_msg_id,
        "serverId": server_msg_id,
        "seq": seq,
        "conversationId": ack.conversation_id,
        "ackId": send_ack_id(ack),
        "timestamp": timestamp,
        "success": success,
        "errorCode": error_code,
        "errorMessage": error_message,
        "errorDetail": error_detail,
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
            json!({ "clientMsgId": client_msg_id, "reason": reason }),
        ),
        SdkEvent::Message(MessageEvent::Recalled {
            conversation_id,
            event,
        }) => (
            "message.recalled",
            json!({
                "conversationId": conversation_id,
                "messageId": event.server_msg_id,
                "serverMsgId": event.server_msg_id,
                "recallerId": ""
            }),
        ),
        SdkEvent::Message(MessageEvent::Edited {
            conversation_id,
            server_msg_id,
            edit_version,
        }) => (
            "message.edited",
            json!({
                "conversationId": conversation_id,
                "messageId": server_msg_id,
                "serverMsgId": server_msg_id,
                "editVersion": edit_version
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
                "conversationId": conversation_id,
                "serverMsgId": server_msg_id,
                "userId": user_id,
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
                "conversationId": conversation_id,
                "userId": event.user_id,
                "typing": event.typing
            }),
        ),
        SdkEvent::Message(MessageEvent::Deleted {
            conversation_id,
            event,
        }) => (
            "message.deleted",
            json!({
                "conversationId": conversation_id,
                "messageId": event.server_msg_id,
                "serverMsgId": event.server_msg_id
            }),
        ),
        SdkEvent::Message(MessageEvent::ReadReceipt {
            conversation_id,
            event,
        }) => (
            "message.read_receipt",
            json!({
                "conversationId": conversation_id,
                "userId": event.user_id,
                "readSeq": event.read_seq,
            }),
        ),
        SdkEvent::Message(MessageEvent::PresenceChanged {
            conversation_id,
            event,
        }) => (
            "message.presence_changed",
            json!({
                "conversationId": conversation_id,
                "userId": event.user_id,
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
                "conversationId": conversation_id,
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
            json!({ "conversationIds": conversation_ids }),
        ),
        SdkEvent::Conversation(ConversationEvent::Created { conversation_id }) => (
            "conversation.created",
            json!({ "conversationId": conversation_id }),
        ),
        SdkEvent::Conversation(ConversationEvent::Updated { conversation_id }) => (
            "conversation.updated",
            json!({ "conversationId": conversation_id }),
        ),
        SdkEvent::Conversation(ConversationEvent::Deleted { conversation_id }) => (
            "conversation.deleted",
            json!({ "conversationId": conversation_id }),
        ),
        SdkEvent::Conversation(ConversationEvent::UnreadCountChanged {
            conversation_id,
            unread_count,
        }) => (
            "conversation.unread_count_changed",
            json!({ "conversationId": conversation_id, "unreadCount": unread_count }),
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
                "droppedEvents": dropped_events
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
        SdkEvent::View(update) => (
            "view.updated",
            serde_json::to_value(update).unwrap_or_else(|_| json!({})),
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

pub fn sdk_event_channel_payload(ev: &SdkEvent) -> Option<(&'static str, Value)> {
    let (id, payload) = sdk_event_payload(ev)?;
    let channel = crate::find_event_by_id(id)?.tauri?;
    Some((channel, payload))
}

pub fn sdk_event_web_payload(ev: &SdkEvent) -> Option<Value> {
    let (channel, payload) = sdk_event_channel_payload(ev)?;
    Some(json!({
        "channel": channel,
        "payload": payload,
    }))
}

pub fn sdk_event_json(ev: &SdkEvent) -> String {
    sdk_event_payload(ev)
        .and_then(|(_, payload)| serde_json::to_string(&payload).ok())
        .unwrap_or_else(|| "{}".to_string())
}

pub fn sdk_event_batch_json<'a, I>(events: I) -> Option<(String, usize)>
where
    I: IntoIterator<Item = &'a RawSdkEvent>,
{
    let mut event_count = 0usize;
    let mut out = String::from("{\"events\":[");

    for event in events {
        let event_type = sdk_event_code(event.event());
        if event_type == crate::generated::event_codes::FLARE_EVENT_UNKNOWN {
            continue;
        }

        let payload = event.cached_json(sdk_event_json);
        if event_count > 0 {
            out.push(',');
        }
        write!(
            &mut out,
            "{{\"eventType\":{event_type},\"payload\":{}",
            payload.as_ref()
        )
        .expect("writing to String cannot fail");
        out.push('}');
        event_count += 1;
    }

    if event_count == 0 {
        return None;
    }

    out.push_str("]}");
    Some((out, event_count))
}

pub fn platform_event_bridge_resync_marker(dropped_events: u64) -> SdkEvent {
    SdkEvent::Sync(SyncNotify::ResyncNeeded {
        scope: "platform_event_bridge".to_string(),
        reason: "platform_event_bridge_lagged".to_string(),
        dropped_events,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flare_im_core_sdk::EventBus;
    use flare_proto::common::SendAck;

    #[test]
    fn send_ack_event_uses_client_msg_id_when_ack_id_is_absent() {
        let payload = send_ack_json(&SendAck {
            client_msg_id: "client-1".to_string(),
            conversation_id: "conversation-1".to_string(),
            ack_id: None,
            result: None,
        });

        assert_eq!(payload["ackId"], "client-1");
        assert_ne!(payload["ackId"], serde_json::Value::Null);
    }

    #[test]
    fn platform_event_bridge_resync_marker_uses_canonical_contract_event() {
        let event = platform_event_bridge_resync_marker(3);
        let (channel, payload) =
            sdk_event_channel_payload(&event).expect("resync marker should map to channel");

        assert_eq!(channel, "im://resync_needed");
        assert_eq!(
            payload.get("scope").and_then(Value::as_str),
            Some("platform_event_bridge")
        );
        assert_eq!(
            payload.get("reason").and_then(Value::as_str),
            Some("platform_event_bridge_lagged")
        );
        assert_eq!(
            payload.get("droppedEvents").and_then(Value::as_u64),
            Some(3)
        );
    }

    #[test]
    fn sdk_event_batch_json_uses_stable_event_codes_and_payloads() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe_shared_raw();

        bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));
        bus.publish(SdkEvent::Connection(ConnectionEvent::Disconnected {
            reason: "network".to_string(),
        }));

        let first = rx.try_recv().expect("first event");
        let second = rx.try_recv().expect("second event");
        let (json, event_count) =
            sdk_event_batch_json([first.as_ref(), second.as_ref()]).expect("batch json");

        assert_eq!(event_count, 2);
        let value: Value = serde_json::from_str(&json).expect("batch JSON parses");
        let events = value
            .get("events")
            .and_then(Value::as_array)
            .expect("events array");
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].get("eventType").and_then(Value::as_i64),
            Some(1001)
        );
        assert_eq!(
            events[1].get("eventType").and_then(Value::as_i64),
            Some(1002)
        );
        assert_eq!(
            events[1]
                .get("payload")
                .and_then(|payload| payload.get("reason"))
                .and_then(Value::as_str),
            Some("network")
        );
    }
}
