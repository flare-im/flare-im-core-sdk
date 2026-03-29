//! SdkEvent → Tauri `im://*` 事件名与可序列化 payload；无查库、无合并等业务逻辑。

use flare_im_core_sdk::event::{
    ConnectionEvent, ConversationEvent, MessageEvent, SdkEvent, SyncNotify, SyncPhase,
};

use crate::model::{
    ConnectedPayload, ConversationDeletedPayload, ConversationsSyncedPayload,
    ConversationUpdatedPayload, DisconnectedPayload, EventPayload, ExtensionPayload,
    KickedOffPayload, MessageRecalledPayload, MessageSendFailedPayload, ReconnectingPayload,
    ServerErrorPayload, StateChangedPayload, SyncCompletedPayload,
    SyncFailedPayload, SyncFinishedPayload, SyncProgressPayload, SyncStartedPayload,
    SyncStateChangedPayload, TokenExpiredPayload, TypingPayload, UnreadCountChangedPayload,
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
                state: format!("{state:?}"),
            }),
        )),

        SdkEvent::Message(MessageEvent::Received { message }) => Some((
            "im://message".into(),
            EventPayload::Message(message.clone()),
        )),
        SdkEvent::Message(MessageEvent::ReceivedBatch { messages }) => Some((
            "im://message_batch".into(),
            EventPayload::MessageBatch(messages.clone()),
        )),
        SdkEvent::Message(MessageEvent::SendAck { ack }) => Some((
            "im://send_ack".into(),
            EventPayload::SendAck(ack.clone().into()),
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

        SdkEvent::Sync(SyncNotify::Started) => Some((
            "im://sync_started".into(),
            EventPayload::SyncStarted(SyncStartedPayload {}),
        )),
        SdkEvent::Sync(SyncNotify::Finished { phase }) => {
            let phase_str = match phase {
                SyncPhase::Init => "Init",
                SyncPhase::Background => "Background",
            };
            Some((
                "im://sync_finished".into(),
                EventPayload::SyncFinished(SyncFinishedPayload {
                    phase: phase_str.to_string(),
                }),
            ))
        }
        SdkEvent::Sync(SyncNotify::Progress {
            task,
            progress,
            detail,
        }) => Some((
            "im://sync_progress".into(),
            EventPayload::SyncProgress(SyncProgressPayload {
                task: task.clone(),
                progress: *progress,
                detail: detail.clone(),
            }),
        )),
        SdkEvent::Sync(SyncNotify::TaskCompleted { task }) => Some((
            "im://sync_completed".into(),
            EventPayload::SyncCompleted(SyncCompletedPayload { task: task.clone() }),
        )),
        SdkEvent::Sync(SyncNotify::Failed { task, message }) => Some((
            "im://sync_failed".into(),
            EventPayload::SyncFailed(SyncFailedPayload {
                task: task.clone(),
                error: message.clone(),
            }),
        )),
        SdkEvent::Sync(SyncNotify::StateChanged { state }) => Some((
            "im://sync_state_changed".into(),
            EventPayload::SyncStateChanged(SyncStateChangedPayload {
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
