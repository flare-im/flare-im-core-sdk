//! SdkEvent → Tauri `im://*` 事件名与可序列化 payload；无查库、无合并等业务逻辑。

use flare_proto::common::call_audience;
use flare_proto::common::call_signal_event::Signal;
use flare_proto::common::{
    CallAudience, CallMediaSessionInfo, CallOfferedMedia, CallSignalEvent, SfuPeerSnapshot,
    SfuRoomSnapshot, SfuTrackSnapshot, SfuTransportContext,
};

use flare_im_core_sdk::core::SyncRunContext;
use flare_im_core_sdk::event::{
    ConnectionEvent, ConversationEvent, MessageEvent, NotificationEvent, SdkEvent, SyncNotify,
    SyncPhase,
};

use crate::model::{
    CallSignalPayload, ConnectedPayload, ConversationDeletedPayload, ConversationUpdatedPayload,
    ConversationsSyncedPayload, DisconnectedPayload, EventPayload, ExtensionPayload,
    KickedOffPayload, MessageBurnScheduledPayload, MessageBurnedPayload, MessageCustomEventPayload,
    MessageDeletedPayload, MessageEditedPayload, MessageHardDeletedPayload, MessageMarkedPayload,
    MessagePinnedPayload, MessageReactionChangedPayload, MessageReadReceiptPayload,
    MessageRecalledPayload, MessageSendFailedPayload, MessageUnmarkedPayload,
    MessageUnpinnedPayload, PresenceChangedPayload, ReconnectingPayload, ServerErrorPayload,
    StateChangedPayload, SyncCompletedPayload, SyncFailedPayload, SyncFinishedPayload,
    SyncProgressPayload, SyncRunPayload, SyncStartedPayload, SyncStateChangedPayload,
    TokenExpiredPayload, TypingPayload, UnreadCountChangedPayload,
};

/// SdkEvent → (事件名, payload)；无法序列化或不需要转发的返回 `None`。
#[inline]
pub fn sdk_event_to_tauri(e: &SdkEvent) -> Option<(String, EventPayload)> {
    match e {
        SdkEvent::Connection(ConnectionEvent::Connected) => Some((
            "im://connected".into(),
            EventPayload::Connected(ConnectedPayload {}),
        )),
        SdkEvent::Connection(ConnectionEvent::Disconnected { reason }) => Some((
            "im://disconnected".into(),
            EventPayload::Disconnected(DisconnectedPayload {
                reason: reason.clone(),
            }),
        )),
        SdkEvent::Connection(ConnectionEvent::StateChanged { state }) => Some((
            "im://state".into(),
            EventPayload::StateChanged(StateChangedPayload {
                state: format!("{state:?}"),
            }),
        )),
        SdkEvent::Connection(ConnectionEvent::ServerError { code, message }) => Some((
            "im://server_error".into(),
            EventPayload::ServerError(ServerErrorPayload {
                code: *code,
                message: message.clone(),
            }),
        )),
        SdkEvent::Connection(ConnectionEvent::Reconnecting { attempt }) => Some((
            "im://reconnecting".into(),
            EventPayload::Reconnecting(ReconnectingPayload { attempt: *attempt }),
        )),
        SdkEvent::Connection(ConnectionEvent::KickedOff { reason }) => Some((
            "im://kicked_off".into(),
            EventPayload::KickedOff(KickedOffPayload {
                reason: reason.clone(),
            }),
        )),
        SdkEvent::Connection(ConnectionEvent::TokenExpired { message }) => Some((
            "im://token_expired".into(),
            EventPayload::TokenExpired(TokenExpiredPayload {
                message: message.clone(),
            }),
        )),
        SdkEvent::Connection(ConnectionEvent::SyncStateChanged { state }) => Some((
            "im://sync_state_changed".into(),
            EventPayload::SyncStateChanged(SyncStateChangedPayload {
                run: legacy_sync_run_payload(),
                state: format!("{state:?}"),
            }),
        )),

        SdkEvent::Message(MessageEvent::Received { message }) => Some((
            "im://message".into(),
            EventPayload::Message(Box::new(message.as_ref().clone())),
        )),
        SdkEvent::Message(MessageEvent::ReceivedBatch { messages }) => Some((
            "im://message_batch".into(),
            EventPayload::MessageBatch(messages.clone()),
        )),
        SdkEvent::Message(MessageEvent::SendAck { ack }) => Some((
            "im://send_ack".into(),
            EventPayload::SendAck(ack.as_ref().clone().into()),
        )),
        SdkEvent::Message(MessageEvent::SendFailed {
            client_msg_id,
            reason,
        }) => Some((
            "im://send_failed".into(),
            EventPayload::MessageSendFailed(MessageSendFailedPayload {
                client_msg_id: client_msg_id.clone(),
                reason: reason.clone(),
            }),
        )),
        SdkEvent::Message(MessageEvent::Recalled {
            conversation_id,
            event,
        }) => Some((
            "im://message_recalled".into(),
            EventPayload::MessageRecalled(MessageRecalledPayload {
                conversation_id: conversation_id.clone(),
                message_id: event.server_msg_id.clone(),
                recaller_id: String::new(),
            }),
        )),
        SdkEvent::Message(MessageEvent::Edited {
            conversation_id,
            server_msg_id,
            edit_version,
        }) => Some((
            "im://message_edited".into(),
            EventPayload::MessageEdited(MessageEditedPayload {
                conversation_id: conversation_id.clone(),
                message_id: server_msg_id.clone(),
                edit_version: *edit_version,
            }),
        )),
        SdkEvent::Message(MessageEvent::ReactionChanged {
            conversation_id,
            server_msg_id,
            user_id,
            emoji,
            action,
        }) => Some((
            "im://message_reaction_changed".into(),
            EventPayload::MessageReactionChanged(MessageReactionChangedPayload {
                conversation_id: conversation_id.clone(),
                message_id: server_msg_id.clone(),
                user_id: user_id.clone(),
                emoji: emoji.clone(),
                action: *action,
            }),
        )),
        SdkEvent::Message(MessageEvent::Typing {
            conversation_id,
            event,
        }) => Some((
            "im://typing".into(),
            EventPayload::Typing(TypingPayload {
                conversation_id: conversation_id.clone(),
                user_id: event.user_id.clone(),
                typing: event.typing,
            }),
        )),
        SdkEvent::Message(MessageEvent::Deleted {
            conversation_id,
            event,
        }) => Some((
            "im://message_deleted".into(),
            EventPayload::MessageDeleted(MessageDeletedPayload {
                conversation_id: conversation_id.clone(),
                message_id: event.server_msg_id.clone(),
            }),
        )),
        SdkEvent::Message(MessageEvent::ReadReceipt {
            conversation_id,
            event,
        }) => Some((
            "im://message_read_receipt".into(),
            EventPayload::MessageReadReceipt(MessageReadReceiptPayload {
                conversation_id: conversation_id.clone(),
                user_id: event.user_id.clone(),
                read_seq: event.read_seq,
                message_ids: event.message_ids.clone(),
                burn_after_read: event.burn_after_read.unwrap_or(false),
            }),
        )),
        SdkEvent::Message(MessageEvent::BurnScheduled {
            conversation_id,
            event,
        }) => Some((
            "im://message_burn_scheduled".into(),
            EventPayload::MessageBurnScheduled(MessageBurnScheduledPayload {
                conversation_id: conversation_id.clone(),
                message_id: event.message_id.clone(),
                server_id: event.server_id.clone(),
                reader_id: event.reader_id.clone(),
                burn_at: event.burn_at,
                event_time: event.event_time,
            }),
        )),
        SdkEvent::Message(MessageEvent::Burned {
            conversation_id,
            event,
        }) => Some((
            "im://message_burned".into(),
            EventPayload::MessageBurned(MessageBurnedPayload {
                conversation_id: conversation_id.clone(),
                message_id: event.message_id.clone(),
                server_id: event.server_id.clone(),
                reader_id: event.reader_id.clone(),
                burn_at: event.burn_at,
                burned_at: event.burned_at,
                event_time: event.event_time,
            }),
        )),
        SdkEvent::Message(MessageEvent::HardDeleted {
            conversation_id,
            event,
        }) => Some((
            "im://message_hard_deleted".into(),
            EventPayload::MessageHardDeleted(MessageHardDeletedPayload {
                conversation_id: conversation_id.clone(),
                message_id: event.message_id.clone(),
                server_id: event.server_id.clone(),
                reader_id: event.reader_id.clone(),
                burn_at: event.burn_at,
                burned_at: event.burned_at,
                event_time: event.event_time,
            }),
        )),
        SdkEvent::Message(MessageEvent::Pinned {
            conversation_id,
            event,
        }) => Some((
            "im://message_pinned".into(),
            EventPayload::MessagePinned(MessagePinnedPayload {
                conversation_id: conversation_id.clone(),
                message_id: event.server_msg_id.clone(),
                pinned_by: event.pinned_by.clone(),
            }),
        )),
        SdkEvent::Message(MessageEvent::Unpinned {
            conversation_id,
            event,
        }) => Some((
            "im://message_unpinned".into(),
            EventPayload::MessageUnpinned(MessageUnpinnedPayload {
                conversation_id: conversation_id.clone(),
                message_id: event.server_msg_id.clone(),
            }),
        )),
        SdkEvent::Message(MessageEvent::Marked {
            conversation_id,
            event,
        }) => Some((
            "im://message_marked".into(),
            EventPayload::MessageMarked(MessageMarkedPayload {
                conversation_id: conversation_id.clone(),
                message_id: event.server_msg_id.clone(),
                user_id: event.user_id.clone(),
                mark_type: event.mark_type,
                color: event.color.clone(),
            }),
        )),
        SdkEvent::Message(MessageEvent::Unmarked {
            conversation_id,
            event,
        }) => Some((
            "im://message_unmarked".into(),
            EventPayload::MessageUnmarked(MessageUnmarkedPayload {
                conversation_id: conversation_id.clone(),
                message_id: event.server_msg_id.clone(),
                user_id: event.user_id.clone(),
                mark_type: event.mark_type,
            }),
        )),
        SdkEvent::Message(MessageEvent::PresenceChanged {
            conversation_id,
            event,
        }) => Some((
            "im://presence_changed".into(),
            EventPayload::PresenceChanged(PresenceChangedPayload {
                conversation_id: conversation_id.clone(),
                user_id: event.user_id.clone(),
                status: event.status.clone(),
                extra: event.extra.clone(),
            }),
        )),
        SdkEvent::Message(MessageEvent::CallSignal {
            conversation_id,
            event,
        }) => Some((
            "im://call_signal".into(),
            EventPayload::CallSignal(call_signal_to_payload(conversation_id, event)),
        )),
        SdkEvent::Message(MessageEvent::Custom {
            conversation_id,
            event,
        }) => Some((
            "im://message_custom_event".into(),
            EventPayload::MessageCustomEvent(MessageCustomEventPayload {
                conversation_id: conversation_id.clone(),
                namespace: event.namespace.clone(),
                name: event.name.clone(),
                version: event.version.clone(),
                payload: event.payload.clone(),
                metadata: event.metadata.clone(),
            }),
        )),

        SdkEvent::Notification(NotificationEvent::Received { message }) => Some((
            "im://message".into(),
            EventPayload::Message(Box::new(message.as_ref().clone())),
        )),

        SdkEvent::Conversation(ConversationEvent::Synced { conversation_ids }) => Some((
            "im://conversations_synced".into(),
            EventPayload::ConversationsSynced(ConversationsSyncedPayload {
                conversation_ids: conversation_ids.clone(),
            }),
        )),
        SdkEvent::Conversation(ConversationEvent::Created { conversation_id }) => Some((
            "im://conversation_created".into(),
            EventPayload::ConversationUpdated(ConversationUpdatedPayload {
                conversation_id: conversation_id.clone(),
            }),
        )),
        SdkEvent::Conversation(ConversationEvent::Updated { conversation_id }) => Some((
            "im://conversation_updated".into(),
            EventPayload::ConversationUpdated(ConversationUpdatedPayload {
                conversation_id: conversation_id.clone(),
            }),
        )),
        SdkEvent::Conversation(ConversationEvent::UnreadCountChanged {
            conversation_id,
            unread_count,
        }) => Some((
            "im://unread_count_changed".into(),
            EventPayload::UnreadCountChanged(UnreadCountChangedPayload {
                conversation_id: conversation_id.clone(),
                unread_count: *unread_count,
            }),
        )),
        SdkEvent::Conversation(ConversationEvent::Deleted { conversation_id }) => Some((
            "im://conversation_deleted".into(),
            EventPayload::ConversationDeleted(ConversationDeletedPayload {
                conversation_id: conversation_id.clone(),
            }),
        )),

        SdkEvent::Sync(SyncNotify::Started { run }) => Some((
            "im://sync_started".into(),
            EventPayload::SyncStarted(SyncStartedPayload {
                run: sync_run_payload(run),
            }),
        )),
        SdkEvent::Sync(SyncNotify::Finished { run, phase }) => {
            let phase_str = match phase {
                SyncPhase::Init => "Init",
                SyncPhase::Background => "Background",
            };
            Some((
                "im://sync_finished".into(),
                EventPayload::SyncFinished(SyncFinishedPayload {
                    run: sync_run_payload(run),
                    phase: phase_str.to_string(),
                }),
            ))
        }
        SdkEvent::Sync(SyncNotify::Progress {
            run,
            task,
            progress,
            detail,
        }) => Some((
            "im://sync_progress".into(),
            EventPayload::SyncProgress(SyncProgressPayload {
                run: sync_run_payload(run),
                task: task.clone(),
                progress: *progress,
                detail: detail.clone(),
            }),
        )),
        SdkEvent::Sync(SyncNotify::TaskCompleted { run, task }) => Some((
            "im://sync_completed".into(),
            EventPayload::SyncCompleted(SyncCompletedPayload {
                run: sync_run_payload(run),
                task: task.clone(),
            }),
        )),
        SdkEvent::Sync(SyncNotify::Failed { run, task, message }) => Some((
            "im://sync_failed".into(),
            EventPayload::SyncFailed(SyncFailedPayload {
                run: sync_run_payload(run),
                task: task.clone(),
                error: message.clone(),
            }),
        )),
        SdkEvent::Sync(SyncNotify::StateChanged { run, state }) => Some((
            "im://sync_state_changed".into(),
            EventPayload::SyncStateChanged(SyncStateChangedPayload {
                run: sync_run_payload(run),
                state: format!("{state:?}"),
            }),
        )),

        SdkEvent::Extension(ev) => Some((
            "im://extension".into(),
            EventPayload::Extension(ExtensionPayload {
                source: ev.source.clone(),
                event_type: ev.event_type.clone(),
                payload: ev.payload.clone(),
            }),
        )),
    }
}

fn sync_run_payload(run: &SyncRunContext) -> SyncRunPayload {
    SyncRunPayload {
        run_id: run.run_id.clone(),
        trigger: run.trigger.as_str().to_string(),
        scope: run.scope.as_str().to_string(),
        visibility: run.visibility.as_str().to_string(),
        reason: run.reason.as_str().to_string(),
    }
}

fn legacy_sync_run_payload() -> SyncRunPayload {
    SyncRunPayload {
        run_id: "legacy-sync-state".to_string(),
        trigger: "BackgroundMaintenance".to_string(),
        scope: "Global".to_string(),
        visibility: "Silent".to_string(),
        reason: "BackgroundCatchUp".to_string(),
    }
}

fn call_signal_to_payload(conversation_id: &str, event: &CallSignalEvent) -> CallSignalPayload {
    let cid = if event.conversation_id.is_empty() {
        conversation_id.to_string()
    } else {
        event.conversation_id.clone()
    };
    CallSignalPayload {
        conversation_id: cid,
        call_id: event.call_id.clone(),
        from_user_id: event.from_user_id.clone(),
        to_user_id: direct_peer_user_id(event.audience.as_ref()),
        audience: audience_to_json(event.audience.as_ref()),
        media_session: media_session_to_json(event.media_session.as_ref()),
        transport: transport_to_json(event.transport.as_ref()),
        invite_expires_at_unix: event.invite_deadline.as_ref().map(|t| t.seconds),
        ext: event.ext.clone(),
        variant: call_signal_variant_name(&event.signal).to_string(),
        body: call_signal_body_json(&event.signal),
    }
}

fn direct_peer_user_id(a: Option<&CallAudience>) -> Option<String> {
    let a = a?;
    match &a.shape {
        Some(call_audience::Shape::Direct(d)) if !d.peer_user_id.trim().is_empty() => {
            Some(d.peer_user_id.clone())
        }
        _ => None,
    }
}

fn audience_to_json(a: Option<&CallAudience>) -> serde_json::Value {
    let Some(a) = a else {
        return serde_json::Value::Null;
    };
    match &a.shape {
        Some(call_audience::Shape::Direct(d)) => serde_json::json!({
            "direct": { "peerUserId": d.peer_user_id }
        }),
        Some(call_audience::Shape::Explicit(e)) => serde_json::json!({
            "explicit": { "userIds": e.user_ids }
        }),
        Some(call_audience::Shape::Broadcast(_)) => serde_json::json!({ "broadcast": {} }),
        None => serde_json::Value::Null,
    }
}

fn media_session_to_json(m: Option<&CallMediaSessionInfo>) -> serde_json::Value {
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

fn transport_to_json(t: Option<&SfuTransportContext>) -> serde_json::Value {
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

fn call_signal_variant_name(signal: &Option<Signal>) -> &'static str {
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

fn offered_media_json(m: Option<&CallOfferedMedia>) -> serde_json::Value {
    let Some(m) = m else {
        return serde_json::Value::Null;
    };
    serde_json::json!({
        "types": m.types,
        "primarySource": m.primary_source,
        "codecHint": m.codec_hint,
    })
}

fn track_state_json(t: &SfuTrackSnapshot) -> serde_json::Value {
    serde_json::json!({
        "trackId": t.track_id,
        "mediaType": t.media_type,
        "source": t.source,
        "codec": t.codec,
    })
}

fn room_snapshot_json(s: &SfuRoomSnapshot) -> serde_json::Value {
    serde_json::json!({
        "roomId": s.room_id,
        "peerCount": s.peer_count,
        "trackCount": s.track_count,
        "exists": s.exists,
        "draining": s.draining,
    })
}

fn peer_snapshot_json(p: &SfuPeerSnapshot) -> serde_json::Value {
    serde_json::json!({
        "peerId": p.peer_id,
        "userId": p.user_id,
        "publishedTrackIds": p.published_track_ids,
        "subscribedTrackIds": p.subscribed_track_ids,
        "mediaSessionId": p.media_session_id,
        "sfuRole": p.sfu_role,
        "rosterRole": p.roster_role,
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

fn call_signal_body_json(signal: &Option<Signal>) -> serde_json::Value {
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
        Some(Signal::IceCandidate(c)) => serde_json::json!({
            "iceCandidate": {
                "candidate": c.candidate,
                "sdpMid": c.sdp_mid,
                "sdpMLineIndex": c.sdp_mline_index,
                "candidateJson": c.candidate_json,
            }
        }),
        Some(Signal::Ringing(_)) => serde_json::json!({ "ringing": {} }),
        Some(Signal::Busy(b)) => serde_json::json!({ "busy": { "reason": b.reason } }),
        Some(Signal::Renegotiate(r)) => {
            serde_json::json!({ "renegotiate": { "wantMedia": r.want_media } })
        }
        Some(Signal::SfuRoom(s)) => serde_json::json!({ "sfuRoom": room_snapshot_json(s) }),
        Some(Signal::SfuPeerJoined(p)) => {
            serde_json::json!({ "sfuPeerJoined": peer_snapshot_json(p) })
        }
        Some(Signal::SfuPeerLeft(p)) => serde_json::json!({
            "sfuPeerLeft": { "peerId": p.peer_id, "userId": p.user_id }
        }),
        Some(Signal::SfuTrackPublished(t)) => {
            serde_json::json!({ "sfuTrackPublished": track_state_json(t) })
        }
        Some(Signal::SfuTrackUnpublished(t)) => {
            serde_json::json!({ "sfuTrackUnpublished": track_state_json(t) })
        }
        Some(Signal::SfuSubscribed(r)) => {
            serde_json::json!({ "sfuSubscribed": { "trackId": r.track_id } })
        }
        Some(Signal::SfuUnsubscribed(r)) => {
            serde_json::json!({ "sfuUnsubscribed": { "trackId": r.track_id } })
        }
        Some(Signal::SfuJoinHints(h)) => serde_json::json!({
            "sfuJoinHints": {
                "token": h.token,
                "ttlSeconds": h.ttl_seconds,
                "roomId": h.room_id,
                "peerId": h.peer_id,
                "mediaSessionId": h.media_session_id,
                "signalingWsBase": h.signaling_ws_base,
                "instanceId": h.instance_id,
            }
        }),
        Some(Signal::SfuSubscription(s)) => serde_json::json!({
            "sfuSubscription": {
                "subscriberPeerId": s.subscriber_peer_id,
                "trackId": s.track_id,
                "enabled": s.enabled,
                "media": s.media,
                "preferredLayer": s.preferred_layer,
                "priority": s.priority,
            }
        }),
        Some(Signal::SfuAudioLevel(a)) => serde_json::json!({
            "sfuAudioLevel": {
                "peerId": a.peer_id,
                "userId": a.user_id,
                "linearLevel": a.linear_level,
                "voiceActive": a.voice_active,
            }
        }),
        Some(Signal::SfuNetworkQuality(n)) => serde_json::json!({
            "sfuNetworkQuality": {
                "peerId": n.peer_id,
                "upstreamScore": n.upstream_score,
                "downstreamScore": n.downstream_score,
                "rttMs": n.rtt_ms,
                "packetLossRatio": n.packet_loss_ratio,
            }
        }),
        Some(Signal::SfuBweHint(h)) => serde_json::json!({
            "sfuBweHint": {
                "peerId": h.peer_id,
                "congested": h.congested,
                "suggestedCapKbps": h.suggested_cap_kbps,
            }
        }),
        Some(Signal::InviteeUpdate(u)) => serde_json::json!({
            "inviteeUpdate": { "addedUserIds": u.added_user_ids, "removedUserIds": u.removed_user_ids }
        }),
    }
}
