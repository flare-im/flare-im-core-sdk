//! 消息模型 — 与 proto Message 对齐，定义 SDK 统一使用的 IMMessage（属性与 Message 一致，content 使用 Elem）
//!
//! ## content 设计说明（为何在 IMMessage 内直接持有一个解码后的 Elem）
//!
//! **选择：在 IMMessage 构造时用 `decode_content_bytes` + `decoded_content_to_elem` 解码并缓存 `content: Option<Elem>`。**
//!
//! - **一次解码、多处使用**：列表/详情/搜索等都会用到同一条消息的展示内容，若由上层按需调用
//!   `decoded_content_to_elem`，要么重复解码（浪费 CPU/分配），要么每层自己缓存，逻辑分散且易错。
//! - **与常见 IM SDK 一致**：多数 SDK 在消息对象上暴露“解码后的内容”为构造时或懒计算的一次性结果，
//!   上层只读即可，无需关心解码时机。
//! - **职责清晰**：解码与类型映射集中在 model 层完成，接入层（Tauri/FFI）只消费 `content`，无需依赖
//!   `DecodedContent` 与 `decoded_content_to_elem` 的细节；需要裸字节时仍使用 `content_bytes`。
//!
//! 因此不采用“仅暴露 `decoded_content_to_elem` 给上层按需调用”的方案，而是保留 `content` 字段。

use std::collections::HashMap;

use flare_proto::common::{Message as ProtoMessage, OfflinePushInfo};
use serde::{Deserialize, Serialize};

use crate::model::decoder::decode_content_bytes;
use crate::model::message_elem::{
    Elem, decoded_content_to_elem, elem_plain_summary, elem_preview_storage_payload,
    elem_to_message_content,
};
use crate::model::preview_storage::is_redundant_content_text_extra;
use crate::util::date::{ms_to_prost_timestamp, prost_timestamp_to_ms};
use prost::Message as ProstMessage;

/// 从下行 `Message.extra` 推断是否已编辑（与 storage writer 写入的 `messageFsmState`、`currentEditVersion` 对齐）。
fn is_edited_from_extra(extra: &HashMap<String, String>) -> bool {
    if extra.get("messageFsmState").map(|s| s.as_str()) == Some("EDITED") {
        return true;
    }
    if let Some(v) = extra.get("currentEditVersion")
        && v.trim().parse::<i32>().unwrap_or(0) > 0
    {
        return true;
    }
    false
}

const REACTIONS_JSON_KEY: &str = "reactionsJson";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReactionEntry {
    pub emoji: String,
    #[serde(default)]
    #[serde(alias = "userIds")]
    pub user_ids: Vec<String>,
    pub count: u32,
}

pub(crate) fn parse_reactions_from_extra(extra: &HashMap<String, String>) -> Vec<ReactionEntry> {
    let raw = match extra.get(REACTIONS_JSON_KEY) {
        Some(v) if !v.trim().is_empty() => v,
        _ => return Vec::new(),
    };
    // 兼容两种形态：
    // 1) 直接数组 JSON：`[{"emoji":"👍","user_ids":["u1"],"count":1}]`
    // 2) 二次编码字符串：`"[{\"emoji\":\"👍\",\"user_ids\":[\"u1\"],\"count\":1]"`
    if let Ok(list) = serde_json::from_str::<Vec<ReactionEntry>>(raw) {
        return list;
    }
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::String(inner)) => {
            serde_json::from_str::<Vec<ReactionEntry>>(&inner).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

#[cfg(feature = "storage-sqlite")]
pub(crate) fn has_reaction_snapshot_in_extra(extra: &HashMap<String, String>) -> bool {
    extra.contains_key(REACTIONS_JSON_KEY)
}

fn write_reactions_to_extra(extra: &mut HashMap<String, String>, reactions: &[ReactionEntry]) {
    if reactions.is_empty() {
        extra.remove(REACTIONS_JSON_KEY);
        return;
    }
    if let Ok(s) = serde_json::to_string(reactions) {
        extra.insert(REACTIONS_JSON_KEY.to_string(), s);
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageLocalState {
    /// 是否发送中
    pub sending: bool,
    /// 是否失败
    pub failed: bool,
    /// 本地消息
    pub is_local: bool,
    /// 本地列表排序时间（毫秒），**不是**服务端会话 `seq`。
    ///
    /// 用途：待发/未 ACK 消息常保持 `seq == 0`，会话列表「最新一页」在 SQLite 中按本字段与时间戳排序，
    /// 避免仅占位 `seq`；持久化时若为 `0`，仓储层会回退为 `max(timestamp, client_timestamp, 墙钟)`。
    pub sort_ts: u64,
}

/// SDK 层消息类型：与 message.proto 的 Message 属性一致，content 为解码后的 Elem；
/// 另保留 content_bytes 与 proto 一致用于持久化/网络，并增加发送者展示字段。
/// 序列化：字段名为 snake_case（默认）；`content_bytes` 对 JSON `skip`（避免把二进制塞进 WebView）。**上行发送**时若仅有 `content`（Elem），
/// [`to_proto`](IMMessage::to_proto) 会从 `content` 重编码为 `MessageContent` 字节，与协议一致。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IMMessage {
    // ==============================
    // Identity
    // ==============================
    /// 服务端唯一ID
    pub server_id: String,

    /// 客户端生成ID（去重）
    pub client_msg_id: String,

    /// 会话ID
    pub conversation_id: String,

    /// 会话类型
    pub conversation_type: i32,

    /// 会话频道 ID：单聊=对方 user_id，群聊=群 ID，频道/话题=对应 ID
    pub channel_id: String,

    /// 发送者
    pub sender_id: String,

    /// 消息来源
    pub source: i32,

    // ==============================
    // Sequence / Ordering
    // ==============================
    /// 会话内序列号
    pub seq: u64,

    /// 服务端时间（毫秒）
    pub timestamp: u64,

    /// 客户端时间（毫秒）
    pub client_timestamp: u64,

    // ==============================
    // Message Content
    // ==============================
    /// 消息类型
    pub message_type: i32,

    /// proto结构
    pub content: Option<Elem>,

    /// 原始二进制（反序列化时由上层编码生成，不从前端传入）
    #[serde(skip, default)]
    pub content_bytes: Vec<u8>,

    // ==============================
    // Sender Display
    // ==============================
    pub sender_name: String,

    pub sender_avatar: String,

    /// SDK计算展示名
    pub sender_display_name: String,

    // ==============================
    // Reply / Quote
    // ==============================
    pub reply_to: Option<String>,

    pub quote_preview: Option<String>,

    // ==============================
    // Status
    // ==============================
    pub status: i32,

    pub is_read: bool,

    pub is_recalled: bool,

    pub is_edited: bool,

    // ==============================
    // Burn-after-read FSM
    // ==============================
    /// 是否启用阅后即焚
    pub burn_enabled: bool,

    /// 首次阅读后多少秒焚毁（服务端权威）
    pub burn_after_read_seconds: Option<i64>,

    /// 阅后即焚状态（见 proto BurnStatus）
    pub burn_status: i32,

    /// 首次阅读时间（Unix 秒）
    pub first_read_at: Option<i64>,

    /// 计划焚毁时间（Unix 秒）
    pub burn_at: Option<i64>,

    /// 实际焚毁时间（Unix 秒）
    pub burned_at: Option<i64>,

    // ==============================
    // Mention
    // ==============================
    pub mention_users: Vec<String>,

    pub mention_all: bool,

    // ==============================
    // Push
    // ==============================
    #[serde(skip, default)]
    pub offline_push_info: Option<OfflinePushInfo>,

    // ==============================
    // Extensions
    // ==============================
    pub extra: HashMap<String, String>,

    /// 扩展数据；反序列化时若前端未传或格式不兼容则用 default
    #[serde(default)]
    pub extensions: HashMap<String, Vec<u8>>,

    /// 表情反应快照（由 ReactionEvent 驱动更新并持久化）
    #[serde(default)]
    pub reactions: Vec<ReactionEntry>,

    // ==============================
    // Sync / Version
    // ==============================
    pub version: u64,

    pub updated_at: u64,

    // ==============================
    // Local State（SDK内部）
    // ==============================
    #[serde(skip, default)]
    pub local_state: MessageLocalState,
}

impl IMMessage {
    pub fn new(message: ProtoMessage) -> Self {
        let decoded_opt = decode_content_bytes(&message.content).ok();
        let content = decoded_opt.as_ref().and_then(decoded_content_to_elem);
        let mut extra = message.extra.clone();
        if content.is_none()
            && let Some(ref decoded) = decoded_opt
        {
            let p = decoded.text_preview();
            if !is_redundant_content_text_extra(&p) {
                extra.entry("contentText".into()).or_insert(p);
            }
        }
        let is_edited = is_edited_from_extra(&extra);
        let reactions = parse_reactions_from_extra(&extra);
        let is_recalled = message.status == flare_proto::common::MessageStatus::Recalled as i32;
        let timestamp = prost_timestamp_to_ms(message.timestamp.as_ref());
        let mut reply_to: Option<String> = None;
        let mut quote_preview: Option<String> = None;
        if let Some(Elem::Quote(q)) = content.as_ref() {
            if !q.quoted_message_id.is_empty() {
                reply_to = Some(q.quoted_message_id.clone());
            }
            let preview = if !q.quoted_text_preview.is_empty() {
                q.quoted_text_preview.clone()
            } else {
                q.quoted_content
                    .as_deref()
                    .map(elem_plain_summary)
                    .unwrap_or_default()
            };
            if !preview.is_empty() {
                quote_preview = Some(preview);
            }
        }
        Self {
            server_id: message.server_id,
            conversation_id: message.conversation_id,
            client_msg_id: message.client_msg_id,
            sender_id: message.sender_id,
            source: message.source,
            seq: message.seq,
            timestamp,
            client_timestamp: timestamp,
            conversation_type: message.conversation_type,
            message_type: message.message_type,
            channel_id: message.channel_id,
            sender_name: message.sender_name,
            sender_avatar: message.sender_avatar,
            content,
            content_bytes: message.content,
            reply_to,
            quote_preview,
            status: message.status,
            is_read: false,
            is_recalled,
            is_edited,
            burn_enabled: message.burn_enabled,
            burn_after_read_seconds: message.burn_after_read_seconds,
            burn_status: message.burn_status,
            first_read_at: message.first_read_at,
            burn_at: message.burn_at,
            burned_at: message.burned_at,
            mention_users: Vec::new(),
            mention_all: false,
            offline_push_info: message.offline_push_info,
            extra,
            extensions: message.extensions,
            reactions,
            sender_display_name: String::new(),
            version: 0,
            updated_at: 0,
            local_state: MessageLocalState {
                sending: false,
                failed: false,
                is_local: false,
                sort_ts: timestamp,
            },
        }
    }

    /// 填充发送者资料（显示名称）；头像使用 proto 的 sender_avatar。
    pub fn with_sender_profile(mut self, display_name: impl Into<String>) -> Self {
        self.sender_display_name = display_name.into();
        self
    }

    /// 若 `content_bytes` 为空且存在 `content`（Elem），则编码为 `MessageContent` 写入 `content_bytes`。
    ///
    /// 供发送前落库、可靠队列持久化等与 `to_proto` 一致。
    pub fn materialize_content_bytes_from_elem(&mut self) {
        if !self.content_bytes.is_empty() {
            return;
        }
        if let Some(ref elem) = self.content {
            self.content_bytes = elem_to_message_content(elem).encode_to_vec();
        }
    }

    /// 按 `ReactionAction`（1=ADD, 2=REMOVE）应用一次表情反应变更，并同步到 `extra.reactionsJson`。
    pub fn apply_reaction_change(&mut self, user_id: &str, emoji: &str, action: i32) {
        if user_id.is_empty() || emoji.is_empty() {
            return;
        }
        let add_action = ReactionAction::Add as i32;
        let remove_action = ReactionAction::Remove as i32;
        if action != add_action && action != remove_action {
            return;
        }
        let idx = self.reactions.iter().position(|r| r.emoji == emoji);
        let is_remove = action == remove_action;
        match idx {
            Some(i) => {
                let r = &mut self.reactions[i];
                if is_remove {
                    r.user_ids.retain(|u| u != user_id);
                } else if !r.user_ids.iter().any(|u| u == user_id) {
                    r.user_ids.push(user_id.to_string());
                }
                if r.user_ids.is_empty() {
                    self.reactions.remove(i);
                } else {
                    r.count = r.user_ids.len() as u32;
                }
            }
            None => {
                if !is_remove {
                    self.reactions.push(ReactionEntry {
                        emoji: emoji.to_string(),
                        user_ids: vec![user_id.to_string()],
                        count: 1,
                    });
                }
            }
        }
        write_reactions_to_extra(&mut self.extra, &self.reactions);
    }

    /// 转为 proto Message（用于持久化/网络发送）。
    ///
    /// `content` 优先使用 `content_bytes`；为空时从解码后的 `content`（Elem）编码为 `MessageContent`，
    /// 否则 Tauri/JSON 等路径在 `sdk_send` 时会发出空 `Message.content`。
    pub fn to_proto(&self) -> ProtoMessage {
        let timestamp = ms_to_prost_timestamp(self.timestamp);
        let content = if !self.content_bytes.is_empty() {
            self.content_bytes.clone()
        } else if let Some(ref elem) = self.content {
            elem_to_message_content(elem).encode_to_vec()
        } else {
            Vec::new()
        };
        ProtoMessage {
            server_id: self.server_id.clone(),
            conversation_id: self.conversation_id.clone(),
            client_msg_id: self.client_msg_id.clone(),
            sender_id: self.sender_id.clone(),
            source: self.source,
            seq: self.seq,
            timestamp,
            conversation_type: self.conversation_type,
            message_type: self.message_type,
            channel_id: self.channel_id.clone(),
            sender_name: self.sender_name.clone(),
            sender_avatar: self.sender_avatar.clone(),
            content,
            status: self.status,
            burn_enabled: self.burn_enabled,
            burn_after_read_seconds: self.burn_after_read_seconds,
            burn_status: self.burn_status,
            first_read_at: self.first_read_at,
            burn_at: self.burn_at,
            burned_at: self.burned_at,
            offline_push_info: self.offline_push_info.clone(),
            extra: self.extra.clone(),
            extensions: self.extensions.clone(),
        }
    }

    pub fn conversation_id(&self) -> &str {
        &self.conversation_id
    }

    pub fn sender_id(&self) -> &str {
        &self.sender_id
    }

    pub fn server_id(&self) -> &str {
        &self.server_id
    }

    pub fn client_msg_id(&self) -> &str {
        &self.client_msg_id
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn status(&self) -> i32 {
        self.status
    }

    /// 供 `messages.text` 与 `conversations.last_message_preview`：JSON 字符串形态的 [`crate::model::preview_storage::PreviewStoragePayload`]（稳定 `k` + 参数 `a`），供应用端 i18n；与 [`elem_plain_summary`] / [`elem_preview_storage_payload`](crate::model::message_elem::elem_preview_storage_payload) 一致。
    pub fn text_for_storage(&self) -> Option<String> {
        self.content.as_ref().and_then(|e| {
            let p = elem_preview_storage_payload(e);
            if p.is_empty_for_last_preview() {
                return None;
            }
            serde_json::to_string(&p).ok()
        })
    }

    /// 展示用发送者名称：优先 sender_display_name，否则 sender_name，再否则 sender_id
    pub fn display_name(&self) -> &str {
        if !self.sender_display_name.is_empty() {
            &self.sender_display_name
        } else if !self.sender_name.is_empty() {
            &self.sender_name
        } else {
            &self.sender_id
        }
    }

    /// 发送者头像 URL（来自 proto sender_avatar）
    pub fn avatar_url(&self) -> &str {
        &self.sender_avatar
    }
}

impl From<ProtoMessage> for IMMessage {
    fn from(message: ProtoMessage) -> Self {
        Self::new(message)
    }
}

// Re-export proto 消息相关类型（供上层使用）
pub use flare_proto::common::MessageType;
pub use flare_proto::common::{
    AudioInfo, BurnStatus, ConversationType, DeleteScope, DeleteType, ImageInfo, MarkType, Message,
    MessageSource, MessageStatus, ReactionAction, SendAck, VideoInfo,
};
