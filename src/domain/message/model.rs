//! 消息领域模型
//!
//! 包含 Message 聚合根、值对象等

use anyhow::{Context, Result};
use chrono::Utc;
use flare_proto::{Message as ProtoMessage, MessageStatus, MessageType};
use prost_types::Timestamp;
use std::time::Duration;

use super::event::{
    MessageDeletedEvent, MessageEditedEvent, MessageFavoritedEvent, MessageForwardedEvent,
    MessagePinnedEvent, MessageReactionAddedEvent, MessageReactionRemovedEvent,
    MessageRecalledEvent, MessageReceivedEvent, MessageSentEvent, MessageUnfavoritedEvent,
    MessageUnpinnedEvent,
};
use crate::domain::message::repository::MessageRepository;

/// MessageId 值对象
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageId(String);

impl MessageId {
    pub fn new(id: String) -> Self {
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for MessageId {
    fn from(id: String) -> Self {
        Self::new(id)
    }
}

impl From<&str> for MessageId {
    fn from(id: &str) -> Self {
        Self::new(id.to_string())
    }
}

impl std::fmt::Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// SessionId 值对象
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(id: String) -> Self {
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for SessionId {
    fn from(id: String) -> Self {
        Self::new(id)
    }
}

impl From<&str> for SessionId {
    fn from(id: &str) -> Self {
        Self::new(id.to_string())
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// UserId 值对象
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UserId(String);

impl UserId {
    pub fn new(id: String) -> Self {
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for UserId {
    fn from(id: String) -> Self {
        Self::new(id)
    }
}

impl From<&str> for UserId {
    fn from(id: &str) -> Self {
        Self::new(id.to_string())
    }
}

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 消息错误
#[derive(Debug, thiserror::Error)]
pub enum MessageError {
    #[error("Message validation failed: {0}")]
    ValidationFailed(String),

    #[error("Not authorized to perform this operation")]
    NotAuthorized,

    #[error("Message recall timeout (max {0} seconds)")]
    RecallTimeout(u64),

    #[error("Message not found")]
    NotFound,

    #[error("Invalid message status")]
    InvalidStatus,

    #[error("Message edit timeout (max {0} seconds)")]
    EditTimeout(u64),
}

/// Message 聚合根
///
/// 封装消息的领域逻辑和行为
pub struct Message {
    id: MessageId,
    session_id: SessionId,
    sender_id: UserId,
    receiver_id: Option<UserId>,
    channel_id: Option<String>,
    content: flare_proto::MessageContent,
    message_type: MessageType,
    status: MessageStatus,
    timestamp: Timestamp,
    // 其他字段...
    proto_message: ProtoMessage,
}

impl Message {
    /// 创建新消息
    pub fn new(
        id: MessageId,
        session_id: SessionId,
        sender_id: UserId,
        content: flare_proto::MessageContent,
        message_type: MessageType,
    ) -> Self {
        let now = Utc::now();
        let timestamp = Timestamp {
            seconds: now.timestamp(),
            nanos: 0,
        };

        let proto_message = ProtoMessage {
            id: id.to_string(),
            session_id: session_id.to_string(),
            sender_id: sender_id.to_string(),
            content: Some(content.clone()),
            message_type: message_type as i32,
            status: MessageStatus::Created as i32,
            timestamp: Some(timestamp.clone()),
            ..Default::default()
        };

        Self {
            id,
            session_id,
            sender_id,
            receiver_id: None,
            channel_id: None,
            content,
            message_type,
            status: MessageStatus::Created,
            timestamp,
            proto_message,
        }
    }

    /// 从 ProtoMessage 创建
    pub fn from_proto(proto: ProtoMessage) -> Result<Self> {
        Ok(Self {
            id: MessageId::new(proto.id.clone()),
            session_id: SessionId::new(proto.session_id.clone()),
            sender_id: UserId::new(proto.sender_id.clone()),
            receiver_id: if proto.receiver_id.is_empty() {
                None
            } else {
                Some(UserId::new(proto.receiver_id.clone()))
            },
            channel_id: if proto.channel_id.is_empty() {
                None
            } else {
                Some(proto.channel_id.clone())
            },
            content: proto.content.clone().ok_or_else(|| {
                MessageError::ValidationFailed("Message content is required".to_string())
            })?,
            message_type: MessageType::try_from(proto.message_type)
                .map_err(|_| MessageError::ValidationFailed("Invalid message type".to_string()))?,
            status: MessageStatus::try_from(proto.status).map_err(|_| {
                MessageError::ValidationFailed("Invalid message status".to_string())
            })?,
            timestamp: proto.timestamp.clone().unwrap_or_else(|| Timestamp {
                seconds: Utc::now().timestamp(),
                nanos: 0,
            }),
            proto_message: proto,
        })
    }

    /// 转换为 ProtoMessage
    pub fn to_proto(&self) -> ProtoMessage {
        self.proto_message.clone()
    }

    /// 验证消息
    pub fn validate(&self) -> Result<()> {
        if self.id.as_str().is_empty() {
            return Err(
                MessageError::ValidationFailed("Message ID cannot be empty".to_string()).into(),
            );
        }

        if self.session_id.as_str().is_empty() {
            return Err(
                MessageError::ValidationFailed("Session ID cannot be empty".to_string()).into(),
            );
        }

        if self.sender_id.as_str().is_empty() {
            return Err(
                MessageError::ValidationFailed("Sender ID cannot be empty".to_string()).into(),
            );
        }

        Ok(())
    }

    /// 发送消息（领域行为）
    ///
    /// 返回领域事件
    pub fn send(self) -> Result<MessageSentEvent> {
        // 验证消息
        self.validate()?;

        // 创建领域事件
        let event = MessageSentEvent {
            message_id: self.id.clone(),
            session_id: self.session_id.clone(),
            sender_id: self.sender_id.clone(),
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 撤回消息（领域行为）
    ///
    /// 业务规则：
    /// 1. 只能撤回自己的消息
    /// 2. 只能撤回一定时间内的消息（默认 2 分钟）
    pub fn recall(
        self,
        current_user_id: &UserId,
        reason: Option<String>,
    ) -> Result<MessageRecalledEvent> {
        // 业务规则：只能撤回自己的消息
        if &self.sender_id != current_user_id {
            return Err(MessageError::NotAuthorized.into());
        }

        // 业务规则：只能撤回一定时间内的消息（2 分钟）
        const MAX_RECALL_DURATION_SECS: i64 = 120;
        let message_time = chrono::DateTime::<chrono::Utc>::from_timestamp(
            self.timestamp.seconds,
            self.timestamp.nanos as u32,
        )
        .ok_or_else(|| MessageError::ValidationFailed("Invalid timestamp".to_string()))?;

        let elapsed = Utc::now().signed_duration_since(message_time);
        if elapsed.num_seconds() > MAX_RECALL_DURATION_SECS {
            return Err(MessageError::RecallTimeout(MAX_RECALL_DURATION_SECS as u64).into());
        }

        // 创建领域事件
        let event = MessageRecalledEvent {
            message_id: self.id.clone(),
            session_id: self.session_id.clone(),
            reason,
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 删除消息（领域行为）
    pub fn delete(self, current_user_id: &UserId, delete_type: i32) -> Result<MessageDeletedEvent> {
        // 业务规则：只能删除自己的消息或管理员删除
        // 这里简化处理，实际应该检查权限
        if &self.sender_id != current_user_id {
            // TODO: 检查管理员权限
        }

        // 创建领域事件
        let event = MessageDeletedEvent {
            message_id: self.id.clone(),
            session_id: self.session_id.clone(),
            delete_type,
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    // Getters
    pub fn id(&self) -> &MessageId {
        &self.id
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub fn sender_id(&self) -> &UserId {
        &self.sender_id
    }

    pub fn receiver_id(&self) -> Option<&UserId> {
        self.receiver_id.as_ref()
    }

    pub fn channel_id(&self) -> Option<&str> {
        self.channel_id.as_deref()
    }

    pub fn content(&self) -> &flare_proto::MessageContent {
        &self.content
    }

    pub fn message_type(&self) -> MessageType {
        self.message_type
    }

    pub fn status(&self) -> MessageStatus {
        self.status
    }

    pub fn timestamp(&self) -> &Timestamp {
        &self.timestamp
    }

    /// 更新消息状态（领域行为）
    pub fn update_status(mut self, status: MessageStatus) -> Self {
        self.status = status;
        // 更新 proto_message 中的状态
        self.proto_message.status = status as i32;
        self
    }

    /// 设置接收者ID
    pub fn set_receiver_id(mut self, receiver_id: Option<UserId>) -> Self {
        self.receiver_id = receiver_id.clone();
        if let Some(ref id) = receiver_id {
            self.proto_message.receiver_id = id.to_string();
        } else {
            self.proto_message.receiver_id = String::new();
        }
        self
    }

    /// 设置频道ID
    pub fn set_channel_id(mut self, channel_id: Option<String>) -> Self {
        self.channel_id = channel_id.clone();
        if let Some(ref id) = channel_id {
            self.proto_message.channel_id = id.clone();
        } else {
            self.proto_message.channel_id = String::new();
        }
        self
    }

    /// 接收消息（领域行为）
    ///
    /// 当收到服务端推送的消息时调用
    pub fn receive(self, receiver_id: UserId) -> Result<MessageReceivedEvent> {
        // 验证消息
        self.validate()?;

        // 创建领域事件
        let event = MessageReceivedEvent {
            message_id: self.id.clone(),
            session_id: self.session_id.clone(),
            receiver_id,
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 编辑消息（领域行为）
    ///
    /// 业务规则：
    /// 1. 只能编辑自己的消息
    /// 2. 只能编辑文本消息
    /// 3. 只能编辑一定时间内的消息（默认 5 分钟）
    pub fn edit(self, editor_id: &UserId, new_content: String) -> Result<MessageEditedEvent> {
        // 业务规则：只能编辑自己的消息
        if &self.sender_id != editor_id {
            return Err(MessageError::NotAuthorized.into());
        }

        // 业务规则：只能编辑文本消息
        if self.message_type != MessageType::Text {
            return Err(MessageError::ValidationFailed(
                "Only text messages can be edited".to_string(),
            )
            .into());
        }

        // 业务规则：只能编辑一定时间内的消息（5 分钟）
        const MAX_EDIT_DURATION_SECS: i64 = 300;
        let message_time = chrono::DateTime::<chrono::Utc>::from_timestamp(
            self.timestamp.seconds,
            self.timestamp.nanos as u32,
        )
        .ok_or_else(|| MessageError::ValidationFailed("Invalid timestamp".to_string()))?;

        let elapsed = Utc::now().signed_duration_since(message_time);
        if elapsed.num_seconds() > MAX_EDIT_DURATION_SECS {
            return Err(MessageError::ValidationFailed(format!(
                "Message edit timeout (max {} seconds)",
                MAX_EDIT_DURATION_SECS
            ))
            .into());
        }

        // 创建领域事件
        let event = MessageEditedEvent {
            message_id: self.id.clone(),
            session_id: self.session_id.clone(),
            editor_id: editor_id.clone(),
            new_content,
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 转发消息（领域行为）
    ///
    /// 创建转发消息的领域事件
    pub fn forward(
        self,
        target_session_id: SessionId,
        forwarder_id: UserId,
        new_message_id: MessageId,
    ) -> Result<MessageForwardedEvent> {
        // 验证消息
        self.validate()?;

        // 创建领域事件
        let event = MessageForwardedEvent {
            original_message_id: self.id.clone(),
            original_session_id: self.session_id.clone(),
            new_message_id,
            target_session_id,
            forwarder_id,
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 添加反应（领域行为）
    pub fn add_reaction(self, user_id: UserId, emoji: String) -> Result<MessageReactionAddedEvent> {
        // 验证消息
        self.validate()?;

        // 创建领域事件
        let event = MessageReactionAddedEvent {
            message_id: self.id.clone(),
            session_id: self.session_id.clone(),
            user_id,
            emoji,
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 移除反应（领域行为）
    pub fn remove_reaction(
        self,
        user_id: UserId,
        emoji: String,
    ) -> Result<MessageReactionRemovedEvent> {
        // 验证消息
        self.validate()?;

        // 创建领域事件
        let event = MessageReactionRemovedEvent {
            message_id: self.id.clone(),
            session_id: self.session_id.clone(),
            user_id,
            emoji,
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 置顶消息（领域行为）
    pub fn pin(
        self,
        user_id: UserId,
        expire_at: Option<prost_types::Timestamp>,
    ) -> Result<MessagePinnedEvent> {
        // 验证消息
        self.validate()?;

        // 创建领域事件
        let event = MessagePinnedEvent {
            message_id: self.id.clone(),
            session_id: self.session_id.clone(),
            user_id,
            expire_at,
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 取消置顶（领域行为）
    pub fn unpin(self, user_id: UserId) -> Result<MessageUnpinnedEvent> {
        // 验证消息
        self.validate()?;

        // 创建领域事件
        let event = MessageUnpinnedEvent {
            message_id: self.id.clone(),
            session_id: self.session_id.clone(),
            user_id,
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 收藏消息（领域行为）
    pub fn favorite(
        self,
        user_id: UserId,
        tags: Option<Vec<String>>,
        note: Option<String>,
    ) -> Result<MessageFavoritedEvent> {
        // 验证消息
        self.validate()?;

        // 创建领域事件
        let event = MessageFavoritedEvent {
            message_id: self.id.clone(),
            session_id: self.session_id.clone(),
            user_id,
            tags,
            note,
            timestamp: Utc::now(),
        };

        Ok(event)
    }

    /// 取消收藏（领域行为）
    pub fn unfavorite(self, user_id: UserId) -> Result<MessageUnfavoritedEvent> {
        // 验证消息
        self.validate()?;

        // 创建领域事件
        let event = MessageUnfavoritedEvent {
            message_id: self.id.clone(),
            session_id: self.session_id.clone(),
            user_id,
            timestamp: Utc::now(),
        };

        Ok(event)
    }
}
