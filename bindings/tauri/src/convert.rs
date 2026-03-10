//! SDK Core ← 语言模型 → App：仅做 proto/SDK → model 转换，不涉及 JSON
//!
//! 会话/消息同步由 SDK 调度，通过 SdkEvent 回调；本层只做事件 → (name, payload 模型) 映射。

use flare_proto::common::{ConversationSummary, Message, MessagePreview};
use prost_types::Timestamp;

use crate::model::{
    ConversationSummaryOut, EventPayload, MessageOut, MessagePreviewOut,
    ConversationsSyncedPayload, MessageDeletedPayload, MessageRecalledPayload,
    StateChangedPayload, SyncCompletedPayload, SyncFailedPayload, SyncProgressPayload,
};

fn ts_iso(t: Option<&Timestamp>) -> String {
    let t = match t {
        Some(x) => x,
        None => return String::new(),
    };
    if t.seconds == 0 && t.nanos == 0 {
        return String::new();
    }
    chrono::DateTime::from_timestamp(t.seconds as i64, t.nanos as u32)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

fn message_preview_to_model(p: &MessagePreview) -> MessagePreviewOut {
    MessagePreviewOut {
        message_id: p.message_id.clone(),
        sender_id: p.sender_id.clone(),
        type_: p.r#type,
        text: p.text.clone(),
        time: ts_iso(p.time.as_ref()),
    }
}

/// Message（proto）→ 语言模型
pub fn message_to_model(m: &Message) -> MessageOut {
    MessageOut {
        server_id: m.server_id.clone(),
        conversation_id: m.conversation_id.clone(),
        client_msg_id: m.client_msg_id.clone(),
        sender_id: m.sender_id.clone(),
        receiver_id: m.receiver_id.clone(),
        seq: m.seq,
        timestamp: ts_iso(m.timestamp.as_ref()),
        conversation_type: m.conversation_type,
        message_type: m.message_type,
        content: m.content.clone(),
        status: m.status,
        extra: m.extra.clone(),
    }
}

/// ConversationSummary（proto）→ 语言模型
pub fn conversation_to_model(c: &ConversationSummary) -> ConversationSummaryOut {
    let peer_id = c.ext.get("peer_id").cloned();
    ConversationSummaryOut {
        conversation_id: c.conversation_id.clone(),
        conversation_type: c.conversation_type.clone(),
        business_type: c.business_type.clone(),
        display_name: c.display_name.clone(),
        avatar_url: c.avatar_url.clone(),
        unread_count: c.unread_count,
        max_seq: c.max_seq,
        last_read_seq: c.last_read_seq,
        last_message: c.last_message.as_ref().map(message_preview_to_model),
        updated_at: ts_iso(c.updated_at.as_ref()),
        created_at: ts_iso(c.created_at.as_ref()),
        peer_id,
    }
}

/// SdkEvent → Tauri 事件 (name, payload 模型)；None 表示不转发或由调用方特殊处理（如 SendAck 需查库再发 message）
pub fn sdk_event_to_tauri(e: &flare_im_core_sdk::event::SdkEvent) -> Option<(String, EventPayload)> {
    use flare_im_core_sdk::event::{ConversationEvent, MessageEvent, SdkEvent};

    match e {
        SdkEvent::StateChanged { state } => Some((
            "im://state".into(),
            EventPayload::StateChanged(StateChangedPayload {
                state: format!("{:?}", state),
            }),
        )),
        SdkEvent::Message(MessageEvent::Received { message }) => {
            Some(("im://message".into(), EventPayload::Message(message_to_model(message))))
        }
        SdkEvent::Message(MessageEvent::SendAck { .. }) => None,
        SdkEvent::Message(MessageEvent::Recalled { conversation_id, event }) => Some((
            "im://message_recalled".into(),
            EventPayload::MessageRecalled(MessageRecalledPayload {
                conversation_id: conversation_id.clone(),
                message_id: event.server_msg_id.clone(),
                recaller_id: String::new(),
            }),
        )),
        SdkEvent::Message(MessageEvent::Deleted { event, .. }) => Some((
            "im://message_deleted".into(),
            EventPayload::MessageDeleted(MessageDeletedPayload {
                message_id: event.server_msg_id.clone(),
            }),
        )),
        SdkEvent::Conversation(ConversationEvent::Synced { conversations }) => {
            let list: Vec<ConversationSummaryOut> =
                conversations.iter().map(conversation_to_model).collect();
            Some((
                "im://conversations_synced".into(),
                EventPayload::ConversationsSynced(ConversationsSyncedPayload {
                    conversations: list,
                }),
            ))
        }
        SdkEvent::SyncProgress { task, progress, detail } => Some((
            "im://sync_progress".into(),
            EventPayload::SyncProgress(SyncProgressPayload {
                task: task.clone(),
                progress: *progress,
                detail: detail.clone(),
            }),
        )),
        SdkEvent::SyncTaskCompleted { task } => Some((
            "im://sync_completed".into(),
            EventPayload::SyncCompleted(SyncCompletedPayload {
                task: task.clone(),
            }),
        )),
        SdkEvent::SyncTaskFailed { task, error } => Some((
            "im://sync_failed".into(),
            EventPayload::SyncFailed(SyncFailedPayload {
                task: task.clone(),
                error: error.clone(),
            }),
        )),
        _ => None,
    }
}
