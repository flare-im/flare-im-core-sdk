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
use crate::model::message_elem::{Elem, decoded_content_to_elem, elem_to_message_content};
use crate::util::date::{ms_to_prost_timestamp, prost_timestamp_to_ms};
use prost::Message as ProstMessage;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageLocalState {
    /// 是否发送中
    pub sending: bool,
    /// 是否失败
    pub failed: bool,
    /// 本地消息
    pub is_local: bool,
    /// 本地排序时间
    pub sort_ts: u64,
}

/// SDK 层消息类型：与 message.proto 的 Message 属性一致，content 为解码后的 Elem；
/// 另保留 content_bytes 与 proto 一致用于持久化/网络，并增加发送者展示字段。
/// 序列化：camelCase；`content_bytes` 对 JSON `skip`（避免把二进制塞进 WebView）。**上行发送**时若仅有 `content`（Elem），
/// [`to_proto`](IMMessage::to_proto) 会从 `content` 重编码为 `MessageContent` 字节，与协议一致。
#[derive(Debug, Clone, Serialize, Deserialize)]
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
        let content = decoded_opt
            .as_ref()
            .and_then(|decoded| decoded_content_to_elem(decoded));
        let mut extra = message.extra.clone();
        if content.is_none() {
            if let Some(ref decoded) = decoded_opt {
                let p = decoded.text_preview();
                if !p.is_empty() && p != "[未知]" {
                    extra.entry("content_text".into()).or_insert(p);
                }
            }
        }
        let timestamp = prost_timestamp_to_ms(message.timestamp.as_ref());
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
            reply_to: None,
            quote_preview: None,
            status: message.status,
            is_read: false,
            is_recalled: false,
            is_edited: false,
            mention_users: Vec::new(),
            mention_all: false,
            offline_push_info: message.offline_push_info,
            extra,
            extensions: message.extensions,
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

    /// 供存储使用：仅文本消息返回 Some(text)，其余为 None。存库时只存 content_bytes + 可选 text，不存 content。
    pub fn text_for_storage(&self) -> Option<String> {
        if self.message_type != flare_proto::common::MessageType::Text as i32 {
            return None;
        }
        self.content.as_ref().and_then(|c| {
            if let Elem::Text(t) = c {
                Some(t.text.clone())
            } else {
                None
            }
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
    AudioInfo, ConversationType, DeleteScope, DeleteType, ImageInfo, MarkType, Message,
    MessageSource, MessageStatus, ReactionAction, SendAck, VideoInfo,
};
