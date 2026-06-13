//! 会话模型 — SDK 内部统一使用 Conversation；从 proto ConversationSummary 获取后即转换为 Conversation。
//! 序列化用 serde camelCase JSON；client 不再做命名桥接转换。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::message_elem::{
    MessagePreviewElem, message_preview_from_proto, message_preview_to_proto,
};

fn proto_ms_to_u64(ms: i64) -> u64 {
    if ms > 0 { ms as u64 } else { 0 }
}

fn u64_to_proto_ms(ms: u64) -> i64 {
    i64::try_from(ms).unwrap_or(i64::MAX)
}

// ---------- 会话类型（严格对齐 `flare.common.v1.ConversationType`，并与 flare_core CID 前缀一致）----------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

    /// 是否为单聊会话（与 [crate::domain::conversation::id::is_single_chat_conversation] 语义一致）
    pub fn is_single_chat_conversation(&self) -> bool {
        *self == ConversationType::Single
    }

    /// 是否为群聊会话（与 [crate::domain::conversation::id::is_group_chat_conversation] 语义一致）
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

/// SDK 本地会话参与者快照。单聊不依赖该结构；群聊/频道/客服等非单聊用它支撑群通话、成员面板和后续设置页。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct ConversationParticipant {
    pub user_id: String,
    pub roles: Vec<String>,
    pub muted: bool,
    pub pinned: bool,
    pub attributes: HashMap<String, String>,
    pub joined_at: u64,
    pub nickname: String,
}

impl From<flare_proto::common::ConversationParticipant> for ConversationParticipant {
    fn from(p: flare_proto::common::ConversationParticipant) -> Self {
        let nickname = p.attributes.get("nickname").cloned().unwrap_or_default();
        Self {
            user_id: p.user_id,
            roles: p.roles,
            muted: p.muted,
            pinned: p.pinned,
            attributes: p.attributes,
            joined_at: proto_ms_to_u64(p.joined_at),
            nickname,
        }
    }
}

impl From<ConversationParticipant> for flare_proto::common::ConversationParticipant {
    fn from(p: ConversationParticipant) -> Self {
        let mut attributes = p.attributes.clone();
        if !p.nickname.is_empty() {
            attributes
                .entry("nickname".to_string())
                .or_insert_with(|| p.nickname.clone());
        }
        Self {
            user_id: p.user_id,
            roles: p.roles,
            muted: p.muted,
            pinned: p.pinned,
            attributes,
            joined_at: u64_to_proto_ms(p.joined_at),
        }
    }
}

/// SDK 层会话类型：内部统一使用，从 proto ConversationSummary 获取后即转换为此类型。
/// 与 message.rs 的 IMMessage 一致：扁平字段、serde camelCase。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
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
    /// 对端（其他成员）最大已读序列号；用于发送方已读双勾在重连/重登后恢复。
    /// 由服务端同步摘要 `ext.peer_read_seq` 下发并持久化。
    pub peer_read_seq: u64,
    pub max_seq: u64,
    /// 当前用户的历史可见边界；seq <= visible_after_seq 的消息不可见，不参与冷启动回灌。
    pub visible_after_seq: u64,

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
    /// 服务端成员读模型版本。完整成员通过独立 participants 同步拉取。
    pub participant_version: u64,
    /// 摘要级成员预览，最多少量成员，不能作为完整成员列表使用。
    pub member_preview: Vec<ConversationParticipant>,

    // ===============================
    // 草稿 / @ / 徽标 / 群角色
    // ===============================
    pub draft: Option<String>,
    pub mention_count: u32,
    pub mention_me: bool,
    pub badge: Option<String>,
    pub role: Option<String>,
    /// 已按需同步到本地的完整成员快照；会话摘要同步不会填充该字段。
    pub participants: Vec<ConversationParticipant>,

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
            peer_read_seq: 0,
            max_seq: 0,
            visible_after_seq: 0,
            is_pinned: false,
            is_muted: false,
            is_archived: false,
            version: 0,
            updated_at: 0,
            created_at: 0,
            updated_at_ts: None,
            ext: HashMap::new(),
            participant_version: 0,
            member_preview: Vec::new(),
            draft: None,
            mention_count: 0,
            mention_me: false,
            badge: None,
            role: None,
            participants: Vec::new(),
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
        let mut attributes = self.ext.clone();
        attributes.insert("peer_read_seq".to_string(), self.peer_read_seq.to_string());
        if !self.business_type.trim().is_empty() {
            attributes.insert("business_type".to_string(), self.business_type.clone());
        }
        flare_proto::common::ConversationSummary {
            conversation_id: self.conversation_id.clone(),
            conversation_type: self.conversation_type.as_str().to_string(),
            channel_id: self.channel_id.clone(),
            display_name: self.display_name.clone(),
            avatar_url: self.avatar_url.clone(),
            unread_count: self.unread_count,
            max_conversation_seq: self.max_seq,
            visible_after_conversation_seq: self.visible_after_seq,
            last_read_seq: self.last_read_seq,
            is_muted: self.is_muted,
            is_pinned: self.is_pinned,
            is_archived: self.is_archived,
            updated_at: u64_to_proto_ms(self.updated_at),
            created_at: u64_to_proto_ms(self.created_at),
            member_count: self.members_count as i32,
            participant_version: self.participant_version,
            member_preview: self
                .member_preview
                .clone()
                .into_iter()
                .map(Into::into)
                .collect(),
            draft: self.draft.clone().unwrap_or_default(),
            last_message: self.last_message.as_ref().map(message_preview_to_proto),
            attributes,
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
        let updated_at = proto_ms_to_u64(s.updated_at);
        let created_at = proto_ms_to_u64(s.created_at);
        let last_message = s.last_message.as_ref().map(message_preview_from_proto);
        let business_type = s
            .attributes
            .get("business_type")
            .cloned()
            .unwrap_or_default();
        let peer_read_seq = s
            .attributes
            .get("peer_read_seq")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or_default();
        let peer_read_seq = if peer_read_seq <= s.max_conversation_seq {
            peer_read_seq
        } else {
            0
        };
        let member_preview: Vec<ConversationParticipant> =
            s.member_preview.into_iter().map(Into::into).collect();
        let visible_after_seq = s.visible_after_conversation_seq;
        // 以服务端聚合后的 unread_count 为准；若服务端下发了用户历史可见边界，
        // SDK 仍在模型入口做一次硬约束，避免旧摘要/旧消息回灌成本地未读。
        let unread_count = if visible_after_seq > 0 && s.max_conversation_seq <= visible_after_seq {
            0
        } else {
            s.unread_count
        };
        let last_read_seq = s.last_read_seq.max(visible_after_seq);
        Self {
            conversation_id: s.conversation_id,
            conversation_type: ConversationType::from(s.conversation_type.as_str()),
            business_type,
            channel_id: s.channel_id,
            display_name: s.display_name,
            avatar_url: s.avatar_url,
            unread_count,
            max_seq: s.max_conversation_seq,
            visible_after_seq,
            last_read_seq,
            peer_read_seq,
            last_message_id: last_message.as_ref().and_then(|m| {
                if m.message_id.trim().is_empty() {
                    None
                } else {
                    Some(m.message_id.clone())
                }
            }),
            last_sender_id: last_message.as_ref().and_then(|m| {
                if m.sender_id.trim().is_empty() {
                    None
                } else {
                    Some(m.sender_id.clone())
                }
            }),
            last_message_at: last_message
                .as_ref()
                .and_then(|m| if m.time > 0 { Some(m.time) } else { None }),
            last_message_preview: last_message.as_ref().and_then(|m| {
                if m.text.trim().is_empty() {
                    None
                } else {
                    Some(m.text.clone())
                }
            }),
            last_message,
            updated_at,
            created_at,
            last_sender_nickname: String::new(),
            last_sender_avatar_url: String::new(),
            is_pinned: s.is_pinned,
            is_muted: s.is_muted,
            is_archived: s.is_archived,
            members_count: (s.member_count.max(0) as u32).max(member_preview.len() as u32),
            participant_version: s.participant_version,
            member_preview,
            participants: Vec::new(),
            draft: if s.draft.trim().is_empty() {
                None
            } else {
                Some(s.draft.clone())
            },
            mention_count: 0,
            mention_me: false,
            badge: None,
            role: None,
            ext: {
                let mut ext = s.attributes.clone();
                ext.insert("peer_read_seq".to_string(), peer_read_seq.to_string());
                if s.user_settings_version > 0 {
                    ext.insert(
                        crate::model::EXT_USER_SETTINGS_VERSION.to_string(),
                        s.user_settings_version.to_string(),
                    );
                }
                ext.insert(
                    crate::model::EXT_SETTINGS_DIRTY.to_string(),
                    "0".to_string(),
                );
                ext
            },
            ..Default::default()
        }
    }
}

/// 仅边界使用：持久化/网络层解码得到 proto，应即转为 [Conversation]
pub use flare_proto::common::ConversationSummary;

#[cfg(test)]
mod tests {
    use super::Conversation;
    use flare_proto::common::{ConversationSummary, MessagePreview};

    #[test]
    fn summary_conversion_populates_persisted_last_message_fields() {
        let conversation = Conversation::from(ConversationSummary {
            conversation_id: "conv-1".to_string(),
            conversation_type: "single".to_string(),
            max_conversation_seq: 7,
            last_message: Some(MessagePreview {
                message_id: "msg-7".to_string(),
                sender_id: "u2".to_string(),
                text: "latest".to_string(),
                created_at: 12_345,
                ..Default::default()
            }),
            ..Default::default()
        });

        assert_eq!(conversation.last_message_id.as_deref(), Some("msg-7"));
        assert_eq!(conversation.last_sender_id.as_deref(), Some("u2"));
        assert_eq!(conversation.last_message_preview.as_deref(), Some("latest"));
        assert_eq!(conversation.last_message_at, Some(12_345));
    }

    #[test]
    fn summary_conversion_drops_impossible_peer_read_seq() {
        let mut summary = ConversationSummary {
            conversation_id: "conv-1".to_string(),
            conversation_type: "single".to_string(),
            max_conversation_seq: 7,
            ..Default::default()
        };
        summary
            .attributes
            .insert("peer_read_seq".to_string(), "999999".to_string());

        let conversation = Conversation::from(summary);

        assert_eq!(conversation.peer_read_seq, 0);
        assert_eq!(
            conversation.ext.get("peer_read_seq").map(String::as_str),
            Some("0")
        );
    }
}
