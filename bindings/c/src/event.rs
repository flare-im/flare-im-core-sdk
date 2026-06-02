//! 事件 API - 事件订阅和监听
//!
//! 统一事件总线,支持所有平台

use std::ffi::c_void;
use std::sync::Arc;

use crate::abi;
use crate::helpers::string_to_flare;
use crate::registry::{SdkInstance, require_instance};
use crate::types::{FlareEventCallback, FlareHandle, FlareSubscriptionHandle};
use dashmap::DashMap;
use tokio::sync::oneshot;
use tokio::time::{Duration, sleep};

/// 事件类型码
pub const FLARE_EVENT_CONNECTION_CONNECTED: i32 = 1001;
pub const FLARE_EVENT_CONNECTION_DISCONNECTED: i32 = 1002;
/// 与 Tauri `im://reconnecting` 对齐：SDK 自动重连尝试中
pub const FLARE_EVENT_CONNECTION_RECONNECTING: i32 = 1003;
pub const FLARE_EVENT_MESSAGE_RECEIVED: i32 = 2001;
pub const FLARE_EVENT_MESSAGE_SEND_ACK: i32 = 2002;
pub const FLARE_EVENT_CONVERSATION_UPDATED: i32 = 3001;
pub const FLARE_EVENT_SYNC_UPDATED: i32 = 4001;

lazy_static::lazy_static! {
    static ref EVENT_SUBSCRIPTIONS: DashMap<u64, oneshot::Sender<()>> = DashMap::new();
}

pub(crate) fn unsubscribe_all_events() {
    let keys: Vec<u64> = EVENT_SUBSCRIPTIONS.iter().map(|e| *e.key()).collect();
    for key in keys {
        if let Some((_, tx)) = EVENT_SUBSCRIPTIONS.remove(&key) {
            let _ = tx.send(());
        }
    }
}

/// 事件类型转换
fn event_type_to_code(event: &flare_im_core_sdk::event::SdkEvent) -> i32 {
    use flare_im_core_sdk::event::{ConnectionEvent, ConversationEvent, MessageEvent, SdkEvent};

    match event {
        SdkEvent::Connection(ConnectionEvent::Connected) => FLARE_EVENT_CONNECTION_CONNECTED,
        SdkEvent::Connection(ConnectionEvent::Disconnected { .. }) => {
            FLARE_EVENT_CONNECTION_DISCONNECTED
        }
        SdkEvent::Connection(ConnectionEvent::Reconnecting { .. }) => {
            FLARE_EVENT_CONNECTION_RECONNECTING
        }
        SdkEvent::Message(MessageEvent::SendAck { .. }) => FLARE_EVENT_MESSAGE_SEND_ACK,
        SdkEvent::Message(MessageEvent::Received { .. })
        | SdkEvent::Message(MessageEvent::ReceivedBatch { .. })
        | SdkEvent::Message(MessageEvent::SendFailed { .. })
        | SdkEvent::Message(MessageEvent::Recalled { .. })
        | SdkEvent::Message(MessageEvent::Typing { .. })
        | SdkEvent::Message(MessageEvent::Edited { .. })
        | SdkEvent::Message(MessageEvent::ReactionChanged { .. })
        | SdkEvent::Message(MessageEvent::Deleted { .. })
        | SdkEvent::Message(MessageEvent::ReadReceipt { .. })
        | SdkEvent::Message(MessageEvent::Pinned { .. })
        | SdkEvent::Message(MessageEvent::Unpinned { .. })
        | SdkEvent::Message(MessageEvent::Marked { .. })
        | SdkEvent::Message(MessageEvent::Unmarked { .. })
        | SdkEvent::Message(MessageEvent::PresenceChanged { .. })
        | SdkEvent::Message(MessageEvent::CallSignal { .. })
        | SdkEvent::Message(MessageEvent::Custom { .. }) => FLARE_EVENT_MESSAGE_RECEIVED,
        SdkEvent::Conversation(ConversationEvent::Synced { .. })
        | SdkEvent::Conversation(ConversationEvent::Created { .. })
        | SdkEvent::Conversation(ConversationEvent::Updated { .. })
        | SdkEvent::Conversation(ConversationEvent::UnreadCountChanged { .. })
        | SdkEvent::Conversation(ConversationEvent::Deleted { .. }) => {
            FLARE_EVENT_CONVERSATION_UPDATED
        }
        SdkEvent::Sync(_) => FLARE_EVENT_SYNC_UPDATED,
        _ => 0, // 保留给未来扩展；不会在下游被丢弃
    }
}

/// 将事件转换为 JSON 字符串（手动序列化）
fn event_to_json(event: &flare_im_core_sdk::event::SdkEvent) -> String {
    use flare_im_core_sdk::event::{
        ConnectionEvent, ConversationEvent, MessageEvent, SdkEvent, SyncNotify,
    };

    match event {
        SdkEvent::Connection(ConnectionEvent::Connected) => {
            r#"{"type":"connection","event":"connected"}"#.to_string()
        }
        SdkEvent::Connection(ConnectionEvent::Disconnected { reason }) => {
            format!(
                r#"{{"type":"connection","event":"disconnected","reason":"{}"}}"#,
                reason.replace('"', "\\\"")
            )
        }
        SdkEvent::Connection(ConnectionEvent::Reconnecting { attempt }) => serde_json::json!({
            "type": "connection",
            "event": "reconnecting",
            "attempt": attempt,
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
        SdkEvent::Message(MessageEvent::SendAck { ack }) => {
            // 手动序列化 SendAck
            format!(
                r#"{{"type":"message","event":"send_ack","ack":{{"client_msg_id":"{}","server_msg_id":"{}","seq":{},"conversation_id":"{}","success":{}}}}}"#,
                ack.client_msg_id.replace('"', "\\\""),
                ack.server_msg_id.replace('"', "\\\""),
                ack.seq,
                ack.conversation_id.replace('"', "\\\""),
                ack.success
            )
        }
        SdkEvent::Message(MessageEvent::Typing {
            conversation_id,
            event,
        }) => {
            format!(
                r#"{{"type":"message","event":"typing","conversation_id":"{}","user_id":"{}","typing":{}}}"#,
                conversation_id.replace('"', "\\\""),
                event.user_id.replace('"', "\\\""),
                event.typing
            )
        }
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
        }) => {
            format!(
                r#"{{"type":"message","event":"recalled","conversation_id":"{}","message_id":"{}","server_msg_id":"{}","reason":"{}"}}"#,
                conversation_id.replace('"', "\\\""),
                event.server_msg_id.replace('"', "\\\""),
                event.server_msg_id.replace('"', "\\\""),
                event.reason.replace('"', "\\\"")
            )
        }
        SdkEvent::Message(MessageEvent::ReadReceipt {
            conversation_id,
            event,
        }) => {
            format!(
                r#"{{"type":"message","event":"read_receipt","conversation_id":"{}","user_id":"{}","read_seq":{}}}"#,
                conversation_id.replace('"', "\\\""),
                event.user_id.replace('"', "\\\""),
                event.read_seq
            )
        }
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
        SdkEvent::Message(MessageEvent::CallSignal {
            conversation_id,
            event,
        }) => call_signal_to_json(conversation_id, event),
        SdkEvent::Conversation(ConversationEvent::UnreadCountChanged {
            conversation_id,
            unread_count,
        }) => {
            format!(
                r#"{{"type":"conversation","event":"unread_count_changed","conversation_id":"{}","unread_count":{}}}"#,
                conversation_id.replace('"', "\\\""),
                unread_count
            )
        }
        SdkEvent::Conversation(ConversationEvent::Created { conversation_id }) => {
            format!(
                r#"{{"type":"conversation","event":"created","conversation_id":"{}"}}"#,
                conversation_id
            )
        }
        SdkEvent::Conversation(ConversationEvent::Updated { conversation_id }) => {
            format!(
                r#"{{"type":"conversation","event":"updated","conversation_id":"{}"}}"#,
                conversation_id
            )
        }
        SdkEvent::Conversation(ConversationEvent::Deleted { conversation_id }) => {
            format!(
                r#"{{"type":"conversation","event":"deleted","conversation_id":"{}"}}"#,
                conversation_id
            )
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
            "event": "stateChanged",
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
            "event": "taskCompleted",
            "run_id": run.run_id,
            "trigger": run.trigger.as_str(),
            "scope": run.scope.as_str(),
            "visibility": run.visibility.as_str(),
            "reason": run.reason.as_str(),
            "task": task,
        })
        .to_string(),
        _ => r#"{"type":"unknown"}"#.to_string(),
    }
}

fn sync_phase_to_str(phase: &flare_im_core_sdk::event::SyncPhase) -> &'static str {
    match phase {
        flare_im_core_sdk::event::SyncPhase::Init => "Init",
        flare_im_core_sdk::event::SyncPhase::Background => "Background",
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
            serde_json::json!({ "direct": { "peerUserId": d.peer_user_id } })
        }
        Some(flare_proto::common::call_audience::Shape::Explicit(e)) => {
            serde_json::json!({ "explicit": { "userIds": e.user_ids } })
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
        "organizerUserId": m.organizer_user_id,
        "title": m.title,
        "scheduledStart": m.scheduled_start.as_ref().map(|t| t.seconds),
    })
}

fn transport_to_json(t: Option<&flare_proto::common::SfuTransportContext>) -> serde_json::Value {
    let Some(t) = t else {
        return serde_json::Value::Null;
    };
    serde_json::json!({
        "roomId": t.room_id,
        "peerId": t.peer_id,
        "mediaSessionId": t.media_session_id,
        "trackId": t.track_id,
        "signalingWsBase": t.signaling_ws_base,
        "instanceId": t.instance_id,
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
        "primarySource": m.primary_source,
        "codecHint": m.codec_hint,
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
            "invite": { "offeredMedia": offered_media_json(i.offered_media.as_ref()) }
        }),
        Some(Signal::Accept(a)) => serde_json::json!({
            "accept": { "acceptedMedia": offered_media_json(a.accepted_media.as_ref()) }
        }),
        Some(Signal::Reject(r)) => {
            serde_json::json!({ "reject": { "reason": r.reason, "code": r.code } })
        }
        Some(Signal::Hangup(h)) => serde_json::json!({
            "hangup": {
                "reason": h.reason,
                "durationSeconds": h.duration_seconds,
                "closeRoomIfVacant": h.close_room_if_vacant,
                "reasonCode": call_end_reason_code_label(h.reason_code),
                "visibilityScope": call_visibility_scope_label(h.visibility_scope),
                "timeoutSeconds": h.timeout_seconds,
            }
        }),
        Some(Signal::Ringing(_)) => serde_json::json!({ "ringing": {} }),
        Some(Signal::Busy(b)) => serde_json::json!({ "busy": { "reason": b.reason } }),
        Some(Signal::Renegotiate(r)) => {
            serde_json::json!({ "renegotiate": { "wantMedia": r.want_media } })
        }
        Some(Signal::IceCandidate(c)) => serde_json::json!({
            "iceCandidate": {
                "candidate": c.candidate,
                "sdpMid": c.sdp_mid,
                "sdpMLineIndex": c.sdp_mline_index,
                "candidateJson": c.candidate_json,
            }
        }),
        Some(other) => {
            serde_json::json!({ "rawVariant": call_signal_variant_name(&Some(other.clone())) })
        }
    }
}

/// 订阅事件
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `context` - 用户上下文
/// * `callback` - 事件回调
///
/// # Returns
/// 订阅句柄,0 表示失败
#[unsafe(no_mangle)]
pub extern "C" fn flare_event_subscribe(
    handle: FlareHandle,
    context: *mut c_void,
    callback: FlareEventCallback,
) -> FlareSubscriptionHandle {
    abi::catch_ffi_subscription_handle(|| {
        subscribe_events_inner(handle, context, callback).unwrap_or_default()
    })
}

fn subscribe_events_inner(
    handle: FlareHandle,
    context: *mut c_void,
    callback: FlareEventCallback,
) -> Result<FlareSubscriptionHandle, i32> {
    let instance = require_instance(handle)?;

    // 创建取消通道
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();

    // 生成订阅 ID
    let subscription_id = {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_SUB_ID: AtomicU64 = AtomicU64::new(1);
        NEXT_SUB_ID.fetch_add(1, Ordering::SeqCst)
    };
    EVENT_SUBSCRIPTIONS.insert(subscription_id, cancel_tx);

    // 启动事件转发任务
    spawn_event_forwarder(
        instance,
        subscription_id,
        context as usize,
        callback,
        cancel_rx,
    );

    Ok(subscription_id)
}

/// 启动事件转发任务
fn spawn_event_forwarder(
    instance: Arc<SdkInstance>,
    subscription_id: u64,
    user_context: usize, // 使用 usize 代替 *mut c_void
    callback: FlareEventCallback,
    mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let client = instance.client.clone();

    instance.runtime.spawn(async move {
        // 等待事件总线可用，避免登录后瞬时竞态导致“订阅假成功、事件全丢”。
        let mut rx = loop {
            match client.bus().await {
                Ok(bus) => break bus.subscribe(),
                Err(e) => {
                    tracing::warn!(error = %e, "event bus not ready yet, retrying");
                    tokio::select! {
                        _ = &mut cancel_rx => {
                            tracing::debug!("Event subscription cancelled before bus ready");
                            EVENT_SUBSCRIPTIONS.remove(&subscription_id);
                            return;
                        }
                        _ = sleep(Duration::from_millis(200)) => {}
                    }
                }
            }
        };

        loop {
            tokio::select! {
                // 处理取消信号
                _ = &mut cancel_rx => {
                    tracing::debug!("Event subscription cancelled");
                    EVENT_SUBSCRIPTIONS.remove(&subscription_id);
                    break;
                }

                // 接收事件
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            let event_type = event_type_to_code(&event);
                            if event_type == 0 {
                                continue;
                            }
                            // 序列化事件为 JSON
                            let event_json = string_to_flare(event_to_json(&event));

                            abi::invoke_user_c_callback("FlareEventCallback", || {
                                callback(user_context as *mut c_void, event_type, event_json);
                            });
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            tracing::debug!("Event bus closed");
                            EVENT_SUBSCRIPTIONS.remove(&subscription_id);
                            break;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("Event subscription lagged, missed {} events", n);
                            continue;
                        }
                    }
                }
            }
        }
    });
}

/// 取消事件订阅
///
/// # Arguments
/// * `subscription` - 订阅句柄
#[unsafe(no_mangle)]
pub extern "C" fn flare_event_unsubscribe(subscription: FlareSubscriptionHandle) {
    abi::catch_ffi_void(|| {
        if let Some((_, tx)) = EVENT_SUBSCRIPTIONS.remove(&subscription) {
            let _ = tx.send(());
        } else {
            tracing::debug!("Unsubscribe {} ignored: not found", subscription);
        }
    });
}

/// 取消全部事件订阅（用于 Flutter/iOS 热重启或宿主崩溃恢复后的兜底清理）。
#[unsafe(no_mangle)]
pub extern "C" fn flare_event_unsubscribe_all() {
    abi::catch_ffi_void(unsubscribe_all_events);
}
