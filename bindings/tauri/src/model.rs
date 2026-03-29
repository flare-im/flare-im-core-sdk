//! Tauri IPC 用的可序列化类型与事件 payload；业务在 core-sdk。

use flare_proto::common::SendAck as ProtoSendAck;
use serde::{Deserialize, Serialize};

// ---------- 上层可传入的 SDK 配置（camelCase，与 [flare_im_core_sdk::client::SdkConfigOverlay] 一致） ----------

pub use flare_im_core_sdk::client::SdkConfigOverlay as SdkConfigOptions;

/// sdk_init 入参：前端传对象 { environment?, sdkConfig? }，保证反序列化一致
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SdkInitArgs {
    pub environment: Option<String>,
    pub sdk_config: Option<SdkConfigOptions>,
}

/// 消息：直接使用 SDK 的 IMMessage（序列化由 core-sdk 实现，camelCase + contentDecoded/content）
pub use flare_im_core_sdk::model::IMMessage;

// ---------- 事件 payload（Tauri emit） ----------

/// 发消息回执（与 proto SendAck 字段对齐，供 command / `im://send_ack`）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendAckPayload {
    pub client_msg_id: String,
    pub server_msg_id: String,
    pub seq: u64,
    pub conversation_id: String,
    pub success: bool,
    pub error_code: i32,
    pub error_message: String,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateChangedPayload {
    pub state: String,
}

/// 同步状态变更（Idle / Syncing / CatchingUp / Error）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStateChangedPayload {
    pub state: String,
}

/// 消息发送失败（client_msg_id + 原因）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSendFailedPayload {
    pub client_msg_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRecalledPayload {
    pub conversation_id: String,
    pub message_id: String,
    pub recaller_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDeletedPayload {
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationsSyncedPayload {
    pub conversation_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgressPayload {
    pub task: String,
    pub progress: f32,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncCompletedPayload {
    pub task: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncFailedPayload {
    pub task: String,
    pub error: String,
}

/// 同步开始（无额外字段，前端用于显示「同步中」）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStartedPayload {}

/// 同步阶段结束（Init / Background）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncFinishedPayload {
    pub phase: String,
}

/// 连接已建立
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedPayload {}

/// 连接断开
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisconnectedPayload {
    pub reason: String,
}

/// 被踢下线
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KickedOffPayload {
    pub reason: String,
}

/// Token 过期
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenExpiredPayload {
    pub message: String,
}

/// 服务端错误
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerErrorPayload {
    pub code: i32,
    pub message: String,
}

/// 重连中
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconnectingPayload {
    pub attempt: u32,
}

/// 会话信息变更
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationUpdatedPayload {
    pub conversation_id: String,
}

/// 会话删除
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDeletedPayload {
    pub conversation_id: String,
}

/// 未读数变更
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnreadCountChangedPayload {
    pub conversation_id: String,
    pub unread_count: u32,
}

/// 正在输入
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypingPayload {
    pub conversation_id: String,
    pub user_id: String,
    pub typing: bool,
}

/// 扩展事件（payload 为原始字节，前端按需解码）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
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
    MessageDeleted(MessageDeletedPayload),
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
