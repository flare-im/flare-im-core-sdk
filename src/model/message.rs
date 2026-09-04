//! 消息模型 — 与 proto Message 对齐，定义 SDK 统一使用的 IMMessage（属性与 Message 一致，content 使用 Elem）
//!
//! ## content 设计说明（为何在 IMMessage 内直接持有一个解码后的 Elem）
//!
//! **选择：在 IMMessage 构造时用协议层 `MessageContent` + `decoded_content_to_elem` 解码并缓存 `content: Option<Elem>`。**
//!
//! - **一次解码、多处使用**：列表/详情/搜索等都会用到同一条消息的展示内容，若由上层按需调用
//!   `decoded_content_to_elem`，要么重复解码（浪费 CPU/分配），要么每层自己缓存，逻辑分散且易错。
//! - **与常见 IM SDK 一致**：多数 SDK 在消息对象上暴露“解码后的内容”为构造时或懒计算的一次性结果，
//!   上层只读即可，无需关心解码时机。
//! - **职责清晰**：解码与类型映射集中在 model 层完成，接入层（Tauri/FFI）只消费 `content`，无需依赖
//!   `DecodedContent` 与 `decoded_content_to_elem` 的细节；协议边界仍保留 `encoded_content` 作为强类型内容的编码缓存。
//!
//! 因此不采用“仅暴露 `decoded_content_to_elem` 给上层按需调用”的方案，而是保留 `content` 字段。

use std::cmp::Ordering;
use std::collections::HashMap;

use flare_proto::common::{
    Message as ProtoMessage, MessageContent, MessageRetentionPolicy, MessageRetentionState,
    OfflinePushInfo,
};
use schemars::JsonSchema;
use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};

use crate::content::decoder::decode_content_bytes;
use crate::content::message_elem::{
    Elem, decoded_content_to_elem, elem_plain_summary, elem_preview_storage_payload,
    elem_to_message_content,
};
use crate::content::preview_storage::is_redundant_content_text_extra;
use prost::Message as ProstMessage;

/// 从下行 `Message.attributes` 推断是否已编辑（与 storage writer 写入的 `messageFsmState`、`currentEditVersion` 对齐）。
fn is_edited_from_attributes(attributes: &HashMap<String, String>) -> bool {
    if attributes.get("messageFsmState").map(|s| s.as_str()) == Some("EDITED") {
        return true;
    }
    if let Some(v) = attributes.get("currentEditVersion")
        && v.trim().parse::<i32>().unwrap_or(0) > 0
    {
        return true;
    }
    false
}

const REACTIONS_JSON_KEY: &str = "reactionsJson";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReactionEntry {
    pub emoji: String,
    // 容错读旧服务端的 snake_case key（曾因 server 发 user_ids/客户端要 userIds 导致同步后对端反应丢失）。
    #[serde(default, alias = "user_ids")]
    pub user_ids: Vec<String>,
    pub count: u32,
}

pub(crate) fn parse_reactions_from_attributes(
    attributes: &HashMap<String, String>,
) -> Vec<ReactionEntry> {
    let raw = match attributes.get(REACTIONS_JSON_KEY) {
        Some(v) if !v.trim().is_empty() => v,
        _ => return Vec::new(),
    };
    serde_json::from_str::<Vec<ReactionEntry>>(raw).unwrap_or_default()
}

#[cfg(feature = "storage-sqlite")]
pub(crate) fn has_reaction_snapshot_in_attributes(attributes: &HashMap<String, String>) -> bool {
    attributes.contains_key(REACTIONS_JSON_KEY)
}

fn write_reactions_to_attributes(
    attributes: &mut HashMap<String, String>,
    reactions: &[ReactionEntry],
) {
    if reactions.is_empty() {
        attributes.remove(REACTIONS_JSON_KEY);
        return;
    }
    if let Ok(s) = serde_json::to_string(reactions) {
        attributes.insert(REACTIONS_JSON_KEY.to_string(), s);
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MessageLocalState {
    /// 是否发送中
    pub sending: bool,
    /// 是否失败
    pub failed: bool,
    /// 本地消息
    pub is_local: bool,
    /// 本地媒体上传中；仅用于 SDK 本地时间线展示，不写入服务端协议语义。
    #[serde(default)]
    pub uploading: bool,
    /// 本地媒体上传进度，范围 0..=100。
    #[serde(default)]
    pub upload_progress: u32,
    /// 本地列表排序时间（毫秒），**不是**服务端会话 `conversation_seq`。
    ///
    /// 用途：待发/未 ACK 消息常保持 `conversation_seq == 0`，可用本字段稳定停留在本地时间线尾部。
    /// 已分配 `conversation_seq` 的服务端消息必须回到 seq 优先排序，避免设备时钟偏移污染权威顺序。
    pub sort_ts: u64,
}

/// SDK 层消息类型：与 message.proto 的 Message 属性一致，content 为解码后的 Elem；
/// 另保留 raw_content 与 proto 一致用于持久化/网络，并增加发送者展示字段。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
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
    /// 会话内持久化 replay 序列号。
    pub conversation_seq: u64,

    /// 消息创建时间，Unix epoch millis。
    pub created_at: u64,

    /// 客户端本地创建时间，Unix epoch millis。
    pub client_created_at: u64,

    // ==============================
    // Message Content
    // ==============================
    /// 消息类型
    pub message_type: i32,

    /// proto结构
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Elem>,

    /// 当前 `MessageContent` 的 protobuf 编码缓存（反序列化时由上层编码生成，不从前端传入）。
    #[serde(skip, default)]
    pub encoded_content: Vec<u8>,

    /// 列表、搜索、绑定层使用的纯文本预览。
    #[serde(default)]
    pub text_preview: String,

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote_preview: Option<String>,

    /// 话题/线程根消息 ID；普通消息为空，话题回复使用该 typed field。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,

    // ==============================
    // Status
    // ==============================
    pub status: i32,

    pub is_read: bool,

    pub is_recalled: bool,

    pub is_edited: bool,

    /// 保留/过期策略（服务端权威）。
    #[serde(skip, default)]
    pub retention_policy: Option<MessageRetentionPolicy>,

    /// 保留/过期生命周期（服务端权威）。
    #[serde(skip, default)]
    pub retention_state: Option<MessageRetentionState>,

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
    pub attributes: HashMap<String, String>,

    /// 扩展数据；未提供时为空。
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
    // Local State（SDK 本地状态；绑定层用于展示发送中/失败/本地待 ACK 状态）
    // ==============================
    #[serde(default)]
    pub local_state: MessageLocalState,
}

impl Serialize for IMMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("IMMessage", 33)?;
        state.serialize_field("serverId", &self.server_id)?;
        state.serialize_field("clientMsgId", &self.client_msg_id)?;
        state.serialize_field("conversationId", &self.conversation_id)?;
        state.serialize_field("conversationType", &self.conversation_type)?;
        state.serialize_field("channelId", &self.channel_id)?;
        state.serialize_field("senderId", &self.sender_id)?;
        state.serialize_field("source", &self.source)?;
        state.serialize_field("conversationSeq", &self.conversation_seq)?;
        state.serialize_field("createdAt", &self.created_at)?;
        state.serialize_field("clientCreatedAt", &self.client_created_at)?;
        state.serialize_field("messageType", &self.message_type)?;
        if let Some(content) = &self.content {
            state.serialize_field("content", content)?;
        }
        state.serialize_field("textPreview", &self.text_preview)?;
        state.serialize_field("senderName", &self.sender_name)?;
        state.serialize_field("senderAvatar", &self.sender_avatar)?;
        state.serialize_field("senderDisplayName", &self.sender_display_name)?;
        if let Some(reply_to) = &self.reply_to {
            state.serialize_field("replyTo", reply_to)?;
        }
        if let Some(quote_preview) = &self.quote_preview {
            state.serialize_field("quotePreview", quote_preview)?;
        }
        if let Some(thread_id) = &self.thread_id {
            state.serialize_field("threadId", thread_id)?;
        }
        state.serialize_field("status", &self.status)?;
        state.serialize_field("isRead", &self.is_read)?;
        state.serialize_field("isRecalled", &self.is_recalled)?;
        state.serialize_field("isEdited", &self.is_edited)?;
        state.serialize_field("mentionUsers", &self.mention_users)?;
        state.serialize_field("mentionAll", &self.mention_all)?;
        state.serialize_field("attributes", &self.attributes)?;
        state.serialize_field("extensions", &self.extensions)?;
        state.serialize_field("reactions", &self.reactions)?;
        state.serialize_field("version", &self.version)?;
        state.serialize_field("updatedAt", &self.updated_at)?;
        state.serialize_field("localState", &self.local_state)?;
        state.serialize_field("timelineKey", &self.timeline_key())?;
        state.serialize_field("timelineSortTs", &self.timeline_sort_ts())?;
        state.end()
    }
}

impl IMMessage {
    pub fn new(message: ProtoMessage) -> Self {
        let encoded_content = message
            .content
            .as_ref()
            .map(|content| content.encode_to_vec())
            .unwrap_or_default();
        let decoded_opt = decode_content_bytes(&encoded_content).ok();
        // 提及在 wire 上走 content 里的 mentions；`mention_all` / `mention_users`
        // 是本地派生字段。发送侧一直写进 content，接收侧却从来没读回来——
        // 于是对端收到的消息 mentionAll 恒为 false（跨端 @ 高亮与「只接收@我」都因此失效）。
        let decoded_mentions = message
            .content
            .as_ref()
            .map(crate::content::mentions_from_content)
            .unwrap_or_default();
        let content = decoded_opt.as_ref().and_then(decoded_content_to_elem);
        let text_preview = decoded_opt
            .as_ref()
            .map(|decoded| decoded.text_preview())
            .unwrap_or_default();
        let mut attributes = message.attributes.clone();
        if content.is_none()
            && let Some(ref decoded) = decoded_opt
        {
            let p = decoded.text_preview();
            if !is_redundant_content_text_extra(&p) {
                attributes.entry("contentText".into()).or_insert(p);
            }
        }
        let is_edited = is_edited_from_attributes(&attributes);
        let reactions = parse_reactions_from_attributes(&attributes);
        let is_recalled = message.status == flare_proto::common::MessageStatus::Recalled as i32;
        let created_at = message.created_at.max(0) as u64;
        let thread_id = message
            .thread_id
            .filter(|thread_id| !thread_id.trim().is_empty());
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
            conversation_seq: message.conversation_seq,
            created_at,
            client_created_at: created_at,
            conversation_type: message.conversation_type,
            message_type: message.message_type,
            channel_id: message.channel_id,
            sender_name: message.sender_name,
            sender_avatar: message.sender_avatar,
            content,
            encoded_content,
            text_preview,
            reply_to,
            quote_preview,
            thread_id,
            status: message.status,
            is_read: false,
            is_recalled,
            is_edited,
            retention_policy: message.retention_policy,
            retention_state: message.retention_state,
            mention_users: decoded_mentions.user_ids,
            mention_all: decoded_mentions.mention_all,
            offline_push_info: message.offline_push_info,
            attributes,
            extensions: message.extensions,
            reactions,
            sender_display_name: String::new(),
            version: 0,
            updated_at: 0,
            local_state: MessageLocalState {
                sending: false,
                failed: false,
                is_local: false,
                uploading: false,
                upload_progress: 0,
                sort_ts: created_at,
            },
        }
    }

    /// 填充发送者资料（显示名称）；头像使用 proto 的 sender_avatar。
    pub fn with_sender_profile(mut self, display_name: impl Into<String>) -> Self {
        self.sender_display_name = display_name.into();
        self
    }

    /// 若 `encoded_content` 为空且存在 `content`（Elem），则编码为 `MessageContent` 写入 `encoded_content`。
    ///
    /// 供发送前落库、可靠队列持久化等与 `to_proto` 一致。
    pub fn materialize_encoded_content_from_elem(&mut self) {
        if !self.encoded_content.is_empty() {
            return;
        }
        if let Some(ref elem) = self.content {
            self.text_preview = elem_plain_summary(elem);
            self.encoded_content = elem_to_message_content(elem).encode_to_vec();
        }
    }

    /// 按 `ReactionAction`（1=ADD, 2=REMOVE）应用一次表情反应变更，并同步到 `attributes.reactionsJson`。
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
        write_reactions_to_attributes(&mut self.attributes, &self.reactions);
    }

    /// 转为 proto Message（用于持久化/网络发送）。
    pub fn to_proto(&self) -> ProtoMessage {
        let content = self
            .encoded_content
            .is_empty()
            .then(|| self.content.as_ref().map(elem_to_message_content))
            .flatten()
            .or_else(|| MessageContent::decode(self.encoded_content.as_slice()).ok())
            .or_else(|| self.content.as_ref().map(elem_to_message_content));
        ProtoMessage {
            server_id: self.server_id.clone(),
            conversation_id: self.conversation_id.clone(),
            client_msg_id: self.client_msg_id.clone(),
            sender_id: self.sender_id.clone(),
            source: self.source,
            conversation_seq: self.conversation_seq,
            created_at: self.created_at as i64,
            conversation_type: self.conversation_type,
            message_type: self.message_type,
            message_seq: None,
            channel_id: self.channel_id.clone(),
            sender_name: self.sender_name.clone(),
            sender_avatar: self.sender_avatar.clone(),
            thread_id: self.thread_id.clone(),
            content,
            status: self.status,
            retention_policy: self.retention_policy.clone(),
            retention_state: self.retention_state.clone(),
            offline_push_info: self.offline_push_info.clone(),
            attributes: self.attributes.clone(),
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

    pub fn conversation_seq(&self) -> u64 {
        self.conversation_seq
    }

    pub fn status(&self) -> i32 {
        self.status
    }

    /// UI/平台 SDK 使用的稳定时间线行 key。
    ///
    /// 服务端已持久化消息以 `server_id` 为权威；`client_msg_id` 只用于尚未 ACK 的本地行。
    /// 下行历史中不同 `server_id` 可能带相同 `client_msg_id`，不能因此被视为同一时间线行。
    pub fn timeline_key(&self) -> String {
        if !self.server_id.trim().is_empty() {
            return format!("server:{}", self.server_id);
        }
        if !self.client_msg_id.trim().is_empty() {
            return format!("client:{}", self.client_msg_id);
        }
        if self.conversation_seq > 0 {
            return format!("seq:{}:{}", self.conversation_id, self.conversation_seq);
        }
        format!("ts:{}:{}", self.conversation_id, self.timeline_sort_ts())
    }

    /// 时间线/最新窗口排序时间。
    ///
    /// 已分配 `conversation_seq` 后，服务端顺序是权威，排序时间只作为展示和同 seq 兜底，不再让本地 `sort_ts`
    /// 或偏移的客户端时间把历史消息顶到最新窗口。
    /// 不使用 `updated_at`，避免编辑、状态回写等非新消息变更把历史消息重新顶到列表末尾。
    pub fn timeline_sort_ts(&self) -> u64 {
        if self.conversation_seq > 0 {
            if self.created_at > 0 {
                return self.created_at;
            }
            if self.client_created_at > 0 {
                return self.client_created_at;
            }
            return self.local_state.sort_ts;
        }
        self.local_state
            .sort_ts
            .max(self.created_at)
            .max(self.client_created_at)
    }

    /// 展示时间：服务端已分配序列后优先使用服务端时间；本地待 ACK 消息使用本地排序时间。
    ///
    /// 这样客户端时钟异常只会影响 pending 阶段，ACK/sync 收敛后展示时间会回到服务端时间。
    pub fn display_time_ms(&self) -> u64 {
        if self.conversation_seq > 0 && self.created_at > 0 {
            return self.created_at;
        }
        if self.local_state.sort_ts > 0 {
            return self.local_state.sort_ts;
        }
        self.created_at.max(self.client_created_at)
    }

    /// 本地待 ACK 消息在按会话序列展示时应留在尾部，直到服务端分配 `conversation_seq`。
    ///
    /// 失败消息是终态，不再参与“尾随等待 ACK”语义；它应按发送时的 `sort_ts`
    /// 留在原本时间位置，避免后续新消息被插到失败消息上方。
    pub fn is_local_pending_for_timeline(&self) -> bool {
        self.conversation_seq == 0
            && self.local_state.is_local
            && !self.local_state.failed
            && (self.local_state.sending || self.local_state.uploading)
    }

    /// 消息时间线升序比较：服务端序列为主，本地待 ACK 消息稳定留在尾部。
    pub fn compare_for_timeline_asc(left: &Self, right: &Self) -> Ordering {
        let left_seq = left.conversation_seq;
        let right_seq = right.conversation_seq;

        if left_seq > 0 && right_seq > 0 {
            return left_seq
                .cmp(&right_seq)
                .then_with(|| left.timeline_sort_ts().cmp(&right.timeline_sort_ts()))
                .then_with(|| left.timeline_key().cmp(&right.timeline_key()));
        }

        let left_pending = left.is_local_pending_for_timeline();
        let right_pending = right.is_local_pending_for_timeline();
        if left_pending != right_pending && (left_seq > 0 || right_seq > 0) {
            return if left_pending {
                Ordering::Greater
            } else {
                Ordering::Less
            };
        }

        left.timeline_sort_ts()
            .cmp(&right.timeline_sort_ts())
            .then_with(|| left_seq.cmp(&right_seq))
            .then_with(|| left.timeline_key().cmp(&right.timeline_key()))
    }

    /// 最新窗口降序比较：仓储分页先取最近消息，再由上层按时间线需要反转/合并。
    pub fn compare_for_latest_window_desc(left: &Self, right: &Self) -> Ordering {
        Self::compare_for_timeline_asc(right, left)
    }

    /// 供 `messages.text` 与 `conversations.last_message_preview`：JSON 字符串形态的 [`crate::content::preview_storage::PreviewStoragePayload`]（稳定 `k` + 参数 `a`），供应用端 i18n；与 [`elem_plain_summary`] / [`elem_preview_storage_payload`](crate::content::message_elem::elem_preview_storage_payload) 一致。
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
pub use flare_proto::common::send_ack;
pub use flare_proto::common::{
    AudioInfo, ConversationType, DeleteScope, DeleteType, ImageFormat, ImageInfo, MarkType,
    Message, MessageSource, MessageStatus, ReactionAction, SendAck, SendAckDurability, VideoInfo,
};

#[cfg(test)]
mod tests {
    use super::{IMMessage, MessageLocalState, MessageStatus};
    use crate::content::message_elem::{Elem, LinkCardElem, TextElem};
    use flare_proto::common::Message as ProtoMessage;
    use std::collections::HashMap;

    #[test]
    fn reaction_entry_parses_both_userids_and_user_ids() {
        // 契约为 camelCase userIds；同时容错旧服务端的 snake_case user_ids，
        // 否则同步后对端反应会因 user 列表解析为空而被清空（历史缺陷）。
        let camel = r#"[{"emoji":"👽","userIds":["u1","u2"],"count":2}]"#;
        let snake = r#"[{"emoji":"👽","user_ids":["u1","u2"],"count":2}]"#;
        for raw in [camel, snake] {
            let mut attrs = std::collections::HashMap::new();
            attrs.insert(super::REACTIONS_JSON_KEY.to_string(), raw.to_string());
            let r = super::parse_reactions_from_attributes(&attrs);
            assert_eq!(r.len(), 1, "raw={raw}");
            assert_eq!(r[0].emoji, "👽");
            assert_eq!(r[0].user_ids, vec!["u1".to_string(), "u2".to_string()], "user list must parse for raw={raw}");
            assert_eq!(r[0].count, 2);
        }
    }

    fn message(client_msg_id: &str, server_id: &str, seq: u64, created_at: u64) -> IMMessage {
        IMMessage::new(ProtoMessage {
            server_id: server_id.to_string(),
            client_msg_id: client_msg_id.to_string(),
            conversation_id: "conv-1".to_string(),
            sender_id: "user-1".to_string(),
            conversation_seq: seq,
            created_at: created_at as i64,
            ..Default::default()
        })
    }

    #[test]
    fn timeline_key_uses_server_id_after_ack() {
        let mut pending = message("client-1", "", 0, 100);
        let before_ack = pending.timeline_key();

        pending.server_id = "server-1".to_string();
        pending.conversation_seq = 9;
        pending.created_at = 200;

        assert_eq!(before_ack, "client:client-1");
        assert_eq!(pending.timeline_key(), "server:server-1");
    }

    #[test]
    fn timeline_sort_keeps_local_pending_after_sequenced_history() {
        let history = message("client-history", "server-history", 10, 5_000);
        let mut pending = message("client-pending", "", 0, 1_000);
        pending.local_state = MessageLocalState {
            sending: true,
            failed: false,
            is_local: true,
            uploading: false,
            upload_progress: 0,
            sort_ts: 1_000,
        };

        let mut messages = [pending, history];
        messages.sort_by(IMMessage::compare_for_timeline_asc);

        assert_eq!(messages[0].server_id, "server-history");
        assert_eq!(messages[1].client_msg_id, "client-pending");
    }

    #[test]
    fn latest_window_uses_local_sort_time_for_pending_messages() {
        let history = message("client-history", "server-history", 10, 5_000);
        let mut pending = message("client-pending", "", 0, 1_000);
        pending.local_state = MessageLocalState {
            sending: true,
            failed: false,
            is_local: true,
            uploading: false,
            upload_progress: 0,
            sort_ts: 9_000,
        };

        let mut messages = [history, pending];
        messages.sort_by(IMMessage::compare_for_latest_window_desc);

        assert_eq!(messages[0].client_msg_id, "client-pending");
        assert_eq!(messages[0].timeline_sort_ts(), 9_000);
    }

    #[test]
    fn failed_local_message_does_not_pin_after_newer_server_messages() {
        let newer_server = message("client-new", "server-new", 11, 12_000);
        let mut failed = message("client-failed", "client-failed", 0, 9_000);
        failed.status = MessageStatus::Failed as i32;
        failed.local_state = MessageLocalState {
            sending: false,
            failed: true,
            is_local: true,
            uploading: false,
            upload_progress: 100,
            sort_ts: 9_000,
        };

        let mut messages = [newer_server, failed];
        messages.sort_by(IMMessage::compare_for_timeline_asc);

        assert_eq!(messages[0].client_msg_id, "client-failed");
        assert_eq!(messages[1].server_id, "server-new");
        assert!(!messages[0].is_local_pending_for_timeline());
    }

    #[test]
    fn display_time_prefers_server_time_after_ack_and_local_time_for_pending() {
        let mut acked = message("client-1", "server-1", 7, 10_000);
        acked.local_state.sort_ts = 99_000;
        assert_eq!(acked.display_time_ms(), 10_000);

        let mut pending = message("client-2", "", 0, 10_000);
        pending.local_state = MessageLocalState {
            sending: true,
            failed: false,
            is_local: true,
            uploading: false,
            upload_progress: 0,
            sort_ts: 99_000,
        };
        assert_eq!(pending.display_time_ms(), 99_000);
    }

    #[test]
    fn latest_window_uses_sequence_for_acknowledged_messages_despite_future_local_sort_time() {
        let mut skewed_old = message("client-old", "server-old", 10, 1_000);
        skewed_old.local_state = MessageLocalState {
            sending: false,
            failed: false,
            is_local: false,
            uploading: false,
            upload_progress: 0,
            sort_ts: 99_999,
        };
        let newest = message("client-new", "server-new", 11, 2_000);

        let mut messages = [skewed_old, newest];
        messages.sort_by(IMMessage::compare_for_latest_window_desc);

        assert_eq!(messages[0].server_id, "server-new");
        assert_eq!(messages[1].timeline_sort_ts(), 1_000);
    }

    #[test]
    fn serialized_message_exposes_timeline_contract() {
        let mut pending = message("client-pending", "", 0, 1_000);
        pending.local_state = MessageLocalState {
            sending: true,
            failed: false,
            is_local: true,
            uploading: false,
            upload_progress: 0,
            sort_ts: 9_000,
        };

        let value = serde_json::to_value(&pending).expect("serialize message");

        assert_eq!(value["timelineKey"], "client:client-pending");
        assert_eq!(value["timelineSortTs"], 9_000);
        assert_eq!(value["localState"]["sending"], true);
        assert_eq!(value["localState"]["isLocal"], true);
        assert!(value.get("timeline_key").is_none());
        assert!(value.get("local_state").is_none());
    }

    #[test]
    fn serialized_message_omits_absent_optional_fields() {
        let msg = message("client-1", "server-1", 7, 1_000);
        let value = serde_json::to_value(&msg).expect("serialize message");

        for key in ["content", "replyTo", "quotePreview"] {
            assert!(
                value.get(key).is_none(),
                "{key} must be omitted when absent"
            );
        }
    }

    #[test]
    fn serialized_message_uses_camel_case_and_preserves_opaque_map_keys() {
        let mut msg = message("client-1", "server-1", 7, 1_000);
        msg.conversation_type = 1;
        msg.channel_id = "channel-1".to_string();
        msg.message_type = 1;
        msg.content = Some(Elem::Text(TextElem {
            text: "hello".to_string(),
            mentions: vec![],
        }));
        msg.attributes = HashMap::from([("my_custom_key".to_string(), "keep".to_string())]);
        msg.extensions = HashMap::from([("bin_custom_key".to_string(), vec![1, 2, 3])]);

        let value = serde_json::to_value(&msg).expect("serialize message");

        assert_eq!(value["clientMsgId"], "client-1");
        assert_eq!(value["conversationId"], "conv-1");
        assert_eq!(value["conversationSeq"], 7);
        assert_eq!(value["createdAt"], 1_000);
        assert_eq!(value["messageType"], 1);
        assert_eq!(value["content"]["contentType"], "text");
        assert_eq!(value["attributes"]["my_custom_key"], "keep");
        assert_eq!(
            value["extensions"]["bin_custom_key"],
            serde_json::json!([1, 2, 3])
        );
        assert!(value.get("client_msg_id").is_none());
        assert!(value.get("conversation_seq").is_none());
        assert!(value.get("created_at").is_none());
        assert!(value.get("message_type").is_none());
    }

    #[test]
    fn elem_tag_field_is_camel_case_but_tag_values_remain_stable() {
        let elem = Elem::LinkCard(LinkCardElem {
            url: "https://flare.test".to_string(),
            title: "Flare".to_string(),
            description: "IM".to_string(),
            thumbnail_url: "https://flare.test/t.png".to_string(),
            site_name: "Flare".to_string(),
        });

        let value = serde_json::to_value(&elem).expect("serialize elem");

        assert_eq!(value["contentType"], "link_card");
        assert_eq!(value["thumbnailUrl"], "https://flare.test/t.png");
        assert_eq!(value["siteName"], "Flare");
        assert!(value.get("content_type").is_none());
        assert!(value.get("thumbnail_url").is_none());
    }
}
