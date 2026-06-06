//! SdkEvent → contract C JSON + stable event codes (shared by C / Tauri / wasm).

use flare_im_core_sdk::core::event::SdkEvent;
use serde_json::Value;

pub use crate::generated::event_codes::*;

/// 事件类型转换
pub fn sdk_event_code(event: &flare_im_core_sdk::core::event::SdkEvent) -> i32 {
    use flare_im_core_sdk::core::event::{
        ConnectionEvent, ConversationEvent, MessageEvent, NotificationEvent, SdkEvent, SyncNotify,
    };

    match event {
        SdkEvent::Connection(ConnectionEvent::Connected) => FLARE_EVENT_CONNECTION_CONNECTED,
        SdkEvent::Connection(ConnectionEvent::Disconnected { .. }) => {
            FLARE_EVENT_CONNECTION_DISCONNECTED
        }
        SdkEvent::Connection(ConnectionEvent::Reconnecting { .. }) => {
            FLARE_EVENT_CONNECTION_RECONNECTING
        }
        SdkEvent::Connection(ConnectionEvent::StateChanged { .. }) => {
            FLARE_EVENT_CONNECTION_STATE_CHANGED
        }
        SdkEvent::Connection(ConnectionEvent::SyncStateChanged { .. }) => {
            FLARE_EVENT_CONNECTION_SYNC_STATE_CHANGED
        }
        SdkEvent::Connection(ConnectionEvent::ServerError { .. }) => {
            FLARE_EVENT_CONNECTION_SERVER_ERROR
        }
        SdkEvent::Connection(ConnectionEvent::KickedOff { .. }) => {
            FLARE_EVENT_CONNECTION_KICKED_OFF
        }
        SdkEvent::Connection(ConnectionEvent::TokenExpired { .. }) => {
            FLARE_EVENT_CONNECTION_TOKEN_EXPIRED
        }
        SdkEvent::Message(MessageEvent::Received { .. }) => FLARE_EVENT_MESSAGE_RECEIVED,
        SdkEvent::Message(MessageEvent::ReceivedBatch { .. }) => FLARE_EVENT_MESSAGE_RECEIVED_BATCH,
        SdkEvent::Message(MessageEvent::SendAck { .. }) => FLARE_EVENT_MESSAGE_SEND_ACK,
        SdkEvent::Message(MessageEvent::SendFailed { .. }) => FLARE_EVENT_MESSAGE_SEND_FAILED,
        SdkEvent::Message(MessageEvent::Recalled { .. }) => FLARE_EVENT_MESSAGE_RECALLED,
        SdkEvent::Message(MessageEvent::Typing { .. }) => FLARE_EVENT_MESSAGE_TYPING,
        SdkEvent::Message(MessageEvent::Edited { .. }) => FLARE_EVENT_MESSAGE_EDITED,
        SdkEvent::Message(MessageEvent::ReactionChanged { .. }) => {
            FLARE_EVENT_MESSAGE_REACTION_CHANGED
        }
        SdkEvent::Message(MessageEvent::Deleted { .. }) => FLARE_EVENT_MESSAGE_DELETED,
        SdkEvent::Message(MessageEvent::ReadReceipt { .. }) => FLARE_EVENT_MESSAGE_READ_RECEIPT,
        SdkEvent::Message(MessageEvent::BurnScheduled { .. }) => FLARE_EVENT_MESSAGE_BURN_SCHEDULED,
        SdkEvent::Message(MessageEvent::Burned { .. }) => FLARE_EVENT_MESSAGE_BURNED,
        SdkEvent::Message(MessageEvent::HardDeleted { .. }) => FLARE_EVENT_MESSAGE_HARD_DELETED,
        SdkEvent::Message(MessageEvent::Pinned { .. }) => FLARE_EVENT_MESSAGE_PINNED,
        SdkEvent::Message(MessageEvent::Unpinned { .. }) => FLARE_EVENT_MESSAGE_UNPINNED,
        SdkEvent::Message(MessageEvent::Marked { .. }) => FLARE_EVENT_MESSAGE_MARKED,
        SdkEvent::Message(MessageEvent::Unmarked { .. }) => FLARE_EVENT_MESSAGE_UNMARKED,
        SdkEvent::Message(MessageEvent::PresenceChanged { .. }) => {
            FLARE_EVENT_MESSAGE_PRESENCE_CHANGED
        }
        SdkEvent::Message(MessageEvent::CallSignal { .. }) => FLARE_EVENT_MESSAGE_CALL_SIGNAL,
        SdkEvent::Message(MessageEvent::Custom { .. }) => FLARE_EVENT_MESSAGE_CUSTOM,
        SdkEvent::Notification(NotificationEvent::Received { .. }) => {
            FLARE_EVENT_NOTIFICATION_RECEIVED
        }
        SdkEvent::Conversation(ConversationEvent::Synced { .. }) => FLARE_EVENT_CONVERSATION_SYNCED,
        SdkEvent::Conversation(ConversationEvent::Created { .. }) => {
            FLARE_EVENT_CONVERSATION_CREATED
        }
        SdkEvent::Conversation(ConversationEvent::Updated { .. }) => {
            FLARE_EVENT_CONVERSATION_UPDATED
        }
        SdkEvent::Conversation(ConversationEvent::UnreadCountChanged { .. }) => {
            FLARE_EVENT_CONVERSATION_UNREAD_COUNT_CHANGED
        }
        SdkEvent::Conversation(ConversationEvent::Deleted { .. }) => {
            FLARE_EVENT_CONVERSATION_DELETED
        }
        SdkEvent::Sync(SyncNotify::Started { .. }) => FLARE_EVENT_SYNC_STARTED,
        SdkEvent::Sync(SyncNotify::Finished { .. }) => FLARE_EVENT_SYNC_FINISHED,
        SdkEvent::Sync(SyncNotify::Failed { .. }) => FLARE_EVENT_SYNC_FAILED,
        SdkEvent::Sync(SyncNotify::Progress { .. }) => FLARE_EVENT_SYNC_PROGRESS,
        SdkEvent::Sync(SyncNotify::TaskCompleted { .. }) => FLARE_EVENT_SYNC_TASK_COMPLETED,
        SdkEvent::Sync(SyncNotify::StateChanged { .. }) => FLARE_EVENT_SYNC_STATE_CHANGED,
        SdkEvent::Extension(_) => FLARE_EVENT_EXTENSION,
    }
}

/// 将事件转换为 JSON 字符串（手动序列化）
pub fn sdk_event_json(event: &flare_im_core_sdk::core::event::SdkEvent) -> String {
    use flare_im_core_sdk::core::event::{
        ConnectionEvent, ConversationEvent, MessageEvent, NotificationEvent, SdkEvent, SyncNotify,
    };

    match event {
        SdkEvent::Connection(ConnectionEvent::Connected) => serde_json::json!({
            "type": "connection",
            "event": "connected",
        })
        .to_string(),
        SdkEvent::Connection(ConnectionEvent::Disconnected { reason }) => serde_json::json!({
            "type": "connection",
            "event": "disconnected",
            "reason": reason,
        })
        .to_string(),
        SdkEvent::Connection(ConnectionEvent::Reconnecting { attempt }) => serde_json::json!({
            "type": "connection",
            "event": "reconnecting",
            "attempt": attempt,
        })
        .to_string(),
        SdkEvent::Connection(ConnectionEvent::StateChanged { state }) => serde_json::json!({
            "type": "connection",
            "event": "state_changed",
            "state": format!("{state:?}"),
        })
        .to_string(),
        SdkEvent::Connection(ConnectionEvent::SyncStateChanged { state }) => serde_json::json!({
            "type": "connection",
            "event": "sync_state_changed",
            "state": format!("{state:?}"),
        })
        .to_string(),
        SdkEvent::Connection(ConnectionEvent::ServerError { code, message }) => serde_json::json!({
            "type": "connection",
            "event": "server_error",
            "code": code,
            "message": message,
        })
        .to_string(),
        SdkEvent::Connection(ConnectionEvent::KickedOff { reason }) => serde_json::json!({
            "type": "connection",
            "event": "kicked_off",
            "reason": reason,
        })
        .to_string(),
        SdkEvent::Connection(ConnectionEvent::TokenExpired { message }) => serde_json::json!({
            "type": "connection",
            "event": "token_expired",
            "message": message,
        })
        .to_string(),
        SdkEvent::Message(MessageEvent::Received { message }) => {
            match serde_json::to_string(message) {
                Ok(msg_json) => format!(
                    r#"{{"type":"message","event":"received","message":{}}}"#,
                    msg_json
                ),
                Err(_) => r#"{"type":"message","event":"received","error":"serialize_failed"}"#
                    .to_string(),
            }
        }
        SdkEvent::Message(MessageEvent::ReceivedBatch { messages }) => {
            match serde_json::to_string(messages) {
                Ok(arr_json) => format!(
                    r#"{{"type":"message","event":"received_batch","messages":{}}}"#,
                    arr_json
                ),
                Err(_) => {
                    r#"{"type":"message","event":"received_batch","error":"serialize_failed"}"#
                        .to_string()
                }
            }
        }
        SdkEvent::Message(MessageEvent::SendAck { ack }) => serde_json::json!({
            "type": "message",
            "event": "send_ack",
            "ack": {
                "client_msg_id": ack.client_msg_id,
                "server_msg_id": ack.server_msg_id,
                "seq": ack.seq,
                "conversation_id": ack.conversation_id,
                "success": ack.success,
                "error_code": ack.error_code,
                "error_message": ack.error_message,
                "ack_id": ack.ack_id,
            }
        })
        .to_string(),
        SdkEvent::Message(MessageEvent::SendFailed {
            client_msg_id,
            reason,
        }) => serde_json::json!({
            "type": "message",
            "event": "send_failed",
            "client_msg_id": client_msg_id,
            "reason": reason,
        })
        .to_string(),
        SdkEvent::Message(MessageEvent::Typing {
            conversation_id,
            event,
        }) => serde_json::json!({
            "type": "message",
            "event": "typing",
            "conversation_id": conversation_id,
            "user_id": event.user_id,
            "typing": event.typing,
        })
        .to_string(),
        SdkEvent::Message(MessageEvent::PresenceChanged {
            conversation_id,
            event,
        }) => serde_json::json!({
            "type": "message",
            "event": "presence_changed",
            "conversation_id": conversation_id,
            "user_id": event.user_id,
            "status": event.status,
            "extra": event.extra,
        })
        .to_string(),
        SdkEvent::Message(MessageEvent::Recalled {
            conversation_id,
            event,
        }) => serde_json::json!({
            "type": "message",
            "event": "recalled",
            "conversation_id": conversation_id,
            "server_msg_id": event.server_msg_id,
            "reason": event.reason,
            "time_limit_seconds": event.time_limit_seconds,
            "allow_admin_recall": event.allow_admin_recall,
        })
        .to_string(),
        SdkEvent::Message(MessageEvent::Edited {
            conversation_id,
            server_msg_id,
            edit_version,
        }) => serde_json::json!({
            "type": "message",
            "event": "edited",
            "conversation_id": conversation_id,
            "server_msg_id": server_msg_id,
            "edit_version": edit_version,
        })
        .to_string(),
        SdkEvent::Message(MessageEvent::Deleted {
            conversation_id,
            event,
        }) => serde_json::json!({
            "type": "message",
            "event": "deleted",
            "conversation_id": conversation_id,
            "server_msg_id": event.server_msg_id,
            "delete_type": event.delete_type,
            "reason": event.reason,
            "notify_others": event.notify_others,
            "scope": event.scope,
            "target_user_id": event.target_user_id,
        })
        .to_string(),
        SdkEvent::Message(MessageEvent::ReadReceipt {
            conversation_id,
            event,
        }) => serde_json::json!({
            "type": "message",
            "event": "read_receipt",
            "conversation_id": conversation_id,
            "user_id": event.user_id,
            "read_seq": event.read_seq,
            "message_ids": event.message_ids,
            "burn_after_read": event.burn_after_read,
        })
        .to_string(),
        SdkEvent::Message(MessageEvent::BurnScheduled {
            conversation_id,
            event,
        }) => serde_json::json!({
            "type": "message",
            "event": "burn_scheduled",
            "conversation_id": conversation_id,
            "tenant_id": event.tenant_id,
            "message_id": event.message_id,
            "server_id": event.server_id,
            "seq": event.seq,
            "reader_id": event.reader_id,
            "burn_at": event.burn_at,
            "event_time": event.event_time,
        })
        .to_string(),
        SdkEvent::Message(MessageEvent::Burned {
            conversation_id,
            event,
        }) => serde_json::json!({
            "type": "message",
            "event": "burned",
            "conversation_id": conversation_id,
            "tenant_id": event.tenant_id,
            "message_id": event.message_id,
            "server_id": event.server_id,
            "seq": event.seq,
            "reader_id": event.reader_id,
            "burn_at": event.burn_at,
            "burned_at": event.burned_at,
            "event_time": event.event_time,
        })
        .to_string(),
        SdkEvent::Message(MessageEvent::HardDeleted {
            conversation_id,
            event,
        }) => serde_json::json!({
            "type": "message",
            "event": "hard_deleted",
            "conversation_id": conversation_id,
            "tenant_id": event.tenant_id,
            "message_id": event.message_id,
            "server_id": event.server_id,
            "seq": event.seq,
            "reader_id": event.reader_id,
            "burn_at": event.burn_at,
            "burned_at": event.burned_at,
            "event_time": event.event_time,
        })
        .to_string(),
        SdkEvent::Message(MessageEvent::ReactionChanged {
            conversation_id,
            server_msg_id,
            user_id,
            emoji,
            action,
        }) => serde_json::json!({
            "type": "message",
            "event": "reaction_changed",
            "conversation_id": conversation_id,
            "server_msg_id": server_msg_id,
            "user_id": user_id,
            "emoji": emoji,
            "action": action,
        })
        .to_string(),
        SdkEvent::Message(MessageEvent::Pinned {
            conversation_id,
            event,
        }) => serde_json::json!({
            "type": "message",
            "event": "pinned",
            "conversation_id": conversation_id,
            "server_msg_id": event.server_msg_id,
            "pinned_by": event.pinned_by,
            "reason": event.reason,
            "expire_at": event.expire_at.as_ref().map(|t| t.seconds),
        })
        .to_string(),
        SdkEvent::Message(MessageEvent::Unpinned {
            conversation_id,
            event,
        }) => serde_json::json!({
            "type": "message",
            "event": "unpinned",
            "conversation_id": conversation_id,
            "server_msg_id": event.server_msg_id,
        })
        .to_string(),
        SdkEvent::Message(MessageEvent::Marked {
            conversation_id,
            event,
        }) => serde_json::json!({
            "type": "message",
            "event": "marked",
            "conversation_id": conversation_id,
            "server_msg_id": event.server_msg_id,
            "user_id": event.user_id,
            "mark_type": event.mark_type,
            "color": event.color,
        })
        .to_string(),
        SdkEvent::Message(MessageEvent::Unmarked {
            conversation_id,
            event,
        }) => serde_json::json!({
            "type": "message",
            "event": "unmarked",
            "conversation_id": conversation_id,
            "server_msg_id": event.server_msg_id,
            "user_id": event.user_id,
            "mark_type": event.mark_type,
        })
        .to_string(),
        SdkEvent::Message(MessageEvent::CallSignal {
            conversation_id,
            event,
        }) => call_signal_to_json(conversation_id, event),
        SdkEvent::Message(MessageEvent::Custom {
            conversation_id,
            event,
        }) => serde_json::json!({
            "type": "message",
            "event": "custom",
            "conversation_id": conversation_id,
            "namespace": event.namespace,
            "name": event.name,
            "version": event.version,
            "payload": event.payload,
            "metadata": event.metadata,
        })
        .to_string(),
        SdkEvent::Notification(NotificationEvent::Received { message }) => {
            match serde_json::to_string(message) {
                Ok(msg_json) => format!(
                    r#"{{"type":"notification","event":"received","message":{}}}"#,
                    msg_json
                ),
                Err(_) => {
                    r#"{"type":"notification","event":"received","error":"serialize_failed"}"#
                        .to_string()
                }
            }
        }
        SdkEvent::Conversation(ConversationEvent::UnreadCountChanged {
            conversation_id,
            unread_count,
        }) => serde_json::json!({
            "type": "conversation",
            "event": "unread_count_changed",
            "conversation_id": conversation_id,
            "unread_count": unread_count,
        })
        .to_string(),
        SdkEvent::Conversation(ConversationEvent::Created { conversation_id }) => {
            serde_json::json!({
                "type": "conversation",
                "event": "created",
                "conversation_id": conversation_id,
            })
            .to_string()
        }
        SdkEvent::Conversation(ConversationEvent::Updated { conversation_id }) => {
            serde_json::json!({
                "type": "conversation",
                "event": "updated",
                "conversation_id": conversation_id,
            })
            .to_string()
        }
        SdkEvent::Conversation(ConversationEvent::Deleted { conversation_id }) => {
            serde_json::json!({
                "type": "conversation",
                "event": "deleted",
                "conversation_id": conversation_id,
            })
            .to_string()
        }
        SdkEvent::Conversation(ConversationEvent::Synced { conversation_ids }) => {
            match serde_json::to_string(conversation_ids) {
                Ok(ids) => format!(
                    r#"{{"type":"conversation","event":"synced","conversation_ids":{}}}"#,
                    ids
                ),
                Err(_) => {
                    r#"{"type":"conversation","event":"synced","conversation_ids":[]}"#.to_string()
                }
            }
        }
        SdkEvent::Sync(SyncNotify::StateChanged { run, state }) => serde_json::json!({
            "type": "sync",
            "event": "state_changed",
            "run_id": run.run_id,
            "trigger": run.trigger.as_str(),
            "scope": run.scope.as_str(),
            "visibility": run.visibility.as_str(),
            "reason": run.reason.as_str(),
            "state": format!("{state:?}"),
        })
        .to_string(),
        SdkEvent::Sync(SyncNotify::Started { run }) => serde_json::json!({
            "type": "sync",
            "event": "started",
            "run_id": run.run_id,
            "trigger": run.trigger.as_str(),
            "scope": run.scope.as_str(),
            "visibility": run.visibility.as_str(),
            "reason": run.reason.as_str(),
        })
        .to_string(),
        SdkEvent::Sync(SyncNotify::Finished { run, phase }) => serde_json::json!({
            "type": "sync",
            "event": "finished",
            "run_id": run.run_id,
            "trigger": run.trigger.as_str(),
            "scope": run.scope.as_str(),
            "visibility": run.visibility.as_str(),
            "reason": run.reason.as_str(),
            "phase": sync_phase_to_str(phase),
        })
        .to_string(),
        SdkEvent::Sync(SyncNotify::Failed { run, task, message }) => serde_json::json!({
            "type": "sync",
            "event": "failed",
            "run_id": run.run_id,
            "trigger": run.trigger.as_str(),
            "scope": run.scope.as_str(),
            "visibility": run.visibility.as_str(),
            "reason": run.reason.as_str(),
            "task": task,
            "error": {
                "code": "sync_failed",
                "message": message,
                "operation": task,
                "retryable": true
            }
        })
        .to_string(),
        SdkEvent::Sync(SyncNotify::Progress {
            run,
            task,
            progress,
            detail,
        }) => serde_json::json!({
            "type": "sync",
            "event": "progress",
            "run_id": run.run_id,
            "trigger": run.trigger.as_str(),
            "scope": run.scope.as_str(),
            "visibility": run.visibility.as_str(),
            "reason": run.reason.as_str(),
            "task": task,
            "progress": (*progress * 100.0).round() as i32,
            "detail": detail,
        })
        .to_string(),
        SdkEvent::Sync(SyncNotify::TaskCompleted { run, task }) => serde_json::json!({
            "type": "sync",
            "event": "task_completed",
            "run_id": run.run_id,
            "trigger": run.trigger.as_str(),
            "scope": run.scope.as_str(),
            "visibility": run.visibility.as_str(),
            "reason": run.reason.as_str(),
            "task": task,
        })
        .to_string(),
        SdkEvent::Extension(event) => serde_json::json!({
            "type": "extension",
            "event": "extension",
            "source": event.source,
            "event_type": event.event_type,
            "payload": event.payload,
        })
        .to_string(),
    }
}

fn sync_phase_to_str(phase: &flare_im_core_sdk::core::event::SyncPhase) -> &'static str {
    match phase {
        flare_im_core_sdk::core::event::SyncPhase::Init => "Init",
        flare_im_core_sdk::core::event::SyncPhase::Background => "Background",
    }
}

fn call_signal_to_json(
    conversation_id: &str,
    event: &flare_proto::common::CallSignalEvent,
) -> String {
    let cid = if event.conversation_id.trim().is_empty() {
        conversation_id.to_string()
    } else {
        event.conversation_id.clone()
    };
    serde_json::json!({
        "type": "message",
        "event": "call_signal",
        "conversation_id": cid,
        "call_id": event.call_id,
        "from_user_id": event.from_user_id,
        "to_user_id": direct_peer_user_id(event.audience.as_ref()),
        "audience": audience_to_json(event.audience.as_ref()),
        "media_session": media_session_to_json(event.media_session.as_ref()),
        "transport": transport_to_json(event.transport.as_ref()),
        "invite_expires_at_unix": event.invite_deadline.as_ref().map(|t| t.seconds),
        "ext": event.ext,
        "variant": call_signal_variant_name(&event.signal),
        "body": call_signal_body_json(&event.signal),
    })
    .to_string()
}

fn direct_peer_user_id(a: Option<&flare_proto::common::CallAudience>) -> Option<String> {
    match a.and_then(|a| a.shape.as_ref()) {
        Some(flare_proto::common::call_audience::Shape::Direct(d))
            if !d.peer_user_id.trim().is_empty() =>
        {
            Some(d.peer_user_id.clone())
        }
        _ => None,
    }
}

fn audience_to_json(a: Option<&flare_proto::common::CallAudience>) -> serde_json::Value {
    match a.and_then(|a| a.shape.as_ref()) {
        Some(flare_proto::common::call_audience::Shape::Direct(d)) => {
            serde_json::json!({ "direct": { "peer_user_id": d.peer_user_id } })
        }
        Some(flare_proto::common::call_audience::Shape::Explicit(e)) => {
            serde_json::json!({ "explicit": { "user_ids": e.user_ids } })
        }
        Some(flare_proto::common::call_audience::Shape::Broadcast(_)) => {
            serde_json::json!({ "broadcast": {} })
        }
        None => serde_json::Value::Null,
    }
}

fn media_session_to_json(
    m: Option<&flare_proto::common::CallMediaSessionInfo>,
) -> serde_json::Value {
    let Some(m) = m else {
        return serde_json::Value::Null;
    };
    serde_json::json!({
        "kind": m.kind,
        "organizer_user_id": m.organizer_user_id,
        "title": m.title,
        "scheduled_start": m.scheduled_start.as_ref().map(|t| t.seconds),
    })
}

fn transport_to_json(t: Option<&flare_proto::common::SfuTransportContext>) -> serde_json::Value {
    let Some(t) = t else {
        return serde_json::Value::Null;
    };
    serde_json::json!({
        "room_id": t.room_id,
        "peer_id": t.peer_id,
        "media_session_id": t.media_session_id,
        "track_id": t.track_id,
        "signaling_ws_base": t.signaling_ws_base,
        "instance_id": t.instance_id,
    })
}

fn call_signal_variant_name(
    signal: &Option<flare_proto::common::call_signal_event::Signal>,
) -> &'static str {
    use flare_proto::common::call_signal_event::Signal;
    match signal {
        Some(Signal::Invite(_)) => "invite",
        Some(Signal::Accept(_)) => "accept",
        Some(Signal::Reject(_)) => "reject",
        Some(Signal::Hangup(_)) => "hangup",
        Some(Signal::IceCandidate(_)) => "ice_candidate",
        Some(Signal::Ringing(_)) => "ringing",
        Some(Signal::Busy(_)) => "busy",
        Some(Signal::Renegotiate(_)) => "renegotiate",
        Some(Signal::SfuRoom(_)) => "sfu_room",
        Some(Signal::SfuPeerJoined(_)) => "sfu_peer_joined",
        Some(Signal::SfuPeerLeft(_)) => "sfu_peer_left",
        Some(Signal::SfuTrackPublished(_)) => "sfu_track_published",
        Some(Signal::SfuTrackUnpublished(_)) => "sfu_track_unpublished",
        Some(Signal::SfuSubscribed(_)) => "sfu_subscribed",
        Some(Signal::SfuUnsubscribed(_)) => "sfu_unsubscribed",
        Some(Signal::SfuJoinHints(_)) => "sfu_join_hints",
        Some(Signal::SfuSubscription(_)) => "sfu_subscription",
        Some(Signal::SfuAudioLevel(_)) => "sfu_audio_level",
        Some(Signal::SfuNetworkQuality(_)) => "sfu_network_quality",
        Some(Signal::SfuBweHint(_)) => "sfu_bwe_hint",
        Some(Signal::InviteeUpdate(_)) => "invitee_update",
        None => "unspecified",
    }
}

fn offered_media_json(m: Option<&flare_proto::common::CallOfferedMedia>) -> serde_json::Value {
    let Some(m) = m else {
        return serde_json::Value::Null;
    };
    serde_json::json!({
        "types": m.types,
        "primary_source": m.primary_source,
        "codec_hint": m.codec_hint,
    })
}

fn track_state_json(t: &flare_proto::common::SfuTrackSnapshot) -> serde_json::Value {
    serde_json::json!({
        "track_id": t.track_id,
        "media_type": t.media_type,
        "source": t.source,
        "codec": t.codec,
    })
}

fn room_snapshot_json(s: &flare_proto::common::SfuRoomSnapshot) -> serde_json::Value {
    serde_json::json!({
        "room_id": s.room_id,
        "peer_count": s.peer_count,
        "track_count": s.track_count,
        "exists": s.exists,
        "draining": s.draining,
    })
}

fn peer_snapshot_json(p: &flare_proto::common::SfuPeerSnapshot) -> serde_json::Value {
    serde_json::json!({
        "peer_id": p.peer_id,
        "user_id": p.user_id,
        "published_track_ids": p.published_track_ids,
        "subscribed_track_ids": p.subscribed_track_ids,
        "media_session_id": p.media_session_id,
        "sfu_role": p.sfu_role,
        "roster_role": p.roster_role,
    })
}

fn call_end_reason_code_label(raw: Option<i32>) -> Option<&'static str> {
    match raw? {
        1 => Some("user_hangup"),
        2 => Some("rejected"),
        3 => Some("cancelled"),
        4 => Some("no_answer_timeout"),
        5 => Some("busy"),
        6 => Some("failed"),
        _ => None,
    }
}

fn call_visibility_scope_label(raw: Option<i32>) -> Option<&'static str> {
    match raw? {
        1 => Some("all_participants"),
        2 => Some("self_only"),
        _ => None,
    }
}

fn call_signal_body_json(
    signal: &Option<flare_proto::common::call_signal_event::Signal>,
) -> serde_json::Value {
    use flare_proto::common::call_signal_event::Signal;
    match signal {
        None => serde_json::Value::Null,
        Some(Signal::Invite(i)) => serde_json::json!({
            "invite": { "offered_media": offered_media_json(i.offered_media.as_ref()) }
        }),
        Some(Signal::Accept(a)) => serde_json::json!({
            "accept": { "accepted_media": offered_media_json(a.accepted_media.as_ref()) }
        }),
        Some(Signal::Reject(r)) => {
            serde_json::json!({ "reject": { "reason": r.reason, "code": r.code } })
        }
        Some(Signal::Hangup(h)) => serde_json::json!({
            "hangup": {
                "reason": h.reason,
                "duration_seconds": h.duration_seconds,
                "close_room_if_vacant": h.close_room_if_vacant,
                "reason_code": call_end_reason_code_label(h.reason_code),
                "visibility_scope": call_visibility_scope_label(h.visibility_scope),
                "timeout_seconds": h.timeout_seconds,
            }
        }),
        Some(Signal::Ringing(_)) => serde_json::json!({ "ringing": {} }),
        Some(Signal::Busy(b)) => serde_json::json!({ "busy": { "reason": b.reason } }),
        Some(Signal::Renegotiate(r)) => {
            serde_json::json!({ "renegotiate": { "want_media": r.want_media } })
        }
        Some(Signal::IceCandidate(c)) => serde_json::json!({
            "ice_candidate": {
                "candidate": c.candidate,
                "sdp_mid": c.sdp_mid,
                "sdp_mline_index": c.sdp_mline_index,
                "candidate_json": c.candidate_json,
            }
        }),
        Some(Signal::SfuRoom(s)) => serde_json::json!({ "sfu_room": room_snapshot_json(s) }),
        Some(Signal::SfuPeerJoined(p)) => {
            serde_json::json!({ "sfu_peer_joined": peer_snapshot_json(p) })
        }
        Some(Signal::SfuPeerLeft(p)) => serde_json::json!({
            "sfu_peer_left": { "peer_id": p.peer_id, "user_id": p.user_id }
        }),
        Some(Signal::SfuTrackPublished(t)) => {
            serde_json::json!({ "sfu_track_published": track_state_json(t) })
        }
        Some(Signal::SfuTrackUnpublished(t)) => {
            serde_json::json!({ "sfu_track_unpublished": track_state_json(t) })
        }
        Some(Signal::SfuSubscribed(r)) => {
            serde_json::json!({ "sfu_subscribed": { "track_id": r.track_id } })
        }
        Some(Signal::SfuUnsubscribed(r)) => {
            serde_json::json!({ "sfu_unsubscribed": { "track_id": r.track_id } })
        }
        Some(Signal::SfuJoinHints(h)) => serde_json::json!({
            "sfu_join_hints": {
                "token": h.token,
                "ttl_seconds": h.ttl_seconds,
                "room_id": h.room_id,
                "peer_id": h.peer_id,
                "media_session_id": h.media_session_id,
                "signaling_ws_base": h.signaling_ws_base,
                "instance_id": h.instance_id,
            }
        }),
        Some(Signal::SfuSubscription(s)) => serde_json::json!({
            "sfu_subscription": {
                "subscriber_peer_id": s.subscriber_peer_id,
                "track_id": s.track_id,
                "enabled": s.enabled,
                "media": s.media,
                "preferred_layer": s.preferred_layer,
                "priority": s.priority,
            }
        }),
        Some(Signal::SfuAudioLevel(a)) => serde_json::json!({
            "sfu_audio_level": {
                "peer_id": a.peer_id,
                "user_id": a.user_id,
                "linear_level": a.linear_level,
                "voice_active": a.voice_active,
            }
        }),
        Some(Signal::SfuNetworkQuality(n)) => serde_json::json!({
            "sfu_network_quality": {
                "peer_id": n.peer_id,
                "upstream_score": n.upstream_score,
                "downstream_score": n.downstream_score,
                "rtt_ms": n.rtt_ms,
                "packet_loss_ratio": n.packet_loss_ratio,
            }
        }),
        Some(Signal::SfuBweHint(h)) => serde_json::json!({
            "sfu_bwe_hint": {
                "peer_id": h.peer_id,
                "congested": h.congested,
                "suggested_cap_kbps": h.suggested_cap_kbps,
            }
        }),
        Some(Signal::InviteeUpdate(u)) => serde_json::json!({
            "invitee_update": {
                "added_user_ids": u.added_user_ids,
                "removed_user_ids": u.removed_user_ids,
            }
        }),
    }
}

/// Parsed JSON payload for web/Tauri emit adapters.
pub fn sdk_event_json_value(event: &SdkEvent) -> Value {
    serde_json::from_str(&sdk_event_json(event)).unwrap_or(Value::Null)
}
