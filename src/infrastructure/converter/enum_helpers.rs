//! 枚举转换辅助函数
//!
//! 提供枚举值与 Protobuf 数值之间的转换，确保类型安全

use crate::domain::message::{
    MessageState, MessageSource, MessageType, ContentType, ConversationType,
};

/// 消息状态转换辅助函数
pub mod message_state {
    use super::*;
    
    /// 从 Protobuf 数值转换为 MessageState
    pub fn from_proto(value: i32) -> MessageState {
        match value {
            1 => MessageState::Created,
            2 => MessageState::Sent,
            3 => MessageState::Delivered,
            4 => MessageState::Read,
            5 => MessageState::Failed,
            6 => MessageState::Recalled,
            _ => MessageState::Created,
        }
    }
    
    /// 从 MessageState 转换为 Protobuf 数值
    pub fn to_proto(state: MessageState) -> i32 {
        match state {
            MessageState::Created => 1,
            MessageState::Sent => 2,
            MessageState::Delivered => 3,
            MessageState::Read => 4,
            MessageState::Failed => 5,
            MessageState::Recalled => 6,
        }
    }
}

/// 消息来源转换辅助函数
pub mod message_source {
    use super::*;
    
    /// 从 Protobuf 数值转换为 MessageSource
    pub fn from_proto(value: i32) -> MessageSource {
        match value {
            1 => MessageSource::User,
            2 => MessageSource::System,
            3 => MessageSource::Bot,
            4 => MessageSource::Admin,
            _ => MessageSource::User,
        }
    }
    
    /// 从 MessageSource 转换为 Protobuf 数值
    pub fn to_proto(source: MessageSource) -> i32 {
        match source {
            MessageSource::User => 1,
            MessageSource::System => 2,
            MessageSource::Bot => 3,
            MessageSource::Admin => 4,
        }
    }
}

/// 会话类型转换辅助函数
pub mod conversation_type {
    use super::*;
    
    /// 从 Protobuf 数值转换为 ConversationType
    pub fn from_proto(value: i32) -> ConversationType {
        match value {
            1 => ConversationType::Single,
            2 => ConversationType::Group,
            3 => ConversationType::Channel,
            _ => ConversationType::Single,
        }
    }
    
    /// 从 ConversationType 转换为 Protobuf 数值
    pub fn to_proto(conv_type: ConversationType) -> i32 {
        match conv_type {
            ConversationType::Single => 1,
            ConversationType::Group => 2,
            ConversationType::Channel => 3,
        }
    }
}

/// 消息类型转换辅助函数
pub mod message_type {
    use super::*;
    
    /// 从 Protobuf 数值转换为 MessageType
    pub fn from_proto(value: i32) -> MessageType {
        match value {
            1 => MessageType::Text,
            2 => MessageType::Image,
            3 => MessageType::Video,
            4 => MessageType::Audio,
            5 => MessageType::File,
            6 => MessageType::Location,
            7 => MessageType::Card,
            100 => MessageType::Custom,
            101 => MessageType::Notification,
            302 => MessageType::Operation,
            _ => MessageType::Text,
        }
    }
    
    /// 从 MessageType 转换为 Protobuf 数值
    pub fn to_proto(msg_type: MessageType) -> i32 {
        match msg_type {
            MessageType::Text => 1,
            MessageType::Image => 2,
            MessageType::Video => 3,
            MessageType::Audio => 4,
            MessageType::File => 5,
            MessageType::Location => 6,
            MessageType::Card => 7,
            MessageType::Custom => 100,
            MessageType::Notification => 101,
            MessageType::Operation => 302,
        }
    }
}

/// 内容类型转换辅助函数
pub mod content_type {
    use super::*;
    
    /// 从 Protobuf 数值转换为 ContentType
    pub fn from_proto(value: i32) -> ContentType {
        match value {
            1 => ContentType::PlainText,
            2 => ContentType::Markdown,
            3 => ContentType::Html,
            4 => ContentType::Json,
            _ => ContentType::PlainText,
        }
    }
    
    /// 从 ContentType 转换为 Protobuf 数值
    pub fn to_proto(content_type: ContentType) -> i32 {
        match content_type {
            ContentType::PlainText => 1,
            ContentType::Markdown => 2,
            ContentType::Html => 3,
            ContentType::Json => 4,
        }
    }
}
