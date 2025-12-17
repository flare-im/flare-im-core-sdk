//! 会话领域事件
//!
//! 表示会话相关的业务事件
//!
//! ## 事件设计原则
//!
//! 1. **不可变**：事件一旦创建就不能修改
//! 2. **时间戳**：所有事件都包含时间戳
//! 3. **聚合根ID**：包含相关的聚合根ID（session_id等）
//! 4. **业务语义**：事件名称清晰表达业务含义

use crate::domain::message::model::{SessionId, UserId};
use chrono::DateTime;
use chrono::Utc;

/// 会话已创建事件
#[derive(Debug, Clone)]
pub struct SessionCreatedEvent {
    pub session_id: SessionId,
    pub session_type: String,
    pub business_type: String,
    pub timestamp: DateTime<Utc>,
}

/// 会话已更新事件
#[derive(Debug, Clone)]
pub struct SessionUpdatedEvent {
    pub session_id: SessionId,
    pub timestamp: DateTime<Utc>,
}

/// 会话已删除事件
#[derive(Debug, Clone)]
pub struct SessionDeletedEvent {
    pub session_id: SessionId,
    pub timestamp: DateTime<Utc>,
}

/// 会话已隐藏事件
#[derive(Debug, Clone)]
pub struct SessionHiddenEvent {
    pub session_id: SessionId,
    pub timestamp: DateTime<Utc>,
}

/// 会话已显示事件
#[derive(Debug, Clone)]
pub struct SessionShownEvent {
    pub session_id: SessionId,
    pub timestamp: DateTime<Utc>,
}

/// 会话已标记为已读事件
#[derive(Debug, Clone)]
pub struct SessionMarkedReadEvent {
    pub session_id: SessionId,
    pub message_seq: Option<i64>,
    pub reader_id: UserId,
    pub timestamp: DateTime<Utc>,
}

/// 会话草稿已设置事件
#[derive(Debug, Clone)]
pub struct SessionDraftSetEvent {
    pub session_id: SessionId,
    pub draft: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// 会话输入状态已发送事件
#[derive(Debug, Clone)]
pub struct SessionTypingSentEvent {
    pub session_id: SessionId,
    pub user_id: UserId,
    pub is_typing: bool,
    pub timestamp: DateTime<Utc>,
}
