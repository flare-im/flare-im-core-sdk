//! 消息构建工具
//!
//! 提供便捷的消息构建 API，对标微信、Telegram、飞书的生产级别实现

use crate::domain::message::*;
use std::collections::HashMap;
use flare_proto::MessageContentExt;

/// 消息构建器
///
/// 提供链式 API 构建消息
pub struct MessageBuilder {
    server_id: Option<String>,
    client_msg_id: Option<String>,
    conversation_id: Option<String>,
    sender_id: Option<String>,
    message_type: Option<MessageType>,
    content: Option<Vec<u8>>,
    content_type: ContentType,
    conversation_type: ConversationType,
    source: MessageSource,
    business_type: Option<String>,
    receiver_id: Option<String>,
    channel_id: Option<String>,
    attachments: Vec<MediaAttachment>,
    quote: Option<QuoteContent>,
    extra: HashMap<String, String>,
    attributes: HashMap<String, String>,
    is_burn_after_read: bool,
    burn_after_seconds: Option<i32>,
    tags: Vec<String>,
}

impl MessageBuilder {
    /// 创建新的消息构建器
    pub fn new() -> Self {
        Self {
            server_id: None,
            client_msg_id: None,
            conversation_id: None,
            sender_id: None,
            message_type: None,
            content: None,
            content_type: ContentType::PlainText,
            conversation_type: ConversationType::Single,
            source: MessageSource::User,
            business_type: None,
            receiver_id: None,
            channel_id: None,
            attachments: Vec::new(),
            quote: None,
            extra: HashMap::new(),
            attributes: HashMap::new(),
            is_burn_after_read: false,
            burn_after_seconds: None,
            tags: Vec::new(),
        }
    }
    
    /// 设置服务端消息ID
    pub fn with_server_id(mut self, server_id: String) -> Self {
        self.server_id = Some(server_id);
        self
    }
    
    /// 设置消息ID（兼容性方法，内部使用 server_id）
    #[deprecated(note = "使用 with_server_id 代替")]
    pub fn with_id(mut self, id: String) -> Self {
        self.server_id = Some(id);
        self
    }
    
    /// 设置客户端消息ID
    pub fn with_client_msg_id(mut self, client_msg_id: String) -> Self {
        self.client_msg_id = Some(client_msg_id);
        self
    }
    
    /// 设置会话ID
    pub fn with_conversation_id(mut self, conversation_id: impl Into<Option<String>>) -> Self {
        self.conversation_id = conversation_id.into();
        self
    }
    
    /// 设置发送者ID
    pub fn with_sender_id(mut self, sender_id: String) -> Self {
        self.sender_id = Some(sender_id);
        self
    }
    
    /// 设置消息类型
    pub fn with_message_type(mut self, message_type: MessageType) -> Self {
        self.message_type = Some(message_type);
        self
    }
    
    /// 设置内容
    pub fn with_content(mut self, content: Vec<u8>) -> Self {
        self.content = Some(content);
        self
    }
    
    /// 设置内容类型
    pub fn with_content_type(mut self, content_type: ContentType) -> Self {
        self.content_type = content_type;
        self
    }
    
    /// 设置会话类型
    pub fn with_conversation_type(mut self, conversation_type: ConversationType) -> Self {
        self.conversation_type = conversation_type;
        self
    }
    
    /// 设置消息来源
    pub fn with_source(mut self, source: MessageSource) -> Self {
        self.source = source;
        self
    }
    
    /// 设置业务类型
    pub fn with_business_type(mut self, business_type: String) -> Self {
        self.business_type = Some(business_type);
        self
    }
    
    /// 设置接收者ID（单聊）
    pub fn with_receiver_id(mut self, receiver_id: String) -> Self {
        self.receiver_id = Some(receiver_id);
        self
    }
    
    /// 设置通道ID（群聊/频道）
    pub fn with_channel_id(mut self, channel_id: String) -> Self {
        self.channel_id = Some(channel_id);
        self
    }
    
    /// 添加附件
    pub fn with_attachment(mut self, attachment: MediaAttachment) -> Self {
        self.attachments.push(attachment);
        self
    }
    
    /// 设置引用内容
    pub fn with_quote(mut self, quote: QuoteContent) -> Self {
        self.quote = Some(quote);
        self
    }
    
    /// 添加扩展字段
    pub fn with_extra(mut self, key: String, value: String) -> Self {
        self.extra.insert(key, value);
        self
    }
    
    /// 添加业务属性
    pub fn with_attribute(mut self, key: String, value: String) -> Self {
        self.attributes.insert(key, value);
        self
    }
    
    /// 设置阅后即焚
    pub fn with_burn_after_read(mut self, seconds: Option<i32>) -> Self {
        self.is_burn_after_read = seconds.is_some();
        self.burn_after_seconds = seconds;
        self
    }
    
    /// 添加标签
    pub fn with_tag(mut self, tag: String) -> Self {
        self.tags.push(tag);
        self
    }
    
    /// 构建消息
    pub fn build(self) -> anyhow::Result<Message> {
        let client_msg_id = self.client_msg_id
            .ok_or_else(|| anyhow::anyhow!("Client message ID is required"))?;
        let sender_id = self.sender_id
            .ok_or_else(|| anyhow::anyhow!("Sender ID is required"))?;
        let message_type = self.message_type
            .ok_or_else(|| anyhow::anyhow!("Message type is required"))?;
        let content = self.content
            .ok_or_else(|| anyhow::anyhow!("Message content is required"))?;
        let mut message = Message::new(
            None,
            client_msg_id,
            sender_id,
            message_type,
            content,
        );
        
        // 设置其他字段
        message.content_type = self.content_type;
        message.conversation_type = self.conversation_type;
        message.source = self.source;
        message.business_type = self.business_type;
        message.receiver_id = self.receiver_id;
        message.channel_id = self.channel_id;
        message.attachments = self.attachments;
        message.quote = self.quote;
        message.extra = self.extra;
        message.attributes = self.attributes;
        message.is_burn_after_read = self.is_burn_after_read;
        message.burn_after_seconds = self.burn_after_seconds;
        message.tags = self.tags;
        
        Ok(message)
    }
}

impl Default for MessageBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// 便捷方法：构建文本消息
///
/// # 参数
/// * `conversation_id` - 会话 ID（可选，支持草稿消息）
/// * `sender_id` - 发送者 ID
/// * `text` - 消息文本内容
/// * `receiver_id` - 接收者 ID（单聊时必需，群聊时可选）
///
/// # 注意
/// 对于单聊消息，`receiver_id` 是必需的，Message Orchestrator 会验证此字段
pub fn build_text_message(
    conversation_id: Option<String>,
    sender_id: String,
    text: String,
    receiver_id: Option<String>,
) -> anyhow::Result<Message> {
    use uuid::Uuid;
    
    // 构建文本内容（原封不动，不做任何处理）
    use flare_proto::flare::common::v1::{MessageContent, message_content::Content};
    let text_content = flare_proto::flare::common::v1::TextContent {
        text: text,
        mentions: Vec::new(),
    };
    let mut content = MessageContent::default();
    content.content = Some(Content::Text(text_content));
    
    let buf = content.encode_to_bytes()
        .map_err(|e| anyhow::anyhow!("Failed to encode MessageContent: {}", e))?;
    
    let mut builder = MessageBuilder::new()
        .with_server_id(Uuid::new_v4().to_string())
        .with_client_msg_id(Uuid::new_v4().to_string())
        .with_sender_id(sender_id)
        .with_message_type(MessageType::Text)
        .with_content(buf)
        .with_content_type(ContentType::PlainText);
    
    // 如果提供了 conversation_id，则设置
    if let Some(conv_id) = conversation_id {
        builder = builder.with_conversation_id(conv_id);
    }
    
    // 如果是单聊，设置 receiver_id
    if let Some(recv_id) = receiver_id {
        builder = builder.with_receiver_id(recv_id);
    }
    
    builder.build()
}

/// 便捷方法：构建图片消息
pub fn build_image_message(
    conversation_id: Option<String>,
    sender_id: String,
    image_url: String,
    local_path: Option<String>,
) -> anyhow::Result<Message> {
    use uuid::Uuid;
    
    // 构建图片内容
    use flare_proto::flare::common::v1::{MessageContent, message_content::Content, ImageContent, ImageInfo};
    let image_info = ImageInfo {
        uuid: Uuid::new_v4().to_string(),
        url: image_url.clone(),
        mime_type: "image/jpeg".to_string(),
        size: 0,
        width: 0,
        height: 0,
    };
    let image_content = ImageContent {
        image_id: Uuid::new_v4().to_string(),
        source: Some(image_info),
        thumbnail: None,
        description: String::new(),
    };
    let mut content = MessageContent::default();
    content.content = Some(Content::Image(image_content));
    
    let buf = content.encode_to_bytes()
        .map_err(|e| anyhow::anyhow!("Failed to encode MessageContent: {}", e))?;
    
    let mut builder = MessageBuilder::new()
        .with_server_id(Uuid::new_v4().to_string())
        .with_client_msg_id(Uuid::new_v4().to_string())
        .with_sender_id(sender_id)
        .with_message_type(MessageType::Image)
        .with_content(buf);
    
    // 如果提供了 conversation_id，则设置
    if let Some(conv_id) = conversation_id {
        builder = builder.with_conversation_id(conv_id);
    }
    
    // 如果有本地路径，添加为附件
    if let Some(path) = local_path {
        // 获取文件大小
        let file_size = std::fs::metadata(&path)
            .map(|m| m.len())
            .unwrap_or(0);
        
        builder = builder.with_attachment(MediaAttachment {
            attachment_id: Uuid::new_v4().to_string(),
            attachment_type: "image".to_string(),
            url: image_url,
            size: file_size,
            mime_type: "image/jpeg".to_string(),
            metadata: {
                let mut m = HashMap::new();
                m.insert("local_path".to_string(), path);
                m
            },
        });
    }
    
    builder.build()
}

/// 便捷方法：构建文件消息
pub fn build_file_message(
    conversation_id: Option<String>,
    sender_id: String,
    file_url: String,
    file_name: String,
    file_size: u64,
    mime_type: String,
    local_path: Option<String>,
) -> anyhow::Result<Message> {
    use uuid::Uuid;
    
    // 构建文件内容
    use flare_proto::flare::common::v1::{MessageContent, message_content::Content, FileContent};
    let file_content = FileContent {
        file_id: Uuid::new_v4().to_string(),
        file_name: file_name.clone(),
        file_size: file_size as i64,
        mime_type: mime_type.clone(),
        url: file_url.clone(),
        description: String::new(),
    };
    let mut content = MessageContent::default();
    content.content = Some(Content::File(file_content));
    
    let buf = content.encode_to_bytes()
        .map_err(|e| anyhow::anyhow!("Failed to encode MessageContent: {}", e))?;
    
    let mut builder = MessageBuilder::new()
        .with_server_id(Uuid::new_v4().to_string())
        .with_client_msg_id(Uuid::new_v4().to_string())
        .with_sender_id(sender_id)
        .with_message_type(MessageType::File)
        .with_content(buf);
    
    // 如果提供了 conversation_id，则设置
    if let Some(conv_id) = conversation_id {
        builder = builder.with_conversation_id(conv_id);
    }
    
    // 如果有本地路径，添加为附件
    if let Some(path) = local_path {
        builder = builder.with_attachment(MediaAttachment {
            attachment_id: Uuid::new_v4().to_string(),
            attachment_type: "file".to_string(),
            url: file_url,
            size: file_size,
            mime_type: mime_type,
            metadata: {
                let mut m = HashMap::new();
                m.insert("local_path".to_string(), path);
                m.insert("file_name".to_string(), file_name);
                m
            },
        });
    }
    
    builder.build()
}

/// 便捷方法：构建视频消息
pub fn build_video_message(
    conversation_id: Option<String>,
    sender_id: String,
    video_url: String,
    local_path: Option<String>,
    duration_ms: u64,
    width: i32,
    height: i32,
) -> anyhow::Result<Message> {
    use uuid::Uuid;
    
    // 构建视频内容
    use flare_proto::flare::common::v1::{MessageContent, message_content::Content, VideoContent, VideoInfo};
    let video_info = VideoInfo {
        uuid: Uuid::new_v4().to_string(),
        url: video_url.clone(),
        mime_type: "video/mp4".to_string(),
        size: 0,
        duration_ms: duration_ms as i64,
        width,
        height,
    };
    let video_content = VideoContent {
        video_id: Uuid::new_v4().to_string(),
        source: Some(video_info),
        cover: None,
        description: String::new(),
    };
    let mut content = MessageContent::default();
    content.content = Some(Content::Video(video_content));
    
    let buf = content.encode_to_bytes()
        .map_err(|e| anyhow::anyhow!("Failed to encode MessageContent: {}", e))?;
    
    let mut builder = MessageBuilder::new()
        .with_server_id(Uuid::new_v4().to_string())
        .with_client_msg_id(Uuid::new_v4().to_string())
        .with_sender_id(sender_id)
        .with_message_type(MessageType::Video)
        .with_content(buf);
    
    // 如果提供了 conversation_id，则设置
    if let Some(conv_id) = conversation_id {
        builder = builder.with_conversation_id(conv_id);
    }
    
    // 如果有本地路径，添加为附件
    if let Some(path) = local_path {
        let file_size = std::fs::metadata(&path)
            .map(|m| m.len())
            .unwrap_or(0);
        
        builder = builder.with_attachment(MediaAttachment {
            attachment_id: Uuid::new_v4().to_string(),
            attachment_type: "video".to_string(),
            url: video_url,
            size: file_size,
            mime_type: "video/mp4".to_string(),
            metadata: {
                let mut m = HashMap::new();
                m.insert("local_path".to_string(), path);
                m.insert("duration_ms".to_string(), duration_ms.to_string());
                m
            },
        });
    }
    
    builder.build()
}

/// 便捷方法：构建语音消息
pub fn build_audio_message(
    conversation_id: Option<String>,
    sender_id: String,
    audio_url: String,
    local_path: Option<String>,
    duration_ms: u64,
) -> anyhow::Result<Message> {
    use uuid::Uuid;
    
    // 构建语音内容
    use flare_proto::flare::common::v1::{MessageContent, message_content::Content, AudioContent, AudioInfo};
    let audio_info = AudioInfo {
        uuid: Uuid::new_v4().to_string(),
        url: audio_url.clone(),
        mime_type: "audio/mpeg".to_string(),
        size: 0,
        duration_ms: duration_ms as i64,
    };
    let audio_content = AudioContent {
        audio_id: Uuid::new_v4().to_string(),
        source: Some(audio_info),
        description: String::new(),
    };
    let mut content = MessageContent::default();
    content.content = Some(Content::Audio(audio_content));
    
    let buf = content.encode_to_bytes()
        .map_err(|e| anyhow::anyhow!("Failed to encode MessageContent: {}", e))?;
    
    let mut builder = MessageBuilder::new()
        .with_server_id(Uuid::new_v4().to_string())
        .with_client_msg_id(Uuid::new_v4().to_string())
        .with_sender_id(sender_id)
        .with_message_type(MessageType::Audio)
        .with_content(buf);
    
    // 如果提供了 conversation_id，则设置
    if let Some(conv_id) = conversation_id {
        builder = builder.with_conversation_id(conv_id);
    }
    
    // 如果有本地路径，添加为附件
    if let Some(path) = local_path {
        let file_size = std::fs::metadata(&path)
            .map(|m| m.len())
            .unwrap_or(0);
        
        builder = builder.with_attachment(MediaAttachment {
            attachment_id: Uuid::new_v4().to_string(),
            attachment_type: "audio".to_string(),
            url: audio_url,
            size: file_size,
            mime_type: "audio/mpeg".to_string(),
            metadata: {
                let mut m = HashMap::new();
                m.insert("local_path".to_string(), path);
                m.insert("duration_ms".to_string(), duration_ms.to_string());
                m
            },
        });
    }
    
    builder.build()
}

/// 便捷方法：构建回复消息
///
/// # 参数
/// * `conversation_id` - 会话 ID（可选，支持草稿消息）
/// * `sender_id` - 发送者 ID
/// * `quoted_message_id` - 被引用的消息ID（用于标识回复关系）
/// * `quoted_sender_id` - 被引用消息的发送者ID（可选）
/// * `quoted_text_preview` - 引用内容预览（可选）
/// * `reply_content` - 回复的消息内容（序列化的 MessageContent）
pub fn build_reply_message(
    conversation_id: Option<String>,
    sender_id: String,
    quoted_message_id: String,
    quoted_sender_id: Option<String>,
    quoted_text_preview: Option<String>,
    reply_content: Vec<u8>,
) -> anyhow::Result<Message> {
    use uuid::Uuid;
    use crate::domain::message::QuoteContent;
    
    // 构建引用内容
    let quote = QuoteContent {
        quoted_message_id: quoted_message_id.clone(),
        quoted_sender_id: quoted_sender_id.unwrap_or_default(),
        quoted_text_preview: quoted_text_preview.unwrap_or_default(),
        quoted_content: None, // 可选：后续可以从原消息中获取完整内容
    };
    
    let mut builder = MessageBuilder::new()
        .with_server_id(Uuid::new_v4().to_string())
        .with_client_msg_id(Uuid::new_v4().to_string())
        .with_sender_id(sender_id)
        .with_message_type(MessageType::Text)
        .with_content(reply_content)
        .with_quote(quote);
    
    // 如果提供了 conversation_id，则设置
    if let Some(conv_id) = conversation_id {
        builder = builder.with_conversation_id(conv_id);
    }
    
    builder.build()
}
