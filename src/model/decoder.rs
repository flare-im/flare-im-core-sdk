//! 消息内容解码与预览 — 直接使用 message_content.proto 的 Content，仅做薄包装与预览

use crate::error::Result;
use crate::model::message::Message;
use flare_proto::MessageContentExt;
use flare_proto::common::MessageType;
use flare_proto::common::message_content::Content as ProtoContent;

/// 解码结果：直接包装 proto Content，或 Unknown（空/解析失败）
#[derive(Clone, Debug)]
pub enum DecodedContent {
    /// 与 message_content.proto 的 MessageContent.content 一一对应
    Content(ProtoContent),
    Unknown,
}

impl DecodedContent {
    /// 列表/搜索用短文案
    pub fn text_preview(&self) -> String {
        match self {
            DecodedContent::Content(c) => content_text_preview(c),
            DecodedContent::Unknown => "[未知]".to_string(),
        }
    }

    /// 对应的 MessageType（与 proto MessageType 一致）
    pub fn message_type(&self) -> MessageType {
        match self {
            DecodedContent::Content(c) => content_message_type(c),
            DecodedContent::Unknown => MessageType::Unspecified,
        }
    }

    /// 取得内部 proto Content，便于按类型细粒度访问字段
    pub fn as_content(&self) -> Option<&ProtoContent> {
        match self {
            DecodedContent::Content(c) => Some(c),
            DecodedContent::Unknown => None,
        }
    }
}

fn content_text_preview(c: &ProtoContent) -> String {
    use ProtoContent as C;
    match c {
        C::Text(t) => t.text.clone(),
        C::Image(i) => non_empty(&i.description).unwrap_or_else(|| "[图片]".to_string()),
        C::Video(v) => non_empty(&v.description).unwrap_or_else(|| "[视频]".to_string()),
        C::Audio(a) => non_empty(&a.description).unwrap_or_else(|| "[语音]".to_string()),
        C::File(f) => {
            if f.file_name.is_empty() {
                "[文件]".to_string()
            } else {
                format!("[文件] {}", f.file_name)
            }
        }
        C::Location(l) => non_empty(&l.address)
            .map(|s| format!("[位置] {}", s))
            .unwrap_or_else(|| "[位置]".to_string()),
        C::Card(card) => non_empty(&card.nickname)
            .map(|s| format!("[名片] {}", s))
            .unwrap_or_else(|| "[名片]".to_string()),
        C::Sticker(_) => "[贴纸]".to_string(),
        C::Emoji(e) => e.emoji.clone(),
        C::Gif(_) => "[动图]".to_string(),
        C::Quote(q) => q
            .current_content
            .as_deref()
            .and_then(|mc| mc.content.as_ref())
            .map(content_text_preview)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| q.quoted_text_preview.clone()),
        C::LinkCard(l) => non_empty(&l.title).unwrap_or_else(|| "[链接]".to_string()),
        C::Forward(f) => format!("[转发] {} 条消息", f.message_ids.len()),
        C::Thread(t) => t.thread_title.clone(),
        C::MiniProgram(m) => non_empty(&m.title).unwrap_or_else(|| "[小程序]".to_string()),
        C::RichText(r) => non_empty(&r.content).unwrap_or_else(|| "[富文本]".to_string()),
        C::Markdown(m) => non_empty(&m.text).unwrap_or_else(|| "[Markdown]".to_string()),
        C::ImageGroup(_) => "[多图]".to_string(),
        C::System(s) => non_empty(&s.body).unwrap_or_else(|| "[系统消息]".to_string()),
        C::Notification(n) => non_empty(&n.body)
            .or(non_empty(&n.title))
            .unwrap_or_else(|| "[通知]".to_string()),
        C::Vote(_) => "[投票]".to_string(),
        C::Task(t) => non_empty(&t.title).unwrap_or_else(|| "[任务]".to_string()),
        C::Schedule(_) => "[日程]".to_string(),
        C::Announcement(a) => non_empty(&a.title).unwrap_or_else(|| "[公告]".to_string()),
        C::Custom(c) => non_empty(&c.description).unwrap_or_else(|| "[自定义]".to_string()),
        C::Placeholder(p) => non_empty(&p.fallback_text).unwrap_or_else(|| "[占位]".to_string()),
    }
}

#[inline]
fn non_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn content_message_type(c: &ProtoContent) -> MessageType {
    use MessageType as M;
    use ProtoContent as C;
    match c {
        C::Text(_) => M::Text,
        C::Image(_) => M::Image,
        C::Video(_) => M::Video,
        C::Audio(_) => M::Audio,
        C::File(_) => M::File,
        C::Location(_) => M::Location,
        C::Card(_) => M::Card,
        C::Sticker(_) => M::Sticker,
        C::Emoji(_) => M::Emoji,
        C::Gif(_) => M::Gif,
        C::Quote(_) => M::Quote,
        C::LinkCard(_) => M::LinkCard,
        C::Forward(_) => M::MergeForward,
        C::Thread(_) => M::Thread,
        C::MiniProgram(_) => M::MiniProgram,
        C::RichText(_) => M::RichText,
        C::Markdown(_) => M::Markdown,
        C::ImageGroup(_) => M::ImageGroup,
        C::System(_) => M::System,
        C::Notification(_) => M::Notification,
        C::Vote(_) => M::Poll,
        C::Task(_) => M::Task,
        C::Schedule(_) => M::Schedule,
        C::Announcement(_) => M::Announcement,
        C::Custom(_) => M::Custom,
        C::Placeholder(_) => M::E2ePlaceholder,
    }
}

/// 从 Message 解码内容（直接使用 message_content.proto 的 Content）
pub fn decode_content(msg: &Message) -> Result<DecodedContent> {
    decode_content_bytes(&msg.content)
}

/// 解码 `common.Message.content`（与 [message.proto] field 20、[message_content.proto] `MessageContent` 一致）。
///
/// 规范：`Message.content` **仅**为 `MessageContent` 的 protobuf 编码；`Message.message_type` 应与
/// `MessageContent.content` 所指变体一致（展示侧以解码出的 oneof 为准）。
pub fn decode_content_bytes(bytes: &[u8]) -> Result<DecodedContent> {
    if bytes.is_empty() {
        return Ok(DecodedContent::Unknown);
    }
    let mc = flare_proto::common::MessageContent::decode_from_bytes(bytes).map_err(|e| {
        crate::error::FlareError::deserialization_error(format!("decode MessageContent: {}", e))
    })?;
    Ok(match mc.content {
        Some(c) => DecodedContent::Content(c),
        None => DecodedContent::Unknown,
    })
}
