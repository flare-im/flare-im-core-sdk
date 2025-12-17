//! 会话视图模型
//!
//! 用于 API 层返回会话信息
//!
//! 基于 protobuf SessionSummary/SessionDetail 定义，提供完整的会话视图对象

use crate::domain::session::SessionSummary as DomainSessionSummary;
use serde::{Deserialize, Serialize};

/// 会话视图模型
///
/// 用于 API 层返回会话信息
/// 基于 `flare_proto::common::SessionSummary` 定义，包含所有会话字段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionVO {
    /// 会话 ID
    pub session_id: String,
    /// 会话类型（single/group/channel）
    pub session_type: String,
    /// 业务类型
    pub business_type: String,

    // ========== 展示信息 ==========
    /// 显示名称
    pub display_name: Option<String>,
    /// 头像 URL
    pub avatar_url: Option<String>,

    // ========== 消息信息 ==========
    /// 最后一条消息预览
    pub last_message: Option<MessagePreviewVO>,
    /// 未读数
    pub unread_count: u32,
    /// 最大序列号
    pub max_seq: u64,
    /// 最后已读序列号
    pub last_read_seq: u64,

    // ========== 用户个性化属性 ==========
    /// 是否免打扰
    pub is_muted: bool,
    /// 是否置顶
    pub is_pinned: bool,
    /// 详细免打扰配置
    pub is_muted_detail: bool,
    /// 免打扰结束时间（毫秒时间戳）
    pub mute_until: Option<i64>,

    // ========== 时间信息 ==========
    /// 更新时间（毫秒时间戳）
    pub updated_at: Option<i64>,
    /// 创建时间（毫秒时间戳）
    pub created_at: Option<i64>,

    // ========== 扩展信息 ==========
    /// 元数据（用于存储草稿等扩展信息）
    pub metadata: std::collections::HashMap<String, String>,
    /// 会话标签
    pub labels: Vec<String>,

    // ========== 会话详情（可选，用于详情页）==========
    /// 会话详情（可选，仅在需要时加载）
    pub detail: Option<SessionDetailVO>,
}

/// 消息预览视图模型
///
/// 基于 `flare_proto::common::MessagePreview` 定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePreviewVO {
    /// 消息 ID
    pub message_id: String,
    /// 发送者 ID
    pub sender_id: String,
    /// 消息类型
    pub message_type: i32,
    /// 纯文本预览
    pub text: String,
    /// 时间戳（毫秒）
    pub timestamp: i64,
}

/// 会话详情视图模型
///
/// 基于 `flare_proto::common::SessionDetail` 定义
/// 包含完整的会话信息，用于详情页展示
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetailVO {
    /// 会话属性
    pub attributes: std::collections::HashMap<String, String>,
    /// 会话参与者列表
    pub participants: Vec<SessionParticipantVO>,
    /// 会话可见性（private/tenant/public）
    pub visibility: i32,
    /// 会话生命周期状态（active/suspended/archived/deleted）
    pub lifecycle_state: i32,
    /// 会话策略
    pub policy: Option<SessionPolicyVO>,
    /// 会话公告
    pub announcement: Option<String>,
    /// 公告更新时间（毫秒时间戳）
    pub announcement_updated_at: Option<i64>,
    /// 公告更新者ID
    pub announcement_updated_by: Option<String>,
    /// 会话描述
    pub description: Option<String>,
    /// 会话扩展配置
    pub extended_config: std::collections::HashMap<String, String>,
    /// 设备在线状态（单聊时使用）
    pub presence: Option<DevicePresenceVO>,
}

/// 会话参与者视图模型
///
/// 基于 `flare_proto::common::SessionParticipant` 定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionParticipantVO {
    /// 用户ID
    pub user_id: String,
    /// 角色列表
    pub roles: Vec<String>,
    /// 是否静音
    pub muted: bool,
    /// 是否置顶
    pub pinned: bool,
    /// 参与者属性
    pub attributes: std::collections::HashMap<String, String>,
    /// 参与者加入时间（毫秒时间戳）
    pub joined_at: Option<i64>,
    /// 参与者昵称（群昵称）
    pub nickname: Option<String>,
}

/// 会话策略视图模型
///
/// 基于 `flare_proto::common::SessionPolicy` 定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPolicyVO {
    /// 冲突解决策略（exclusive/platform_exclusive/coexist/force_logout）
    pub conflict_resolution: i32,
    /// 最大设备数
    pub max_devices: Option<i32>,
    /// 是否允许匿名
    pub allow_anonymous: bool,
    /// 是否允许历史同步
    pub allow_history_sync: bool,
    /// 元数据
    pub metadata: std::collections::HashMap<String, String>,
    /// 是否允许消息搜索
    pub allow_message_search: bool,
    /// 是否允许文件传输
    pub allow_file_transfer: bool,
}

/// 设备在线状态视图模型
///
/// 基于 `flare_proto::common::DevicePresence` 定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevicePresenceVO {
    /// 设备ID
    pub device_id: String,
    /// 设备平台（ios/android/web/pc等）
    pub device_platform: String,
    /// 设备状态（online/offline/conflict）
    pub device_state: i32,
    /// 最后在线时间（毫秒时间戳）
    pub last_seen_at: Option<i64>,
    /// 设备名称
    pub device_name: Option<String>,
    /// 设备IP地址
    pub ip_address: Option<String>,
}

/// 会话列表视图模型
///
/// 用于分页返回会话列表
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionListVO {
    /// 会话列表
    pub sessions: Vec<SessionVO>,
    /// 是否有更多会话
    pub has_more: bool,
    /// 下一个游标（用于分页）
    pub next_cursor: Option<String>,
    /// 总数（可选，可能影响性能）
    pub total: Option<usize>,
}

/// 会话同步结果视图模型
///
/// 用于同步操作返回结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSyncResultVO {
    /// 同步的会话列表
    pub sessions: Vec<SessionVO>,
    /// 是否有更多会话
    pub has_more: bool,
    /// 下一个游标
    pub next_cursor: Option<String>,
    /// 同步的会话数量
    pub count: usize,
}

// ============================================================================
// 转换实现
// ============================================================================

impl From<DomainSessionSummary> for SessionVO {
    fn from(domain: DomainSessionSummary) -> Self {
        Self {
            session_id: domain.session_id,
            session_type: domain.session_type,
            business_type: domain.business_type,
            display_name: domain.display_name,
            avatar_url: domain.avatar_url,
            last_message: domain.last_message.map(MessagePreviewVO::from),
            unread_count: domain.unread_count,
            max_seq: domain.max_seq,
            last_read_seq: domain.last_read_seq,
            is_muted: domain.is_muted,
            is_pinned: domain.is_pinned,
            is_muted_detail: domain.is_muted_detail,
            mute_until: domain.mute_until,
            updated_at: domain.updated_at,
            created_at: domain.created_at,
            metadata: domain.metadata,
            labels: domain.labels,
            detail: None, // 详情需要单独加载
        }
    }
}

impl From<flare_proto::common::MessagePreview> for MessagePreviewVO {
    fn from(preview: flare_proto::common::MessagePreview) -> Self {
        Self {
            message_id: preview.message_id,
            sender_id: preview.sender_id,
            message_type: preview.r#type as i32,
            text: preview.text,
            timestamp: preview
                .time
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000))
                .unwrap_or(0),
        }
    }
}

impl SessionVO {
    /// 从领域模型创建视图模型
    pub fn from_domain(domain: &DomainSessionSummary) -> Self {
        Self::from(domain.clone())
    }

    /// 从 ProtoSessionSummary 创建视图模型
    pub fn from_proto(proto: flare_proto::common::SessionSummary) -> Self {
        Self {
            session_id: proto.session_id,
            session_type: proto.session_type,
            business_type: proto.business_type,
            display_name: if proto.display_name.is_empty() {
                None
            } else {
                Some(proto.display_name)
            },
            avatar_url: if proto.avatar_url.is_empty() {
                None
            } else {
                Some(proto.avatar_url)
            },
            last_message: proto.last_message.map(MessagePreviewVO::from),
            unread_count: proto.unread_count,
            max_seq: proto.max_seq,
            last_read_seq: proto.last_read_seq,
            is_muted: proto.is_muted,
            is_pinned: proto.is_pinned,
            is_muted_detail: proto.is_muted_detail,
            mute_until: proto
                .mute_until
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
            updated_at: proto
                .updated_at
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
            created_at: proto
                .created_at
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
            metadata: proto.metadata,
            labels: proto.labels,
            detail: None, // 详情需要单独加载
        }
    }

    /// 从 ProtoSessionDetail 创建完整视图模型（包含详情）
    pub fn from_detail_proto(detail: flare_proto::common::SessionDetail) -> Self {
        // 从详情中提取基础信息
        let summary = flare_proto::common::SessionSummary {
            session_id: detail.session_id.clone(),
            session_type: detail.session_type.clone(),
            business_type: detail.business_type.clone(),
            display_name: detail.display_name.clone(),
            avatar_url: detail.avatar_url.clone(),
            last_message: None, // 详情中没有最后消息
            unread_count: 0,    // 详情中没有未读数
            max_seq: 0,         // 详情中没有序列号
            last_read_seq: 0,   // 详情中没有已读序列号
            is_muted: false,    // 详情中没有免打扰信息
            is_pinned: false,   // 详情中没有置顶信息
            updated_at: detail.updated_at.clone(),
            created_at: detail.created_at.clone(),
            metadata: detail.extended_config.clone(),
            labels: vec![],
            is_muted_detail: false,
            mute_until: None,
        };

        let mut session = Self::from_proto(summary);

        // 设置详情信息
        session.detail = Some(SessionDetailVO::from_proto(detail));

        session
    }
}

impl SessionDetailVO {
    /// 从 ProtoSessionDetail 创建视图模型
    pub fn from_proto(proto: flare_proto::common::SessionDetail) -> Self {
        Self {
            attributes: proto.attributes,
            participants: proto
                .participants
                .into_iter()
                .map(SessionParticipantVO::from_proto)
                .collect(),
            visibility: proto.visibility as i32,
            lifecycle_state: proto.lifecycle_state as i32,
            policy: proto.policy.map(SessionPolicyVO::from_proto),
            announcement: if proto.announcement.is_empty() {
                None
            } else {
                Some(proto.announcement)
            },
            announcement_updated_at: proto
                .announcement_updated_at
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
            announcement_updated_by: if proto.announcement_updated_by.is_empty() {
                None
            } else {
                Some(proto.announcement_updated_by)
            },
            description: if proto.description.is_empty() {
                None
            } else {
                Some(proto.description)
            },
            extended_config: proto.extended_config,
            presence: proto.presence.map(DevicePresenceVO::from_proto),
        }
    }
}

impl SessionParticipantVO {
    fn from_proto(proto: flare_proto::common::SessionParticipant) -> Self {
        Self {
            user_id: proto.user_id,
            roles: proto.roles,
            muted: proto.muted,
            pinned: proto.pinned,
            attributes: proto.attributes,
            joined_at: proto
                .joined_at
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
            nickname: if proto.nickname.is_empty() {
                None
            } else {
                Some(proto.nickname)
            },
        }
    }
}

impl SessionPolicyVO {
    fn from_proto(proto: flare_proto::common::SessionPolicy) -> Self {
        Self {
            conflict_resolution: proto.conflict_resolution as i32,
            max_devices: if proto.max_devices == 0 {
                None
            } else {
                Some(proto.max_devices)
            },
            allow_anonymous: proto.allow_anonymous,
            allow_history_sync: proto.allow_history_sync,
            metadata: proto.metadata,
            allow_message_search: proto.allow_message_search,
            allow_file_transfer: proto.allow_file_transfer,
        }
    }
}

impl DevicePresenceVO {
    fn from_proto(proto: flare_proto::common::DevicePresence) -> Self {
        Self {
            device_id: proto.device_id,
            device_platform: proto.device_platform,
            device_state: proto.state as i32,
            last_seen_at: proto
                .last_seen_at
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
            device_name: if proto.device_name.is_empty() {
                None
            } else {
                Some(proto.device_name)
            },
            ip_address: if proto.ip_address.is_empty() {
                None
            } else {
                Some(proto.ip_address)
            },
        }
    }
}

impl SessionListVO {
    /// 创建空的会话列表
    pub fn empty() -> Self {
        Self {
            sessions: Vec::new(),
            has_more: false,
            next_cursor: None,
            total: None,
        }
    }

    /// 从领域模型列表创建
    pub fn from_domain(
        sessions: Vec<DomainSessionSummary>,
        has_more: bool,
        next_cursor: Option<String>,
    ) -> Self {
        Self {
            sessions: sessions.into_iter().map(SessionVO::from).collect(),
            has_more,
            next_cursor,
            total: None,
        }
    }

    /// 从 ProtoSessionSummary 列表创建
    pub fn from_proto(
        sessions: Vec<flare_proto::common::SessionSummary>,
        has_more: bool,
        next_cursor: Option<String>,
    ) -> Self {
        Self {
            sessions: sessions.into_iter().map(SessionVO::from_proto).collect(),
            has_more,
            next_cursor,
            total: None,
        }
    }
}

impl SessionSyncResultVO {
    /// 创建空的同步结果
    pub fn empty() -> Self {
        Self {
            sessions: Vec::new(),
            has_more: false,
            next_cursor: None,
            count: 0,
        }
    }

    /// 从领域模型列表创建
    pub fn from_domain(
        sessions: Vec<DomainSessionSummary>,
        has_more: bool,
        next_cursor: Option<String>,
    ) -> Self {
        Self {
            count: sessions.len(),
            sessions: sessions.into_iter().map(SessionVO::from).collect(),
            has_more,
            next_cursor,
        }
    }

    /// 从 ProtoSessionSummary 列表创建
    pub fn from_proto(
        sessions: Vec<flare_proto::common::SessionSummary>,
        has_more: bool,
        next_cursor: Option<String>,
    ) -> Self {
        Self {
            count: sessions.len(),
            sessions: sessions.into_iter().map(SessionVO::from_proto).collect(),
            has_more,
            next_cursor,
        }
    }
}
