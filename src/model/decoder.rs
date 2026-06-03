//! 消息内容解码与预览 — 直接使用 message_content.proto 的 Content，仅做薄包装与预览

use crate::model::message::Message;
use crate::model::message_elem::{decoded_content_to_elem, elem_plain_summary};
use crate::model::preview_storage;
use crate::shared::error::Result;
use flare_proto::MessageContentExt;
use flare_proto::common::MessageType;
use flare_proto::common::message_content::Content as ProtoContent;

/// 解码结果：直接包装 proto Content，或 Unknown（空/解析失败）
#[derive(Clone, Debug)]
pub enum DecodedContent {
    /// 与 message_content.proto 的 MessageContent.content 一一对应
    Content(Box<ProtoContent>),
    Unknown,
}

impl DecodedContent {
    /// 列表/搜索用短文案：与 [`crate::model::message_elem::elem_plain_summary`] 一致（JSON 载荷，稳定 `k` + `a`）。
    pub fn text_preview(&self) -> String {
        match self {
            DecodedContent::Content(_) => decoded_content_to_elem(self)
                .map(|e| elem_plain_summary(&e))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(preview_storage::unknown_preview_json),
            DecodedContent::Unknown => preview_storage::unknown_preview_json(),
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
            DecodedContent::Content(c) => Some(c.as_ref()),
            DecodedContent::Unknown => None,
        }
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
        C::Quote(_) => M::Quote,
        C::LinkCard(_) => M::LinkCard,
        C::Forward(_) => M::MergeForward,
        C::Thread(_) => M::Thread,
        C::MiniProgram(_) => M::MiniProgram,
        C::RichText(_) => M::RichText,
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
        crate::shared::error::FlareError::deserialization_error(format!(
            "decode MessageContent: {}",
            e
        ))
    })?;
    Ok(match mc.content {
        Some(c) => DecodedContent::Content(Box::new(c)),
        None => DecodedContent::Unknown,
    })
}
