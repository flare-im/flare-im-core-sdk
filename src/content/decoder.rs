//! 消息内容解码与预览 — 直接使用 message_content.proto 的 Content，仅做薄包装与预览

use crate::content::message_elem::{decoded_content_to_elem, elem_plain_summary};
use crate::content::preview_storage;
use crate::model::message::Message;
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
    /// 列表/搜索用短文案：与 [`crate::content::message_elem::elem_plain_summary`] 一致（JSON 载荷，稳定 `k` + `a`）。
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
        C::Forward(_) => M::Forward,
        C::Thread(_) => M::Thread,
        C::AppCard(_) => M::AppCard,
        C::RichText(_) => M::RichText,
        C::ImageGroup(_) => M::ImageGroup,
        C::System(_) => M::System,
        C::Notification(_) => M::Notification,
        C::Custom(_) => M::Custom,
        C::Placeholder(_) => M::Placeholder,
    }
}

/// 从 Message 解码内容（直接使用 message_content.proto 的 Content）
pub fn decode_content(msg: &Message) -> Result<DecodedContent> {
    let Some(content) = msg.content.as_ref() else {
        return Ok(DecodedContent::Unknown);
    };
    Ok(match content.content.clone() {
        Some(c) => DecodedContent::Content(Box::new(c)),
        None => DecodedContent::Unknown,
    })
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
        Some(c) => DecodedContent::Content(Box::new(sanitize_inbound_content(c))),
        None => DecodedContent::Unknown,
    })
}

/// 入站内容的边界校验。
///
/// 消息正文全部来自**其他用户**，必须按敌意输入对待。此前只有出站路径校验
/// （`validate_outbound_message` 只在 send 上调用），入站一路无人拦：
/// 服务端也只按类型取个预览，从不看 `doc_json`。而 Android / iOS / Flutter
/// 的富文本渲染器都是**没有深度上界的递归**——一份深嵌套 doc_json 就能让
/// 收到它的客户端栈溢出，而且消息已落库，每次打开都会再崩一次。
///
/// 这里不丢弃消息（丢弃等于让攻击者能让别人的消息凭空消失），
/// 而是把不合规的富文本降级成纯文本占位：渲染器永远见不到那棵树。
fn sanitize_inbound_content(
    content: flare_proto::common::message_content::Content,
) -> flare_proto::common::message_content::Content {
    use flare_proto::common::message_content::Content;
    let Content::RichText(ref rich) = content else {
        return content;
    };
    if rich.content_schema != crate::content::rich_doc_v2::pipeline::CONTENT_SCHEMA_RICH_DOC {
        return content;
    }
    match crate::content::rich_doc_v2::validate_doc_json(&rich.doc_json) {
        Ok(()) => content,
        Err(error) => {
            tracing::warn!(
                error = %error,
                doc_json_bytes = rich.doc_json.len(),
                "入站富文本未通过校验，降级为纯文本占位"
            );
            Content::Text(flare_proto::common::TextContent {
                text: rich.plain_text.clone(),
                mentions: Vec::new(),
            })
        }
    }
}

#[cfg(test)]
mod inbound_hardening_tests {
    use super::*;
    use prost::Message as _;
    use flare_proto::common::{MessageContent, RichTextContent, message_content::Content};

    fn encoded(content: Content) -> Vec<u8> {
        MessageContent {
            content: Some(content),
            ..Default::default()
        }
        .encode_to_vec()
    }

    fn nested_doc(depth: usize) -> String {
        // {"type":"doc","version":2,"children":[{...嵌套...}]}
        let mut s = String::from(r#"{"type":"doc","version":2,"children":["#);
        for _ in 0..depth {
            s.push_str(r#"{"type":"paragraph","version":2,"children":["#);
        }
        for _ in 0..depth {
            s.push_str("]}");
        }
        s.push_str("]}");
        s
    }

    /// 敌意对端发一份深嵌套富文本：客户端**不能**把它交给渲染器。
    ///
    /// 三端（Android / iOS / Flutter）的富文本渲染都是没有深度上界的递归，
    /// 这棵树递归下去就是栈溢出；而消息已落库，每次打开还会再崩一次。
    #[test]
    fn deeply_nested_inbound_rich_doc_is_downgraded_not_rendered() {
        let hostile = encoded(Content::RichText(RichTextContent {
            doc_json: nested_doc(5_000),
            content_schema: "rich_doc".to_string(),
            plain_text: "看起来人畜无害".to_string(),
            ..Default::default()
        }));

        let decoded = decode_content_bytes(&hostile).expect("解码本身不该失败");
        let DecodedContent::Content(content) = decoded else {
            panic!("应当解出内容");
        };
        match *content {
            Content::Text(text) => {
                assert_eq!(
                    text.text, "看起来人畜无害",
                    "降级后应保留 plain_text，用户仍看得到这条消息说了什么"
                );
            }
            other => panic!("不合规的富文本必须降级成纯文本，实际是 {other:?}"),
        }
    }

    /// 合规的富文本不受影响 —— 别把正常消息也降级了。
    #[test]
    fn well_formed_inbound_rich_doc_passes_through() {
        let ok = encoded(Content::RichText(RichTextContent {
            doc_json: r#"{"type":"doc","version":2,"children":[]}"#.to_string(),
            content_schema: "rich_doc".to_string(),
            plain_text: "hi".to_string(),
            ..Default::default()
        }));
        let decoded = decode_content_bytes(&ok).expect("解码");
        let DecodedContent::Content(content) = decoded else {
            panic!("应当解出内容");
        };
        assert!(
            matches!(*content, Content::RichText(_)),
            "合规富文本必须原样通过"
        );
    }

    /// 非富文本内容不走这条路，零额外开销。
    #[test]
    fn plain_text_content_is_untouched() {
        let ok = encoded(Content::Text(flare_proto::common::TextContent {
            text: "hello".to_string(),
            mentions: Vec::new(),
        }));
        let decoded = decode_content_bytes(&ok).expect("解码");
        let DecodedContent::Content(content) = decoded else {
            panic!("应当解出内容");
        };
        match *content {
            Content::Text(t) => assert_eq!(t.text, "hello"),
            other => panic!("纯文本不该被改动，实际是 {other:?}"),
        }
    }
}
