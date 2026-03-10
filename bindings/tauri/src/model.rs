//! 语言模型层：SDK Core ← model → App
//!
//! 与 SDK/proto 解耦的契约类型，仅做序列化（serde）；binding 不直接使用 JSON。
//! 传输层可由 Tauri 默认 JSON 或后续改为 proto bytes。

use serde::{Deserialize, Serialize};

// ---------- 上层可传入的 SDK 配置（可选覆盖，camelCase 供前端） ----------

/// sdk_init 入参：前端传对象 { environment?, sdkConfig? }，保证反序列化一致
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SdkInitArgs {
    pub environment: Option<String>,
    pub sdk_config: Option<SdkConfigOptions>,
}

/// SDK 配置可选覆盖：与 [flare_im_core_sdk::client::config::SdkConfig] 对应，全部可选，用于 sdk_init 个性化。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SdkConfigOptions {
    pub ws_url: Option<String>,
    pub quic_url: Option<String>,
    pub http_url: Option<String>,
    pub connect_timeout_secs: Option<u64>,
    pub reconnect_interval_secs: Option<u64>,
    pub max_reconnect_attempts: Option<u32>,
    pub sync_batch_size: Option<u32>,
    pub ack_timeout_secs: Option<u64>,
    pub ack_max_retries: Option<u32>,
    pub enable_metrics: Option<bool>,
}

/// 消息（与 proto Message 对应，camelCase 供前端）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageOut {
    pub server_id: String,
    pub conversation_id: String,
    pub client_msg_id: String,
    pub sender_id: String,
    pub receiver_id: String,
    pub seq: u64,
    pub timestamp: String,
    pub conversation_type: i32,
    pub message_type: i32,
    pub content: Vec<u8>,
    pub status: i32,
    #[serde(skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub extra: std::collections::HashMap<String, String>,
}

/// 会话最后一条消息预览
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePreviewOut {
    pub message_id: String,
    pub sender_id: String,
    #[serde(rename = "type")]
    pub type_: i32,
    pub text: String,
    pub time: String,
}

/// 输入状态（占位用，SDK 无直接接口时返回空列表）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputStateOut {
    pub conversation_id: String,
    pub user_id: String,
    pub typing: bool,
}

/// 会话摘要（与 proto ConversationSummary 对应）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummaryOut {
    pub conversation_id: String,
    pub conversation_type: String,
    pub business_type: String,
    pub display_name: String,
    pub avatar_url: String,
    pub unread_count: u32,
    pub max_seq: u64,
    pub last_read_seq: u64,
    pub last_message: Option<MessagePreviewOut>,
    pub updated_at: String,
    pub created_at: String,
    /// 单聊时对方 user_id（创建会话时写入 ext.peer_id），发送消息时用作 receiver_id
    pub peer_id: Option<String>,
}

// ---------- 事件 payload（Tauri emit 用，统一 Serialize，不碰 JSON） ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateChangedPayload {
    pub state: String,
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
    pub conversations: Vec<ConversationSummaryOut>,
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

/// 事件 payload 枚举，供 Tauri emit 使用；序列化时无标签，仅内层字段
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum EventPayload {
    StateChanged(StateChangedPayload),
    Message(MessageOut),
    MessageRecalled(MessageRecalledPayload),
    MessageDeleted(MessageDeletedPayload),
    ConversationsSynced(ConversationsSyncedPayload),
    SyncProgress(SyncProgressPayload),
    SyncCompleted(SyncCompletedPayload),
    SyncFailed(SyncFailedPayload),
}
