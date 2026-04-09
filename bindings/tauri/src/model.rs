//! Tauri IPC 用的可序列化类型与事件 payload；业务在 core-sdk。
//!
//! **命名**：本 crate 只使用 Rust 默认 JSON 形状（snake_case 字段名）。宿主若用 camelCase（如 WebView），在宿主侧做转换即可，bindings 不重复承担 rename。
//!
//! **去重**：多个 `im://*` 事件若 JSON 形状一致，共用同一 struct，并以 `type` 别名保留语义化名称供 `convert` 使用。

use std::collections::HashMap;

use flare_proto::common::SendAck as ProtoSendAck;
use serde::{Deserialize, Serialize};

// ---------- 上层可传入的 SDK 配置（snake_case JSON，与 [flare_im_core_sdk::client::SdkConfigOverlay] 一致） ----------

pub use flare_im_core_sdk::client::SdkConfigOverlay as SdkConfigOptions;

/// sdk_init 入参：`{ environment?, sdk_config? }`（snake_case）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SdkInitArgs {
    pub environment: Option<String>,
    pub sdk_config: Option<SdkConfigOptions>,
}

/// 消息：直接使用 SDK 的 IMMessage（序列化由 core-sdk 实现，snake_case）
pub use flare_im_core_sdk::model::IMMessage;

// ---------- 事件 payload（Tauri emit） ----------

/// 发消息回执（与 proto SendAck 字段对齐，供 command / `im://send_ack`）
#[derive(Debug, Clone, Serialize)]
pub struct SendAckPayload {
    pub client_msg_id: String,
    pub server_msg_id: String,
    pub seq: u64,
    pub conversation_id: String,
    pub success: bool,
    pub error_code: i32,
    pub error_message: String,
}

/// 媒体上传进度（sdk_send_with_media_progress 通过 im://upload_progress 推送）
#[derive(Debug, Clone, Serialize)]
pub struct UploadProgressPayload {
    pub file_name: String,
    pub upload_id: String,
    pub phase: String,
    pub uploaded_bytes: u64,
    pub total_bytes: u64,
    pub chunk_index: Option<u32>,
    pub total_chunks: Option<u32>,
}

impl From<flare_im_core_sdk::client::UploadProgress> for UploadProgressPayload {
    fn from(p: flare_im_core_sdk::client::UploadProgress) -> Self {
        let phase = match p.phase {
            flare_im_core_sdk::client::UploadPhase::Preparing => "Preparing",
            flare_im_core_sdk::client::UploadPhase::Uploading => "Uploading",
            flare_im_core_sdk::client::UploadPhase::Completing => "Completing",
            flare_im_core_sdk::client::UploadPhase::Finished => "Finished",
        }
        .to_string();
        Self {
            file_name: p.file_name,
            upload_id: p.upload_id,
            phase,
            uploaded_bytes: p.uploaded_bytes,
            total_bytes: p.total_bytes,
            chunk_index: p.chunk_index,
            total_chunks: p.total_chunks,
        }
    }
}

impl From<ProtoSendAck> for SendAckPayload {
    fn from(a: ProtoSendAck) -> Self {
        Self {
            client_msg_id: a.client_msg_id,
            server_msg_id: a.server_msg_id,
            seq: a.seq,
            conversation_id: a.conversation_id,
            success: a.success,
            error_code: a.error_code,
            error_message: a.error_message,
        }
    }
}

/// `{ "state": "..." }` — `im://state`、部分 sync 状态（与 sync_state_changed 同形）
#[derive(Debug, Clone, Serialize)]
pub struct JsonStatePayload {
    pub state: String,
}

pub type StateChangedPayload = JsonStatePayload;
pub type SyncStateChangedPayload = JsonStatePayload;

/// 消息发送失败（client_msg_id + 原因）
#[derive(Debug, Clone, Serialize)]
pub struct MessageSendFailedPayload {
    pub client_msg_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageRecalledPayload {
    pub conversation_id: String,
    pub message_id: String,
    pub recaller_id: String,
}

/// 消息已编辑（本地库已更新，前端宜按 conversationId 刷新列表或按 messageId 合并）
#[derive(Debug, Clone, Serialize)]
pub struct MessageEditedPayload {
    pub conversation_id: String,
    /// 服务端消息 id（与撤回事件的 message_id 语义一致）
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_version: Option<i32>,
}

/// 消息反应变更（添加/移除）
#[derive(Debug, Clone, Serialize)]
pub struct MessageReactionChangedPayload {
    pub conversation_id: String,
    pub message_id: String,
    pub user_id: String,
    pub emoji: String,
    /// 与 proto `ReactionAction` 对齐：1=ADD, 2=REMOVE
    pub action: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageDeletedPayload {
    pub conversation_id: String,
    pub message_id: String,
}

/// 已读回执事件
#[derive(Debug, Clone, Serialize)]
pub struct MessageReadReceiptPayload {
    pub conversation_id: String,
    pub user_id: String,
    pub read_seq: u64,
    pub message_ids: Vec<String>,
}

/// 置顶事件
#[derive(Debug, Clone, Serialize)]
pub struct MessagePinnedPayload {
    pub conversation_id: String,
    pub message_id: String,
    pub pinned_by: String,
}

/// 取消置顶事件
#[derive(Debug, Clone, Serialize)]
pub struct MessageUnpinnedPayload {
    pub conversation_id: String,
    pub message_id: String,
}

/// 标记事件
#[derive(Debug, Clone, Serialize)]
pub struct MessageMarkedPayload {
    pub conversation_id: String,
    pub message_id: String,
    pub user_id: String,
    pub mark_type: i32,
    pub color: String,
}

/// 取消标记事件
#[derive(Debug, Clone, Serialize)]
pub struct MessageUnmarkedPayload {
    pub conversation_id: String,
    pub message_id: String,
    pub user_id: String,
    pub mark_type: i32,
}

/// 在线状态事件
#[derive(Debug, Clone, Serialize)]
pub struct PresenceChangedPayload {
    pub conversation_id: String,
    pub user_id: String,
    pub status: String,
    pub extra: HashMap<String, String>,
}

/// 通话信令事件
#[derive(Debug, Clone, Serialize)]
pub struct CallSignalPayload {
    pub conversation_id: String,
    pub call_id: String,
    pub signal_type: String,
    pub payload: Vec<u8>,
    pub metadata: HashMap<String, String>,
}

/// 自定义领域事件
#[derive(Debug, Clone, Serialize)]
pub struct MessageCustomEventPayload {
    pub conversation_id: String,
    pub namespace: String,
    pub name: String,
    pub version: String,
    pub payload: Vec<u8>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationsSyncedPayload {
    pub conversation_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncProgressPayload {
    pub task: String,
    pub progress: f32,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncCompletedPayload {
    pub task: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncFailedPayload {
    pub task: String,
    pub error: String,
}

/// 同步阶段结束（Init / Background）
#[derive(Debug, Clone, Serialize)]
pub struct SyncFinishedPayload {
    pub phase: String,
}

/// `{}` — 无字段的 ACK 类事件（connected、sync_started 等）
#[derive(Debug, Clone, Serialize)]
pub struct EmptyPayload {}

pub type ConnectedPayload = EmptyPayload;
/// 同步开始（无额外字段，前端用于显示「同步中」）
pub type SyncStartedPayload = EmptyPayload;

/// `{ "reason": "..." }` — disconnected、kicked_off
#[derive(Debug, Clone, Serialize)]
pub struct ReasonPayload {
    pub reason: String,
}

pub type DisconnectedPayload = ReasonPayload;
pub type KickedOffPayload = ReasonPayload;

/// Token 过期
#[derive(Debug, Clone, Serialize)]
pub struct TokenExpiredPayload {
    pub message: String,
}

/// 服务端错误
#[derive(Debug, Clone, Serialize)]
pub struct ServerErrorPayload {
    pub code: i32,
    pub message: String,
}

/// 重连中
#[derive(Debug, Clone, Serialize)]
pub struct ReconnectingPayload {
    pub attempt: u32,
}

/// `{ "conversation_id": "..." }` — created / updated / deleted（仅 id）
#[derive(Debug, Clone, Serialize)]
pub struct ConversationIdPayload {
    pub conversation_id: String,
}

pub type ConversationUpdatedPayload = ConversationIdPayload;
pub type ConversationDeletedPayload = ConversationIdPayload;

/// 未读数变更
#[derive(Debug, Clone, Serialize)]
pub struct UnreadCountChangedPayload {
    pub conversation_id: String,
    pub unread_count: u32,
}

/// 正在输入
#[derive(Debug, Clone, Serialize)]
pub struct TypingPayload {
    pub conversation_id: String,
    pub user_id: String,
    pub typing: bool,
}

/// 扩展事件（payload 为原始字节，前端按需解码）
#[derive(Debug, Clone, Serialize)]
pub struct ExtensionPayload {
    pub source: String,
    pub event_type: String,
    /// 原始字节，JSON 序列化为 number[]
    pub payload: Vec<u8>,
}

/// 事件 payload 枚举，供 Tauri emit 使用；序列化时无标签，仅内层字段
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum EventPayload {
    StateChanged(StateChangedPayload),
    SyncStateChanged(SyncStateChangedPayload),
    Message(IMMessage),
    MessageBatch(Vec<IMMessage>),
    SendAck(SendAckPayload),
    MessageSendFailed(MessageSendFailedPayload),
    MessageRecalled(MessageRecalledPayload),
    MessageEdited(MessageEditedPayload),
    MessageReactionChanged(MessageReactionChangedPayload),
    MessageDeleted(MessageDeletedPayload),
    MessageReadReceipt(MessageReadReceiptPayload),
    MessagePinned(MessagePinnedPayload),
    MessageUnpinned(MessageUnpinnedPayload),
    MessageMarked(MessageMarkedPayload),
    MessageUnmarked(MessageUnmarkedPayload),
    PresenceChanged(PresenceChangedPayload),
    CallSignal(CallSignalPayload),
    MessageCustomEvent(MessageCustomEventPayload),
    ConversationsSynced(ConversationsSyncedPayload),
    SyncProgress(SyncProgressPayload),
    SyncCompleted(SyncCompletedPayload),
    SyncFailed(SyncFailedPayload),
    SyncStarted(SyncStartedPayload),
    SyncFinished(SyncFinishedPayload),
    Connected(ConnectedPayload),
    Disconnected(DisconnectedPayload),
    KickedOff(KickedOffPayload),
    TokenExpired(TokenExpiredPayload),
    ServerError(ServerErrorPayload),
    Reconnecting(ReconnectingPayload),
    ConversationUpdated(ConversationUpdatedPayload),
    ConversationDeleted(ConversationDeletedPayload),
    UnreadCountChanged(UnreadCountChangedPayload),
    Typing(TypingPayload),
    Extension(ExtensionPayload),
}
