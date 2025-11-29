//! 消息模型
//!
//! 封装 flare-proto::Message，提供更友好的 API
//!
//! 设计原则：
//! 1. 基础消息结构直接使用 flare-proto::Message（与服务端一致）
//! 2. SDK 扩展信息通过 ExtendedMessage 添加（头像、名称、本地状态等）

pub use flare_proto::Message;

// 重新导出常用的枚举和类型
pub use flare_proto::{
    MessageType, MessageStatus, MessageSource, ContentType,
    MessageContent, TextContent, ImageContent, VideoContent, AudioContent,
    FileContent, LocationContent, CardContent, NotificationContent, CustomContent,
    ForwardContent, TypingContent,
    MessageTimeline, MessageReadRecord, MessageOperation,
    VisibilityStatus,
};

use crate::model::extension::{MessageExtension, MessageLocalState};

/// 带扩展的消息（SDK 使用）
/// 
/// 包含基础消息（来自 flare-proto）和 SDK 扩展信息（头像、名称、本地状态等）
#[derive(Debug, Clone)]
pub struct ExtendedMessage {
    /// 基础消息（来自 flare-proto）
    pub message: Message,
    
    /// SDK 扩展信息
    pub extension: MessageExtension,
}

impl ExtendedMessage {
    /// 从 Message 创建，扩展信息为空
    pub fn from_message(message: Message) -> Self {
        Self {
            message,
            extension: MessageExtension::default(),
        }
    }
    
    /// 从 Message 和 Extension 创建
    pub fn new(message: Message, extension: MessageExtension) -> Self {
        Self { message, extension }
    }
    
    /// 获取发送者头像（优先使用扩展字段）
    pub fn sender_avatar(&self) -> Option<&str> {
        self.extension.sender_avatar.as_deref()
            .or_else(|| {
                if self.message.sender_avatar_url.is_empty() {
                    None
                } else {
                    Some(self.message.sender_avatar_url.as_str())
                }
            })
    }
    
    /// 获取发送者名称（优先使用扩展字段）
    pub fn sender_name(&self) -> Option<&str> {
        self.extension.sender_name.as_deref()
            .or_else(|| {
                if self.message.sender_nickname.is_empty() {
                    None
                } else {
                    Some(self.message.sender_nickname.as_str())
                }
            })
    }
    
    /// 获取消息本地状态
    pub fn local_state(&self) -> Option<MessageLocalState> {
        self.extension.local_state
    }
    
    /// 设置发送者头像
    pub fn set_sender_avatar(&mut self, avatar: Option<String>) {
        self.extension.sender_avatar = avatar;
    }
    
    /// 设置发送者名称
    pub fn set_sender_name(&mut self, name: Option<String>) {
        self.extension.sender_name = name;
    }
    
    /// 设置本地状态
    pub fn set_local_state(&mut self, state: Option<MessageLocalState>) {
        self.extension.local_state = state;
    }
    
    /// 更新下载进度
    pub fn update_download_progress(&mut self, progress: u8) {
        self.extension.download_progress = Some(progress);
        if progress == 100 {
            self.extension.is_downloaded = Some(true);
        }
    }
    
    /// 标记为已下载
    pub fn mark_as_downloaded(&mut self) {
        self.extension.is_downloaded = Some(true);
        self.extension.download_progress = Some(100);
    }
    
    /// 获取自定义扩展字段
    pub fn get_custom_field(&self, key: &str) -> Option<&String> {
        self.extension.custom.get(key)
    }
    
    /// 设置自定义扩展字段
    pub fn set_custom_field(&mut self, key: String, value: String) {
        self.extension.custom.insert(key, value);
    }
}

impl From<Message> for ExtendedMessage {
    fn from(message: Message) -> Self {
        Self::from_message(message)
    }
}

impl From<ExtendedMessage> for Message {
    fn from(extended: ExtendedMessage) -> Self {
        extended.message
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_types::Timestamp;
    
    #[test]
    fn test_message_creation() {
        let mut message = Message::default();
        message.id = "test-123".to_string();
        message.session_id = "session-456".to_string();
        message.sender_id = "user-789".to_string();
        message.message_type = MessageType::Text as i32;
        message.status = MessageStatus::Created as i32;
        message.source = MessageSource::User as i32;
        message.content_type = ContentType::PlainText as i32;
        
        // 设置文本内容
        message.content = Some(MessageContent {
            content: Some(flare_proto::flare::common::v1::message_content::Content::Text(
                TextContent {
                    text: "Hello, World!".to_string(),
                    mentions: vec![],
                }
            )),
        });
        
        // 设置时间戳
        message.timestamp = Some(Timestamp {
            seconds: 1234567890,
            nanos: 0,
        });
        
        assert_eq!(message.id, "test-123");
        assert_eq!(message.session_id, "session-456");
    }
}
