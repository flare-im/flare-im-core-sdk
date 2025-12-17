//! 会话领域模型
//!
//! 包含 Session 聚合根、值对象等

use anyhow::{Context, Result};
use chrono::Utc;
use flare_proto::common::SessionSummary as ProtoSessionSummary;
use std::collections::HashMap;

/// 会话摘要（用于会话列表展示）
#[derive(Debug, Clone)]
pub struct SessionSummary {
    /// 会话 ID
    pub session_id: String,
    /// 会话类型（single/group/channel）
    pub session_type: String,
    /// 业务类型
    pub business_type: String,
    /// 显示名称
    pub display_name: Option<String>,
    /// 头像 URL
    pub avatar_url: Option<String>,
    /// 最后一条消息预览
    pub last_message: Option<flare_proto::common::MessagePreview>,
    /// 未读数
    pub unread_count: u32,
    /// 最大序列号
    pub max_seq: u64,
    /// 最后已读序列号
    pub last_read_seq: u64,
    /// 是否免打扰
    pub is_muted: bool,
    /// 是否置顶
    pub is_pinned: bool,
    /// 更新时间
    pub updated_at: Option<i64>,
    /// 元数据
    pub metadata: std::collections::HashMap<String, String>,
    /// 会话标签
    pub labels: Vec<String>,
    /// 详细免打扰配置
    pub is_muted_detail: bool,
    /// 免打扰结束时间
    pub mute_until: Option<i64>,
    /// 创建时间
    pub created_at: Option<i64>,
}

impl From<ProtoSessionSummary> for SessionSummary {
    fn from(proto: ProtoSessionSummary) -> Self {
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
            last_message: proto.last_message,
            unread_count: proto.unread_count,
            max_seq: proto.max_seq,
            last_read_seq: proto.last_read_seq,
            is_muted: proto.is_muted,
            is_pinned: proto.is_pinned,
            updated_at: proto
                .updated_at
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
            metadata: proto.metadata,
            labels: proto.labels,
            is_muted_detail: proto.is_muted_detail,
            mute_until: proto
                .mute_until
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
            created_at: proto
                .created_at
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
        }
    }
}

impl SessionSummary {
    /// 获取最后一条消息ID
    pub fn last_message_id(&self) -> Option<String> {
        self.last_message.as_ref().map(|msg| msg.message_id.clone())
    }

    /// 获取最后一条消息时间
    pub fn last_message_time(&self) -> Option<i64> {
        self.last_message.as_ref().and_then(|msg| {
            msg.time
                .as_ref()
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000))
        })
    }

    /// 获取最后一条消息发送者ID
    pub fn last_sender_id(&self) -> Option<String> {
        self.last_message.as_ref().map(|msg| msg.sender_id.clone())
    }

    /// 获取最后一条消息类型
    pub fn last_message_type(&self) -> Option<i32> {
        self.last_message.as_ref().map(|msg| msg.r#type)
    }

    /// 获取最后一条消息内容类型
    pub fn last_content_type(&self) -> Option<i32> {
        // MessagePreview 没有 content_type 字段，返回 None
        None
    }

    /// 获取服务器游标时间戳
    pub fn server_cursor_ts(&self) -> Option<i64> {
        self.updated_at
    }

    /// 转换为 ProtoSessionSummary
    pub fn to_proto(&self) -> ProtoSessionSummary {
        ProtoSessionSummary {
            session_id: self.session_id.clone(),
            session_type: self.session_type.clone(),
            business_type: self.business_type.clone(),
            display_name: self.display_name.clone().unwrap_or_default(),
            avatar_url: self.avatar_url.clone().unwrap_or_default(),
            last_message: self.last_message.clone(),
            unread_count: self.unread_count,
            max_seq: self.max_seq,
            last_read_seq: self.last_read_seq,
            is_muted: self.is_muted,
            is_pinned: self.is_pinned,
            updated_at: self.updated_at.map(|ts_ms| prost_types::Timestamp {
                seconds: ts_ms / 1000,
                nanos: ((ts_ms % 1000) * 1_000_000) as i32,
            }),
            created_at: self.created_at.map(|ts_ms| prost_types::Timestamp {
                seconds: ts_ms / 1000,
                nanos: ((ts_ms % 1000) * 1_000_000) as i32,
            }),
            labels: self.labels.clone(),
            metadata: self.metadata.clone(),
            is_muted_detail: self.is_muted_detail,
            mute_until: self.mute_until.map(|ts_ms| prost_types::Timestamp {
                seconds: ts_ms / 1000,
                nanos: ((ts_ms % 1000) * 1_000_000) as i32,
            }),
        }
    }
}

use super::event::{
    SessionCreatedEvent, SessionDeletedEvent, SessionDraftSetEvent, SessionHiddenEvent,
    SessionMarkedReadEvent, SessionShownEvent, SessionTypingSentEvent, SessionUpdatedEvent,
};
use crate::domain::message::model::{SessionId, UserId};

/// 会话错误
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("Session validation failed: {0}")]
    ValidationFailed(String),

    #[error("Session not found")]
    NotFound,

    #[error("Invalid session type: {0}")]
    InvalidSessionType(String),
}

/// Session 聚合根
///
/// 封装会话的领域逻辑和行为
pub struct Session {
    id: SessionId,
    session_type: String,
    business_type: String,
    display_name: Option<String>,
    avatar_url: Option<String>,
    participants: Vec<String>,
    metadata: HashMap<String, String>,
    // 其他字段...
    proto_summary: ProtoSessionSummary,
}

impl Session {
    /// 创建新会话
    pub fn new(
        id: SessionId,
        session_type: String,
        business_type: String,
        display_name: Option<String>,
        participants: Vec<String>,
    ) -> Self {
        let proto_summary = ProtoSessionSummary {
            session_id: id.to_string(),
            session_type: session_type.clone(),
            business_type: business_type.clone(),
            display_name: display_name.clone().unwrap_or_default(),
            avatar_url: String::new(),
            last_message: None,
            unread_count: 0,
            max_seq: 0,
            last_read_seq: 0,
            is_muted: false,
            is_pinned: false,
            updated_at: Some(prost_types::Timestamp {
                seconds: Utc::now().timestamp(),
                nanos: 0,
            }),
            metadata: HashMap::new(),
            labels: Vec::new(),
            is_muted_detail: false,
            mute_until: None,
            created_at: Some(prost_types::Timestamp {
                seconds: Utc::now().timestamp(),
                nanos: 0,
            }),
        };

        Self {
            id,
            session_type,
            business_type,
            display_name,
            avatar_url: None,
            participants,
            metadata: HashMap::new(),
            proto_summary,
        }
    }

    /// 从 ProtoSessionSummary 创建
    pub fn from_proto(proto: ProtoSessionSummary) -> Result<Self> {
        Ok(Self {
            id: SessionId::new(proto.session_id.clone()),
            session_type: proto.session_type.clone(),
            business_type: proto.business_type.clone(),
            display_name: if proto.display_name.is_empty() {
                None
            } else {
                Some(proto.display_name.clone())
            },
            avatar_url: if proto.avatar_url.is_empty() {
                None
            } else {
                Some(proto.avatar_url.clone())
            },
            participants: Vec::new(), // TODO: 从 proto 中提取
            metadata: proto.metadata.clone(),
            proto_summary: proto,
        })
    }

    /// 转换为 ProtoSessionSummary
    pub fn to_proto(&self) -> ProtoSessionSummary {
        self.proto_summary.clone()
    }

    /// 转换为 SessionSummary（用于会话列表展示）
    pub fn to_summary(&self) -> SessionSummary {
        SessionSummary::from(self.to_proto())
    }

    /// 验证会话
    pub fn validate(&self) -> Result<()> {
        if self.id.as_str().is_empty() {
            return Err(
                SessionError::ValidationFailed("Session ID cannot be empty".to_string()).into(),
            );
        }

        if self.session_type.is_empty() {
            return Err(
                SessionError::ValidationFailed("Session type cannot be empty".to_string()).into(),
            );
        }

        // 验证会话类型
        if !["single", "group", "channel"].contains(&self.session_type.as_str()) {
            return Err(SessionError::InvalidSessionType(self.session_type.clone()).into());
        }

        Ok(())
    }

    /// 创建会话（领域行为）
    ///
    /// 返回领域事件
    pub fn create(self) -> Result<SessionCreatedEvent> {
        // 验证会话
        self.validate()?;

        // 创建领域事件
        let event = SessionCreatedEvent {
            session_id: self.id.clone(),
            session_type: self.session_type.clone(),
            business_type: self.business_type.clone(),
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 更新会话（领域行为）
    ///
    /// 返回更新后的 Session 和领域事件
    pub fn update(
        mut self,
        updates: HashMap<String, String>,
    ) -> Result<(Self, SessionUpdatedEvent)> {
        // 应用更新
        for (key, value) in updates {
            match key.as_str() {
                "display_name" => {
                    let value_clone = value.clone();
                    self.display_name = Some(value);
                    self.proto_summary.display_name = value_clone;
                }
                "avatar_url" => {
                    let value_clone = value.clone();
                    self.avatar_url = Some(value.clone());
                    self.proto_summary.avatar_url = value_clone;
                }
                _ => {
                    let value_clone = value.clone();
                    self.metadata.insert(key.clone(), value_clone);
                    self.proto_summary.metadata.insert(key, value);
                }
            }
        }

        // 更新更新时间
        self.proto_summary.updated_at = Some(prost_types::Timestamp {
            seconds: Utc::now().timestamp(),
            nanos: 0,
        });

        // 创建领域事件
        let event = SessionUpdatedEvent {
            session_id: self.id.clone(),
            timestamp: Utc::now(),
        };

        Ok((self, event))
    }

    // Getters
    pub fn id(&self) -> &SessionId {
        &self.id
    }

    pub fn session_type(&self) -> &str {
        &self.session_type
    }

    pub fn business_type(&self) -> &str {
        &self.business_type
    }

    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    pub fn avatar_url(&self) -> Option<&str> {
        self.avatar_url.as_deref()
    }

    pub fn participants(&self) -> &[String] {
        &self.participants
    }

    pub fn metadata(&self) -> &HashMap<String, String> {
        &self.metadata
    }

    /// 删除会话（领域行为）
    pub fn delete(self) -> Result<SessionDeletedEvent> {
        // 验证会话
        self.validate()?;

        // 创建领域事件
        let event = SessionDeletedEvent {
            session_id: self.id.clone(),
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 隐藏会话（领域行为）
    pub fn hide(self) -> Result<SessionHiddenEvent> {
        // 验证会话
        self.validate()?;

        // 创建领域事件
        let event = SessionHiddenEvent {
            session_id: self.id.clone(),
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 显示会话（领域行为）
    pub fn show(self) -> Result<SessionShownEvent> {
        // 验证会话
        self.validate()?;

        // 创建领域事件
        let event = SessionShownEvent {
            session_id: self.id.clone(),
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 标记已读（领域行为）
    pub fn mark_read(
        self,
        reader_id: UserId,
        message_seq: Option<i64>,
    ) -> Result<SessionMarkedReadEvent> {
        // 验证会话
        self.validate()?;

        // 业务规则：message_seq 不能大于 max_seq
        if let Some(seq) = message_seq {
            if seq > self.proto_summary.max_seq as i64 {
                return Err(SessionError::ValidationFailed(format!(
                    "Message seq {} exceeds max seq {}",
                    seq, self.proto_summary.max_seq
                ))
                .into());
            }
        }

        // 创建领域事件
        let event = SessionMarkedReadEvent {
            session_id: self.id.clone(),
            message_seq,
            reader_id,
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 设置草稿（领域行为）
    pub fn set_draft(self, draft: Option<String>) -> Result<SessionDraftSetEvent> {
        // 验证会话
        self.validate()?;

        // 业务规则：草稿长度限制（5000字符）
        if let Some(ref draft_content) = draft {
            if draft_content.len() > 5000 {
                return Err(SessionError::ValidationFailed(
                    "Draft content exceeds 5000 characters".to_string(),
                )
                .into());
            }
        }

        // 创建领域事件
        let event = SessionDraftSetEvent {
            session_id: self.id.clone(),
            draft,
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 发送输入状态（领域行为）
    pub fn send_typing(self, user_id: UserId, is_typing: bool) -> Result<SessionTypingSentEvent> {
        // 验证会话
        self.validate()?;

        // 创建领域事件
        let event = SessionTypingSentEvent {
            session_id: self.id.clone(),
            user_id,
            is_typing,
            timestamp: Utc::now(),
        };

        Ok(event)
    }
}
