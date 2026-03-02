//! Conversation 聚合根
//!
//! 职责：管理会话和未读数
//! 对齐 flare-proto 的 Conversation 定义，达到生产级别

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Conversation 聚合根
///
/// 对齐 flare-proto/common/conversation_models.proto 的定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    // ========== 基础信息 ==========
    /// 会话ID
    pub conversation_id: String,
    
    /// 会话类型
    pub conversation_type: String,
    
    /// 业务类型
    pub business_type: Option<String>,
    
    // ========== 展示信息 ==========
    /// 显示名称
    pub display_name: String,
    
    /// 头像URL
    pub avatar_url: Option<String>,
    
    // ========== 消息相关 ==========
    /// 未读数
    pub unread_count: u32,
    
    /// 最大序列号
    pub max_seq: u64,
    
    /// 最后已读序列号
    pub last_read_seq: u64,
    
    /// 最后一条消息预览
    pub last_message: Option<MessagePreview>,
    
    // ========== 用户个性化属性 ==========
    /// 是否静音
    pub is_muted: bool,
    
    /// 是否置顶
    pub is_pinned: bool,
    
    /// 会话免打扰配置
    pub is_muted_detail: bool,
    
    /// 免打扰到期时间
    pub mute_until: Option<DateTime<Utc>>,
    
    // ========== 会话状态 ==========
    /// 可见性
    pub visibility: ConversationVisibility,
    
    /// 生命周期状态
    pub lifecycle_state: ConversationLifecycleState,
    
    // ========== 会话详情（Level 3）==========
    /// 属性
    pub attributes: HashMap<String, String>,
    
    /// 参与者列表
    pub participants: Vec<ConversationParticipant>,
    
    /// 会话策略
    pub policy: Option<ConversationPolicy>,
    
    /// 设备在线状态
    pub presence: Option<DevicePresence>,
    
    /// 会话公告
    pub announcement: Option<String>,
    
    /// 公告更新时间
    pub announcement_updated_at: Option<DateTime<Utc>>,
    
    /// 公告更新者
    pub announcement_updated_by: Option<String>,
    
    /// 会话描述
    pub description: Option<String>,
    
    /// 会话扩展配置
    pub extended_config: HashMap<String, String>,
    
    // ========== 扩展字段 ==========
    /// 扩展字段（允许 UI 自定义）
    pub ext: HashMap<String, String>,
    
    /// 会话标签
    pub labels: Vec<String>,
    
    /// 会话草稿（用户输入但未发送的内容）
    pub draft: Option<String>,
    
    /// 输入状态（正在输入、停止输入等）
    pub input_state: Option<InputState>,
    
    // ========== 时间信息 ==========
    /// 创建时间
    pub created_at: DateTime<Utc>,
    
    /// 更新时间
    pub updated_at: DateTime<Utc>,
    
    // ========== 内部状态 ==========
    /// 版本（用于乐观锁）
    pub version: u64,
}

/// 消息预览
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePreview {
    pub message_id: String,
    pub sender_id: String,
    pub message_type: String,
    pub text: String,
    pub time: DateTime<Utc>,
}

/// 会话可见性
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConversationVisibility {
    /// 私有
    Private,
    
    /// 租户可见（保留用于协议兼容）
    Tenant,
    
    /// 公开
    Public,
}

/// 会话生命周期状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConversationLifecycleState {
    /// 活跃
    Active,
    
    /// 暂停
    Suspended,
    
    /// 已归档
    Archived,
    
    /// 已删除
    Deleted,
}

/// 会话参与者
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationParticipant {
    pub user_id: String,
    pub roles: Vec<String>,
    pub muted: bool,
    pub pinned: bool,
    pub attributes: HashMap<String, String>,
    pub joined_at: DateTime<Utc>,
    pub nickname: Option<String>,
}

/// 会话策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationPolicy {
    pub conflict_resolution: ConflictResolution,
    pub max_devices: Option<i32>,
    pub allow_anonymous: bool,
    pub allow_history_sync: bool,
    pub metadata: HashMap<String, String>,
    pub allow_message_search: bool,
    pub allow_file_transfer: bool,
}

/// 设备冲突解决策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictResolution {
    /// 独占
    Exclusive,
    
    /// 平台独占
    PlatformExclusive,
    
    /// 共存
    Coexist,
    
    /// 强制登出
    ForceLogout,
}

/// 设备在线状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevicePresence {
    pub device_id: String,
    pub device_platform: String,
    pub state: DeviceState,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub device_name: Option<String>,
    pub ip_address: Option<String>,
}

/// 输入状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputState {
    /// 用户ID
    pub user_id: String,
    
    /// 输入状态类型
    pub state_type: InputStateType,
    
    /// 状态开始时间
    pub started_at: DateTime<Utc>,
    
    /// 状态持续时间（毫秒）
    pub duration_ms: Option<u64>,
}

/// 输入状态类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputStateType {
    /// 正在输入
    Typing,
    
    /// 停止输入
    Stopped,
    
    /// 正在录音
    Recording,
    
    /// 正在上传
    Uploading,
}

/// 设备状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceState {
    /// 在线
    Online,
    
    /// 离线
    Offline,
    
    /// 冲突
    Conflict,
}

impl Conversation {
    pub fn new(conversation_id: String, conversation_type: String) -> Self {
        let now = Utc::now();
        Self {
            conversation_id,
            conversation_type,
            business_type: None,
            display_name: String::new(),
            avatar_url: None,
            unread_count: 0,
            max_seq: 0,
            last_read_seq: 0,
            last_message: None,
            is_muted: false,
            is_pinned: false,
            is_muted_detail: false,
            mute_until: None,
            visibility: ConversationVisibility::Private,
            lifecycle_state: ConversationLifecycleState::Active,
            attributes: HashMap::new(),
            participants: Vec::new(),
            policy: None,
            presence: None,
            announcement: None,
            announcement_updated_at: None,
            announcement_updated_by: None,
            description: None,
            extended_config: HashMap::new(),
            ext: HashMap::new(),
            labels: Vec::new(),
            draft: None,
            input_state: None,
            created_at: now,
            updated_at: now,
            version: 0,
        }
    }
    
    /// 更新未读数
    pub fn update_unread(&mut self, count: u32) {
        self.unread_count = count;
        self.version += 1;
        self.updated_at = Utc::now();
    }
    
    /// 增加未读数
    pub fn increment_unread(&mut self) {
        self.unread_count += 1;
        self.version += 1;
        self.updated_at = Utc::now();
    }
    
    /// 清除未读数
    pub fn clear_unread(&mut self) {
        self.unread_count = 0;
        self.last_read_seq = self.max_seq;
        self.version += 1;
        self.updated_at = Utc::now();
    }
    
    /// 更新最大序列号
    pub fn update_max_seq(&mut self, seq: u64) {
        self.max_seq = seq;
        self.version += 1;
        self.updated_at = Utc::now();
    }
    
    /// 更新最后已读序列号
    pub fn update_last_read_seq(&mut self, seq: u64) {
        self.last_read_seq = seq;
        // 重新计算未读数
        if self.max_seq > seq {
            self.unread_count = (self.max_seq - seq) as u32;
        } else {
            self.unread_count = 0;
        }
        self.version += 1;
        self.updated_at = Utc::now();
    }
    
    /// 更新最后一条消息
    pub fn update_last_message(&mut self, preview: MessagePreview, seq: u64) {
        self.last_message = Some(preview);
        self.max_seq = seq;
        // 如果新消息序列号大于已读序列号，增加未读数
        if seq > self.last_read_seq {
            self.unread_count = (seq - self.last_read_seq) as u32;
        }
        self.version += 1;
        self.updated_at = Utc::now();
    }
    
    /// 设置静音
    pub fn set_muted(&mut self, muted: bool) {
        self.is_muted = muted;
        self.version += 1;
        self.updated_at = Utc::now();
    }
    
    /// 设置置顶
    pub fn set_pinned(&mut self, pinned: bool) {
        self.is_pinned = pinned;
        self.version += 1;
        self.updated_at = Utc::now();
    }
    
    /// 添加参与者
    pub fn add_participant(&mut self, participant: ConversationParticipant) {
        if !self.participants.iter().any(|p| p.user_id == participant.user_id) {
            self.participants.push(participant);
            self.version += 1;
            self.updated_at = Utc::now();
        }
    }
    
    /// 移除参与者
    pub fn remove_participant(&mut self, user_id: &str) {
        self.participants.retain(|p| p.user_id != user_id);
        self.version += 1;
        self.updated_at = Utc::now();
    }
    
    /// 更新参与者角色
    pub fn update_participant_roles(&mut self, user_id: &str, roles: Vec<String>) {
        if let Some(participant) = self.participants.iter_mut().find(|p| p.user_id == user_id) {
            participant.roles = roles;
            self.version += 1;
            self.updated_at = Utc::now();
        }
    }
    
    /// 归档会话
    pub fn archive(&mut self) {
        self.lifecycle_state = ConversationLifecycleState::Archived;
        self.version += 1;
        self.updated_at = Utc::now();
    }
    
    /// 取消归档
    pub fn unarchive(&mut self) {
        self.lifecycle_state = ConversationLifecycleState::Active;
        self.version += 1;
        self.updated_at = Utc::now();
    }
    
    /// 删除会话
    pub fn delete(&mut self) {
        self.lifecycle_state = ConversationLifecycleState::Deleted;
        self.version += 1;
        self.updated_at = Utc::now();
    }
    
    /// 更新公告
    pub fn update_announcement(&mut self, announcement: String, updated_by: String) {
        self.announcement = Some(announcement);
        self.announcement_updated_at = Some(Utc::now());
        self.announcement_updated_by = Some(updated_by);
        self.version += 1;
        self.updated_at = Utc::now();
    }
}
