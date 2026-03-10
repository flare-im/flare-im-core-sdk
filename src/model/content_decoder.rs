//! 将 Message.content bytes 解码为强类型内容枚举。
//!
//! ```ignore
//! use flare_im_core_sdk::model::content_decoder::{decode_content, DecodedContent};
//!
//! let decoded = decode_content(&message)?;
//! match decoded {
//!     DecodedContent::Text(text) => println!("收到文本: {}", text.text),
//!     DecodedContent::Image(img) => println!("收到图片: {}", img.image_id),
//!     _ => {}
//! }
//! ```

use prost::Message as ProstMessage;

use flare_proto::common::{
    MessageContent, MessageType, message_content,
    TextContent, ImageContent, VideoContent, AudioContent,
    FileContent, LocationContent, CardContent,
    StickerContent, EmojiContent, GifContent,
    QuoteContent, LinkCardContent, ForwardContent,
    ThreadContent, MiniProgramContent,
    RichTextContent, MarkdownContent, ImageGroupContent,
    SystemContent, NotificationContent,
    VoteContent, TaskContent, ScheduleContent, AnnouncementContent,
    CustomContent, PlaceholderContent,
};

use crate::error::{SdkError, Result};
use crate::model::message::Message;

/// 解码后的消息内容
#[derive(Clone, Debug)]
pub enum DecodedContent {
    Text(TextContent),
    Image(ImageContent),
    Video(VideoContent),
    Audio(AudioContent),
    File(FileContent),
    Location(LocationContent),
    Card(CardContent),
    Sticker(StickerContent),
    Emoji(EmojiContent),
    Gif(GifContent),
    Quote(Box<QuoteContent>),
    LinkCard(LinkCardContent),
    Forward(ForwardContent),
    Thread(Box<ThreadContent>),
    MiniProgram(MiniProgramContent),
    RichText(RichTextContent),
    Markdown(MarkdownContent),
    ImageGroup(ImageGroupContent),
    System(SystemContent),
    Notification(NotificationContent),
    Vote(VoteContent),
    Task(TaskContent),
    Schedule(ScheduleContent),
    Announcement(AnnouncementContent),
    Custom(CustomContent),
    Placeholder(PlaceholderContent),
}

/// 从 Message 解码内容 bytes 为强类型
pub fn decode_content(message: &Message) -> Result<DecodedContent> {
    decode_content_bytes(&message.content)
}

/// 从原始 bytes 解码 MessageContent
pub fn decode_content_bytes(bytes: &[u8]) -> Result<DecodedContent> {
    if bytes.is_empty() {
        return Err(SdkError::Codec("empty content bytes".into()));
    }

    let mc = MessageContent::decode(bytes)?;

    match mc.content {
        Some(message_content::Content::Text(c)) => Ok(DecodedContent::Text(c)),
        Some(message_content::Content::Image(c)) => Ok(DecodedContent::Image(c)),
        Some(message_content::Content::Video(c)) => Ok(DecodedContent::Video(c)),
        Some(message_content::Content::Audio(c)) => Ok(DecodedContent::Audio(c)),
        Some(message_content::Content::File(c)) => Ok(DecodedContent::File(c)),
        Some(message_content::Content::Location(c)) => Ok(DecodedContent::Location(c)),
        Some(message_content::Content::Card(c)) => Ok(DecodedContent::Card(c)),
        Some(message_content::Content::Sticker(c)) => Ok(DecodedContent::Sticker(c)),
        Some(message_content::Content::Emoji(c)) => Ok(DecodedContent::Emoji(c)),
        Some(message_content::Content::Gif(c)) => Ok(DecodedContent::Gif(c)),
        Some(message_content::Content::Quote(c)) => Ok(DecodedContent::Quote(c)),
        Some(message_content::Content::LinkCard(c)) => Ok(DecodedContent::LinkCard(c)),
        Some(message_content::Content::Forward(c)) => Ok(DecodedContent::Forward(c)),
        Some(message_content::Content::Thread(c)) => Ok(DecodedContent::Thread(c)),
        Some(message_content::Content::MiniProgram(c)) => Ok(DecodedContent::MiniProgram(c)),
        Some(message_content::Content::RichText(c)) => Ok(DecodedContent::RichText(c)),
        Some(message_content::Content::Markdown(c)) => Ok(DecodedContent::Markdown(c)),
        Some(message_content::Content::ImageGroup(c)) => Ok(DecodedContent::ImageGroup(c)),
        Some(message_content::Content::System(c)) => Ok(DecodedContent::System(c)),
        Some(message_content::Content::Notification(c)) => Ok(DecodedContent::Notification(c)),
        Some(message_content::Content::Vote(c)) => Ok(DecodedContent::Vote(c)),
        Some(message_content::Content::Task(c)) => Ok(DecodedContent::Task(c)),
        Some(message_content::Content::Schedule(c)) => Ok(DecodedContent::Schedule(c)),
        Some(message_content::Content::Announcement(c)) => Ok(DecodedContent::Announcement(c)),
        Some(message_content::Content::Custom(c)) => Ok(DecodedContent::Custom(c)),
        Some(message_content::Content::Placeholder(c)) => Ok(DecodedContent::Placeholder(c)),
        None => Err(SdkError::Codec("MessageContent.content is None".into())),
    }
}

/// 从 MessageContent 提取内容（已解码的 MessageContent 对象）
pub fn extract_content(mc: &MessageContent) -> Result<DecodedContent> {
    match &mc.content {
        Some(message_content::Content::Text(c)) => Ok(DecodedContent::Text(c.clone())),
        Some(message_content::Content::Image(c)) => Ok(DecodedContent::Image(c.clone())),
        Some(message_content::Content::Video(c)) => Ok(DecodedContent::Video(c.clone())),
        Some(message_content::Content::Audio(c)) => Ok(DecodedContent::Audio(c.clone())),
        Some(message_content::Content::File(c)) => Ok(DecodedContent::File(c.clone())),
        Some(message_content::Content::Location(c)) => Ok(DecodedContent::Location(c.clone())),
        Some(message_content::Content::Card(c)) => Ok(DecodedContent::Card(c.clone())),
        Some(message_content::Content::Sticker(c)) => Ok(DecodedContent::Sticker(c.clone())),
        Some(message_content::Content::Emoji(c)) => Ok(DecodedContent::Emoji(c.clone())),
        Some(message_content::Content::Gif(c)) => Ok(DecodedContent::Gif(c.clone())),
        Some(message_content::Content::Quote(c)) => Ok(DecodedContent::Quote(c.clone())),
        Some(message_content::Content::LinkCard(c)) => Ok(DecodedContent::LinkCard(c.clone())),
        Some(message_content::Content::Forward(c)) => Ok(DecodedContent::Forward(c.clone())),
        Some(message_content::Content::Thread(c)) => Ok(DecodedContent::Thread(c.clone())),
        Some(message_content::Content::MiniProgram(c)) => Ok(DecodedContent::MiniProgram(c.clone())),
        Some(message_content::Content::RichText(c)) => Ok(DecodedContent::RichText(c.clone())),
        Some(message_content::Content::Markdown(c)) => Ok(DecodedContent::Markdown(c.clone())),
        Some(message_content::Content::ImageGroup(c)) => Ok(DecodedContent::ImageGroup(c.clone())),
        Some(message_content::Content::System(c)) => Ok(DecodedContent::System(c.clone())),
        Some(message_content::Content::Notification(c)) => Ok(DecodedContent::Notification(c.clone())),
        Some(message_content::Content::Vote(c)) => Ok(DecodedContent::Vote(c.clone())),
        Some(message_content::Content::Task(c)) => Ok(DecodedContent::Task(c.clone())),
        Some(message_content::Content::Schedule(c)) => Ok(DecodedContent::Schedule(c.clone())),
        Some(message_content::Content::Announcement(c)) => Ok(DecodedContent::Announcement(c.clone())),
        Some(message_content::Content::Custom(c)) => Ok(DecodedContent::Custom(c.clone())),
        Some(message_content::Content::Placeholder(c)) => Ok(DecodedContent::Placeholder(c.clone())),
        None => Err(SdkError::Codec("MessageContent.content is None".into())),
    }
}

impl DecodedContent {
    /// 获取与此内容对应的 MessageType
    pub fn message_type(&self) -> MessageType {
        match self {
            Self::Text(_) => MessageType::Text,
            Self::Image(_) => MessageType::Image,
            Self::Video(_) => MessageType::Video,
            Self::Audio(_) => MessageType::Audio,
            Self::File(_) => MessageType::File,
            Self::Location(_) => MessageType::Location,
            Self::Card(_) => MessageType::Card,
            Self::Sticker(_) => MessageType::Sticker,
            Self::Emoji(_) => MessageType::Emoji,
            Self::Gif(_) => MessageType::Gif,
            Self::Quote(_) => MessageType::Quote,
            Self::LinkCard(_) => MessageType::LinkCard,
            Self::Forward(_) => MessageType::MergeForward,
            Self::Thread(_) => MessageType::Thread,
            Self::MiniProgram(_) => MessageType::MiniProgram,
            Self::RichText(_) => MessageType::RichText,
            Self::Markdown(_) => MessageType::Markdown,
            Self::ImageGroup(_) => MessageType::ImageGroup,
            Self::System(_) => MessageType::System,
            Self::Notification(_) => MessageType::Notification,
            Self::Vote(_) => MessageType::Poll,
            Self::Task(_) => MessageType::Task,
            Self::Schedule(_) => MessageType::Schedule,
            Self::Announcement(_) => MessageType::Announcement,
            Self::Custom(_) => MessageType::Custom,
            Self::Placeholder(_) => MessageType::E2ePlaceholder,
        }
    }

    /// 提取文本摘要（用于会话列表/搜索等场景）
    pub fn text_preview(&self) -> String {
        match self {
            Self::Text(c) => c.text.clone(),
            Self::Image(_) => "[图片]".into(),
            Self::Video(_) => "[视频]".into(),
            Self::Audio(_) => "[语音]".into(),
            Self::File(c) => format!("[文件] {}", c.file_name),
            Self::Location(c) => format!("[位置] {}", c.address),
            Self::Card(c) => format!("[名片] {}", c.nickname),
            Self::Sticker(_) => "[贴纸]".into(),
            Self::Emoji(c) => c.emoji.clone(),
            Self::Gif(_) => "[动图]".into(),
            Self::Quote(c) => format!("[引用] {}", c.quoted_text_preview),
            Self::LinkCard(c) => format!("[链接] {}", c.title),
            Self::Forward(c) => format!("[合并转发] {}", c.forward_reason),
            Self::Thread(c) => format!("[话题] {}", c.thread_title),
            Self::MiniProgram(c) => format!("[小程序] {}", c.title),
            Self::RichText(_) => "[富文本]".into(),
            Self::Markdown(c) => c.text.chars().take(50).collect(),
            Self::ImageGroup(_) => "[图组]".into(),
            Self::System(c) => c.body.clone(),
            Self::Notification(c) => c.body.clone(),
            Self::Vote(c) => format!("[投票] {}", c.title),
            Self::Task(c) => format!("[任务] {}", c.title),
            Self::Schedule(c) => format!("[日程] {}", c.title),
            Self::Announcement(c) => format!("[公告] {}", c.title),
            Self::Custom(c) => c.description.clone(),
            Self::Placeholder(c) => c.fallback_text.clone(),
        }
    }
}
