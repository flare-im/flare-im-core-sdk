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

/// 供前端 WebRTC 使用的 ICE 公共配置快照。
#[derive(Debug, Clone, Serialize)]
pub struct RtcIceConfigSnapshotPayload {
    pub source: String,
    pub turn_enabled: bool,
    pub default_ice_tf: String,
    pub ice_servers: serde_json::Value,
}

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

#[derive(Debug, Clone, Serialize)]
pub struct SyncStateChangedPayload {
    #[serde(flatten)]
    pub run: SyncRunPayload,
    pub state: String,
}

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

// ---------- 通话插件 · Tauri 命令入参（snake_case JSON，与 `flare-sdk-plugin-call` 语义对齐） ----------

/// [`commands::call_signal::sdk_send_call_invite`]
#[derive(Debug, Clone, Deserialize)]
pub struct CallPluginInviteRequest {
    #[serde(alias = "conversationId")]
    pub conversation_id: String,
    #[serde(alias = "callId")]
    pub call_id: String,
    #[serde(alias = "toUserId")]
    pub to_user_id: String,
    /// 非单聊会话下的额外被叫（与 `to_user_id` 合并去重）。若两者在去主叫后 **均为空**，则上行 **`broadcast` 全员响铃**。
    #[serde(default, alias = "participantUserIds")]
    pub participant_user_ids: Vec<String>,
    pub video: bool,
    #[serde(default, alias = "sfuRoomId")]
    pub sfu_room_id: Option<String>,
    #[serde(default, alias = "sfuPeerId")]
    pub sfu_peer_id: Option<String>,
    #[serde(default, alias = "sfuSignalingWsBase")]
    pub sfu_signaling_ws_base: Option<String>,
    #[serde(default, alias = "sfuJoinToken")]
    pub sfu_join_token: Option<String>,
}

/// [`commands::call_signal::sdk_send_call_accept`]
#[derive(Debug, Clone, Deserialize)]
pub struct CallPluginAcceptRequest {
    #[serde(alias = "conversationId")]
    pub conversation_id: String,
    #[serde(alias = "callId")]
    pub call_id: String,
    pub video: bool,
    #[serde(default, alias = "toUserId")]
    pub to_user_id: Option<String>,
    #[serde(default, alias = "sfuRoomId")]
    pub sfu_room_id: Option<String>,
    #[serde(default, alias = "sfuPeerId")]
    pub sfu_peer_id: Option<String>,
    #[serde(default, alias = "sfuSignalingWsBase")]
    pub sfu_signaling_ws_base: Option<String>,
    #[serde(default, alias = "sfuJoinToken")]
    pub sfu_join_token: Option<String>,
}

/// [`commands::call_signal::sdk_send_call_hangup`]
#[derive(Debug, Clone, Deserialize)]
pub struct CallPluginHangupRequest {
    #[serde(alias = "conversationId")]
    pub conversation_id: String,
    #[serde(alias = "callId")]
    pub call_id: String,
    #[serde(default)]
    pub mode: Option<String>,
    pub reason: String,
    #[serde(default, alias = "durationSeconds")]
    pub duration_seconds: Option<i32>,
    #[serde(default, alias = "reasonCode")]
    pub reason_code: Option<String>,
    #[serde(default, alias = "visibilityScope")]
    pub visibility_scope: Option<String>,
    #[serde(default, alias = "timeoutSeconds")]
    pub timeout_seconds: Option<u32>,
    #[serde(default, alias = "toUserId")]
    pub to_user_id: Option<String>,
    #[serde(default, alias = "closeRoomIfVacant")]
    pub close_room_if_vacant: Option<bool>,
}

/// [`commands::call_signal::sdk_send_call_reject`]
#[derive(Debug, Clone, Deserialize)]
pub struct CallPluginRejectRequest {
    #[serde(alias = "conversationId")]
    pub conversation_id: String,
    #[serde(alias = "callId")]
    pub call_id: String,
    pub reason: String,
    pub code: i32,
    #[serde(default, alias = "toUserId")]
    pub to_user_id: Option<String>,
}

/// [`commands::call_signal::sdk_send_call_ice_candidate`]
#[derive(Debug, Clone, Deserialize)]
pub struct CallPluginIceCandidateRequest {
    #[serde(alias = "conversationId")]
    pub conversation_id: String,
    #[serde(alias = "callId")]
    pub call_id: String,
    #[serde(default, alias = "toUserId")]
    pub to_user_id: Option<String>,
    pub candidate: String,
    pub sdp_mid: String,
    pub sdp_mline_index: i32,
}

/// [`commands::call_signal::sdk_send_call_webrtc_sdp`] — P2P SDP，`sdp_type` = `offer` \| `answer`。
#[derive(Debug, Clone, Deserialize)]
pub struct CallPluginWebrtcSdpRequest {
    pub conversation_id: String,
    pub call_id: String,
    #[serde(default)]
    pub to_user_id: Option<String>,
    pub sdp_type: String,
    pub sdp: String,
}

/// [`commands::call_signal::sdk_build_call_media_constraints`]
#[derive(Debug, Clone, Deserialize)]
pub struct CallPluginMediaConstraintsRequest {
    pub include_video: bool,
    #[serde(default)]
    pub profile_json: Option<String>,
}

/// 通话信令事件（与 `flare.common.v1.CallSignalEvent` 对齐：`signal` → `variant` + `body` JSON）
#[derive(Debug, Clone, Serialize)]
pub struct CallSignalPayload {
    pub conversation_id: String,
    pub call_id: String,
    pub from_user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_user_id: Option<String>,
    #[serde(default)]
    pub audience: serde_json::Value,
    #[serde(default)]
    pub media_session: serde_json::Value,
    #[serde(default)]
    pub transport: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_expires_at_unix: Option<i64>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub ext: HashMap<String, String>,
    /// `signal` oneof 分支名（与 `flare_sdk_plugin_call::signaling::variant_name` 一致）
    pub variant: String,
    /// 与 `variant` 对应的结构化体（camelCase）
    pub body: serde_json::Value,
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
pub struct SyncRunPayload {
    pub run_id: String,
    pub trigger: String,
    pub scope: String,
    pub visibility: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncProgressPayload {
    #[serde(flatten)]
    pub run: SyncRunPayload,
    pub task: String,
    pub progress: f32,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncCompletedPayload {
    #[serde(flatten)]
    pub run: SyncRunPayload,
    pub task: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncFailedPayload {
    #[serde(flatten)]
    pub run: SyncRunPayload,
    pub task: String,
    pub error: String,
}

/// 同步阶段结束（Init / Background）
#[derive(Debug, Clone, Serialize)]
pub struct SyncFinishedPayload {
    #[serde(flatten)]
    pub run: SyncRunPayload,
    pub phase: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncStartedPayload {
    #[serde(flatten)]
    pub run: SyncRunPayload,
}

/// `{}` — 无字段的 ACK 类事件（connected 等）
#[derive(Debug, Clone, Serialize)]
pub struct EmptyPayload {}

pub type ConnectedPayload = EmptyPayload;

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
