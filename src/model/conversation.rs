//! 会话模型 — SDK 内部统一使用 Conversation；从 proto ConversationSummary 获取后即转换为 Conversation。
//! 序列化用 serde 宏，camelCase，与前端约定一致。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::message_elem::{MessagePreviewElem, message_preview_from_proto};
use crate::util::date::{ms_to_prost_timestamp, prost_timestamp_to_ms};

// ---------- 会话类型（严格对齐 `flare.common.v1.ConversationType`，并与 flare_core CID 前缀一致）----------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConversationType {
    #[default]
    Unspecified,
    Single,
    Group,
    Ai,
    System,
    Customer,
    Temp,
}

impl ConversationType {
    /// 返回与 proto/API 一致的类型字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            ConversationType::Unspecified => "unspecified",
            ConversationType::Single => "single",
            ConversationType::Group => "group",
            ConversationType::Ai => "ai",
            ConversationType::Customer => "customer",
            ConversationType::System => "system",
            ConversationType::Temp => "temp",
        }
    }

    /// CID 类型前缀（与 flare_core 一致：1=单聊 2=群聊 3=AI 4=系统 5=客服 6=临时）
    pub fn prefix(&self) -> Option<&'static str> {
        match self {
            ConversationType::Single => Some("1"),
            ConversationType::Group => Some("2"),
            ConversationType::Ai => Some("3"),
            ConversationType::System => Some("4"),
            ConversationType::Customer => Some("5"),
            ConversationType::Temp => Some("6"),
            ConversationType::Unspecified => None,
        }
    }

    /// 从 CID 类型前缀解析（如 "1" -> Single）
    pub fn from_prefix(prefix: &str) -> Option<Self> {
        match prefix {
            "1" => Some(ConversationType::Single),
            "2" => Some(ConversationType::Group),
            "3" => Some(ConversationType::Ai),
            "4" => Some(ConversationType::System),
            "5" => Some(ConversationType::Customer),
            "6" => Some(ConversationType::Temp),
            _ => None,
        }
    }

    /// 是否为单聊会话（与 [crate::conversation::is_single_chat_conversation] 语义一致）
    pub fn is_single_chat_conversation(&self) -> bool {
        *self == ConversationType::Single
    }

    /// 是否为群聊会话（与 [crate::conversation::is_group_chat_conversation] 语义一致）
    pub fn is_group_chat_conversation(&self) -> bool {
        *self == ConversationType::Group
    }

    /// 与 [`flare_proto::common::ConversationType`] 数值一致（`Message.conversation_type`、Orchestrator 推送等）
    pub fn to_proto_int(self) -> i32 {
        use flare_proto::common::ConversationType as ProtoT;
        match self {
            ConversationType::Unspecified => ProtoT::Unspecified as i32,
            ConversationType::Single => ProtoT::Single as i32,
            ConversationType::Group => ProtoT::Group as i32,
            ConversationType::Ai => ProtoT::Ai as i32,
            ConversationType::Customer => ProtoT::Customer as i32,
            ConversationType::System => ProtoT::System as i32,
            ConversationType::Temp => ProtoT::Temp as i32,
        }
    }

    /// 从消息 proto 的 `conversation_type` 整型解析；未知值映射为 [`Unspecified`]
    pub fn from_proto_int(v: i32) -> Self {
        use flare_proto::common::ConversationType as ProtoT;
        match ProtoT::try_from(v) {
            Ok(ProtoT::Unspecified) => ConversationType::Unspecified,
            Ok(ProtoT::Single) => ConversationType::Single,
            Ok(ProtoT::Group) => ConversationType::Group,
            Ok(ProtoT::Ai) => ConversationType::Ai,
            Ok(ProtoT::Customer) => ConversationType::Customer,
            Ok(ProtoT::System) => ConversationType::System,
            Ok(ProtoT::Temp) => ConversationType::Temp,
            Err(_) => ConversationType::Unspecified,
        }
    }
}

impl From<&str> for ConversationType {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "1" | "single" => ConversationType::Single,
            "2" | "group" => ConversationType::Group,
            "3" | "ai" => ConversationType::Ai,
            "4" | "system" => ConversationType::System,
            "5" | "customer" => ConversationType::Customer,
            "6" | "temp" => ConversationType::Temp,
            _ => ConversationType::Unspecified,
        }
    }
}

// ---------- 本地状态（不序列化到 JSON/DB，仅内存）----------

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationLocalState {
    /// 草稿光标等 UI 状态可放此处
    #[serde(default)]
    pub draft_cursor: Option<u32>,
}

/// SDK 层会话类型：内部统一使用，从 proto ConversationSummary 获取后即转换为此类型。
/// 与 message.rs 的 IMMessage 一致：扁平字段、serde 宏序列化、camelCase。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Conversation {
    // ===============================
    // Identity
    // ===============================
    pub conversation_id: String,
    pub conversation_type: ConversationType,
    pub business_type: String,
    /// 会话路由 ID：单聊为对方 user_id；群/频道为业务 channel（与 proto `channel_id` 一致）
    pub channel_id: String,
    pub members_count: u32,

    // ===============================
    // Display
    // ===============================
    /// 展示名（列表主标题）
    pub display_name: String,
    pub avatar_url: String,
    pub remark: Option<String>,
    pub description: Option<String>,

    // ===============================
    // Last Message
    // ===============================
    pub last_message_id: Option<String>,
    pub last_sender_id: Option<String>,
    pub last_message_at: Option<u64>,
    pub last_message_preview: Option<String>,
    pub last_message: Option<MessagePreviewElem>,
    /// 最后一条消息发送者展示名（列表用）
    pub last_sender_nickname: String,
    /// 最后一条消息发送者头像 URL
    pub last_sender_avatar_url: String,

    // ===============================
    // Unread
    // ===============================
    pub unread_count: u32,
    /// 已读序列号（与 read_seq 同义）
    pub last_read_seq: u64,
    pub max_seq: u64,

    // ===============================
    // User Settings
    // ===============================
    pub is_pinned: bool,
    pub is_muted: bool,
    pub is_archived: bool,

    // ===============================
    // Sync / Time
    // ===============================
    pub version: u64,
    pub updated_at: u64,
    pub created_at: u64,
    /// 更新时间戳（毫秒，用于排序/筛选）
    pub updated_at_ts: Option<u64>,

    // ===============================
    // 扩展
    // ===============================
    /// 扩展键值（与 proto ext 对应）
    pub ext: HashMap<String, String>,

    // ===============================
    // 草稿 / @ / 徽标 / 群角色
    // ===============================
    pub draft: Option<String>,
    pub mention_count: u32,
    pub mention_me: bool,
    pub badge: Option<String>,
    pub role: Option<String>,

    // ===============================
    // Local State（SDK 内部，不序列化到 DB）
    // ===============================
    #[serde(skip)]
    pub local_state: ConversationLocalState,
}

impl Default for Conversation {
    fn default() -> Self {
        Self {
            conversation_id: String::new(),
            conversation_type: ConversationType::Unspecified,
            business_type: String::new(),
            channel_id: String::new(),
            members_count: 0,
            display_name: String::new(),
            avatar_url: String::new(),
            remark: None,
            description: None,
            last_message_id: None,
            last_sender_id: None,
            last_message_at: None,
            last_message_preview: None,
            last_message: None,
            last_sender_nickname: String::new(),
            last_sender_avatar_url: String::new(),
            unread_count: 0,
            last_read_seq: 0,
            max_seq: 0,
            is_pinned: false,
            is_muted: false,
            is_archived: false,
            version: 0,
            updated_at: 0,
            created_at: 0,
            updated_at_ts: None,
            ext: HashMap::new(),
            draft: None,
            mention_count: 0,
            mention_me: false,
            badge: None,
            role: None,
            local_state: ConversationLocalState::default(),
        }
    }
}

impl Conversation {
    /// 从 proto ConversationSummary 构造（获取到 proto 后应即转换）
    pub fn new(summary: flare_proto::common::ConversationSummary) -> Self {
        Self::from(summary)
    }

    /// 仅凭 conversation_id 构造（如同步事件仅带 id 时）
    pub fn from_conversation_id(conversation_id: String) -> Self {
        Self {
            conversation_id,
            ..Default::default()
        }
    }

    /// 已读序列号（与 last_read_seq 同义，API 对齐）
    pub fn read_seq(&self) -> u64 {
        self.last_read_seq
    }

    /// 转为 proto ConversationSummary（持久化/回写服务端用）
    pub fn to_proto_summary(&self) -> flare_proto::common::ConversationSummary {
        let ext = self.ext.clone();
        flare_proto::common::ConversationSummary {
            conversation_id: self.conversation_id.clone(),
            conversation_type: self.conversation_type.as_str().to_string(),
            business_type: self.business_type.clone(),
            channel_id: self.channel_id.clone(),
            display_name: self.display_name.clone(),
            avatar_url: self.avatar_url.clone(),
            unread_count: self.unread_count,
            max_seq: self.max_seq,
            last_read_seq: self.last_read_seq,
            is_muted: self.is_muted,
            is_pinned: self.is_pinned,
            updated_at: ms_to_prost_timestamp(self.updated_at),
            created_at: ms_to_prost_timestamp(self.created_at),
            member_count: self.members_count as i32,
            ext,
            ..Default::default()
        }
    }

    /// 填充最后一条发送者资料
    pub fn with_last_sender(
        mut self,
        display_name: impl Into<String>,
        avatar_url: impl Into<String>,
    ) -> Self {
        self.last_sender_nickname = display_name.into();
        self.last_sender_avatar_url = avatar_url.into();
        self
    }

    pub fn conversation_id(&self) -> &str {
        &self.conversation_id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn avatar_url(&self) -> &str {
        &self.avatar_url
    }

    pub fn last_message(&self) -> Option<&MessagePreviewElem> {
        self.last_message.as_ref()
    }

    pub fn unread_count(&self) -> u32 {
        self.unread_count
    }

    pub fn max_seq(&self) -> u64 {
        self.max_seq
    }

    pub fn last_read_seq(&self) -> u64 {
        self.last_read_seq
    }

    pub fn last_sender_display_name(&self) -> &str {
        &self.last_sender_nickname
    }

    pub fn last_sender_avatar_url(&self) -> &str {
        &self.last_sender_avatar_url
    }

    pub fn draft(&self) -> Option<&str> {
        self.draft.as_deref()
    }

    pub fn mention_count(&self) -> u32 {
        self.mention_count
    }

    pub fn mention_me(&self) -> bool {
        self.mention_me
    }

    pub fn badge(&self) -> Option<&str> {
        self.badge.as_deref()
    }

    pub fn role(&self) -> Option<&str> {
        self.role.as_deref()
    }

    pub fn channel_id(&self) -> &str {
        &self.channel_id
    }
}

impl From<flare_proto::common::ConversationSummary> for Conversation {
    fn from(s: flare_proto::common::ConversationSummary) -> Self {
        let updated_at = prost_timestamp_to_ms(s.updated_at.as_ref());
        let created_at = prost_timestamp_to_ms(s.created_at.as_ref());
        // 以服务端聚合后的 unread_count 为准：
        // 该值已按消息可见性/消息状态（删除、撤回等）处理，能正确覆盖历史未读统计。
        let unread_count = s.unread_count;
        Self {
            conversation_id: s.conversation_id,
            conversation_type: ConversationType::from(s.conversation_type.as_str()),
            business_type: s.business_type,
            channel_id: s.channel_id,
            display_name: s.display_name,
            avatar_url: s.avatar_url,
            unread_count,
            max_seq: s.max_seq,
            last_read_seq: s.last_read_seq,
            last_message: s.last_message.as_ref().map(message_preview_from_proto),
            updated_at,
            created_at,
            last_sender_nickname: String::new(),
            last_sender_avatar_url: String::new(),
            ext: s.ext.clone(),
            is_pinned: s.is_pinned,
            is_muted: s.is_muted,
            members_count: s.member_count.max(0) as u32,
            draft: None,
            mention_count: 0,
            mention_me: false,
            badge: None,
            role: None,
            ..Default::default()
        }
    }
}

/// 仅边界使用：持久化/网络层解码得到 proto，应即转为 [Conversation]
pub use flare_proto::common::ConversationSummary;
