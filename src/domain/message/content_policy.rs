use crate::content::decoder::decode_content_bytes;
use crate::content::message_elem::{Elem, elem_to_message_content};
use crate::content::rich_doc_v2::pipeline::CONTENT_SCHEMA_RICH_DOC;
use crate::content::rich_doc_v2::validate_doc_json;
use crate::content::url_safety::is_safe_remote_url;
use crate::model::message::IMMessage;
use crate::shared::error::{ErrorCode, FlareError, Result};
use flare_proto::common::message_content::Content as ProtoContent;
use flare_proto::common::{AudioInfo, ImageInfo, MessageContent, QuoteContent, VideoInfo};

const MAX_OUTBOUND_ENCODED_CONTENT_BYTES: usize = 1024 * 1024;
const MAX_OUTBOUND_ATTRIBUTES: usize = 64;

pub struct MessageContentPolicy;

impl MessageContentPolicy {
    pub fn validate_outbound_message(&self, message: &IMMessage) -> Result<()> {
        if message.attributes.len() > MAX_OUTBOUND_ATTRIBUTES {
            return Err(FlareError::localized(
                ErrorCode::ResourceExhausted,
                format!("sdk.message.attributes.exceeds_{MAX_OUTBOUND_ATTRIBUTES}"),
            ));
        }

        let is_quote_message =
            message.message_type == flare_proto::common::MessageType::Quote as i32;
        if let Some(elem) = message.content.as_ref() {
            let content = elem_to_message_content(elem);
            validate_content_contract(&content, is_quote_message)?;
        }

        if message.encoded_content.is_empty() {
            if is_quote_message && message.content.is_none() {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "sdk.message.quote.missing_content",
                ));
            }
            return Ok(());
        }

        if message.encoded_content.len() > MAX_OUTBOUND_ENCODED_CONTENT_BYTES {
            return Err(FlareError::localized(
                ErrorCode::ResourceExhausted,
                format!("sdk.message.content.exceeds_{MAX_OUTBOUND_ENCODED_CONTENT_BYTES}_bytes"),
            ));
        }

        let decoded = decode_content_bytes(&message.encoded_content).map_err(|_| {
            FlareError::localized(
                ErrorCode::InvalidParameter,
                "sdk.message.invalid_content_encoding",
            )
        })?;
        validate_proto_content_contract(decoded.as_content(), is_quote_message)
    }
}

fn validate_content_contract(content: &MessageContent, is_quote_message: bool) -> Result<()> {
    validate_proto_content_contract(content.content.as_ref(), is_quote_message)
}

fn validate_proto_content_contract(
    content: Option<&ProtoContent>,
    is_quote_message: bool,
) -> Result<()> {
    match content {
        Some(ProtoContent::Quote(quote)) => validate_quote_content(quote),
        Some(content) => {
            if is_quote_message {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "sdk.message.quote.content_type_mismatch",
                ));
            }
            validate_content_urls(content)
        }
        None => {
            if is_quote_message {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "sdk.message.quote.missing_content",
                ));
            }
            Ok(())
        }
    }
}

fn validate_quote_content(quote: &QuoteContent) -> Result<()> {
    if quote.quoted_message_id.trim().is_empty() {
        return Err(FlareError::localized(
            ErrorCode::InvalidParameter,
            "sdk.message.quote.missing_quoted_message_id",
        ));
    }
    let quoted_content = required_nested_content(
        quote.quoted_content.as_deref(),
        "sdk.message.quote.missing_quoted_content",
    )?;
    let current_content = required_nested_content(
        quote.current_content.as_deref(),
        "sdk.message.quote.missing_current_content",
    )?;
    if matches!(current_content, ProtoContent::Quote(_)) {
        return Err(FlareError::localized(
            ErrorCode::InvalidParameter,
            "sdk.message.quote.current_content_quote_not_allowed",
        ));
    }
    validate_content_urls(quoted_content)?;
    validate_content_urls(current_content)
}

fn required_nested_content<'a>(
    content: Option<&'a MessageContent>,
    message: &'static str,
) -> Result<&'a ProtoContent> {
    content
        .and_then(|content| content.content.as_ref())
        .ok_or_else(|| FlareError::localized(ErrorCode::InvalidParameter, message))
}

fn validate_content_urls(content: &ProtoContent) -> Result<()> {
    match content {
        ProtoContent::Image(image) => {
            validate_image_info_url("sdk.message.image.source.url", image.source.as_ref())?;
            validate_image_info_url("sdk.message.image.thumbnail.url", image.thumbnail.as_ref())
        }
        ProtoContent::Video(video) => {
            validate_video_info_url("sdk.message.video.source.url", video.source.as_ref())?;
            validate_image_info_url("sdk.message.video.cover.url", video.cover.as_ref())
        }
        ProtoContent::Audio(audio) => {
            validate_audio_info_url("sdk.message.audio.source.url", audio.source.as_ref())
        }
        ProtoContent::File(file) => validate_optional_remote_url("sdk.message.file.url", &file.url),
        ProtoContent::Location(location) => validate_optional_remote_url(
            "sdk.message.location.snapshot_url",
            location.snapshot_url.as_deref().unwrap_or_default(),
        ),
        ProtoContent::Card(card) => {
            validate_optional_remote_url("sdk.message.card.avatar", &card.avatar)
        }
        ProtoContent::Sticker(sticker) => {
            validate_optional_remote_url("sdk.message.sticker.url", &sticker.url)
        }
        ProtoContent::Quote(quote) => validate_quote_content(quote),
        ProtoContent::LinkCard(link) => {
            validate_required_remote_url("sdk.message.link_card.url", &link.url)?;
            validate_optional_remote_url("sdk.message.link_card.thumbnail_url", &link.thumbnail_url)
        }
        ProtoContent::Forward(forward) => {
            for item in &forward.items {
                if let Some(content) = item.content.as_ref()
                    && let Some(content) = content.content.as_ref()
                {
                    validate_content_urls(content)?;
                }
            }
            Ok(())
        }
        ProtoContent::Thread(thread) => {
            if let Some(content) = thread.root_content.as_deref()
                && let Some(content) = content.content.as_ref()
            {
                validate_content_urls(content)?;
            }
            Ok(())
        }
        ProtoContent::AppCard(card) => {
            validate_optional_remote_url("sdk.message.app_card.thumbnail_url", &card.thumbnail_url)
        }
        ProtoContent::RichText(rich_text) => {
            if rich_text.content_schema == CONTENT_SCHEMA_RICH_DOC {
                validate_doc_json(&rich_text.doc_json).map_err(|e| {
                    FlareError::localized(
                        ErrorCode::InvalidParameter,
                        format!("sdk.message.rich_doc.invalid_doc_json: {e}"),
                    )
                })?;
            }
            Ok(())
        }
        ProtoContent::ImageGroup(group) => {
            for image in &group.images {
                validate_image_info_url("sdk.message.image_group.image.url", Some(image))?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_image_info_url(field: &'static str, info: Option<&ImageInfo>) -> Result<()> {
    if let Some(info) = info {
        validate_optional_remote_url(field, &info.url)?;
    }
    Ok(())
}

fn validate_video_info_url(field: &'static str, info: Option<&VideoInfo>) -> Result<()> {
    if let Some(info) = info {
        validate_optional_remote_url(field, &info.url)?;
    }
    Ok(())
}

fn validate_audio_info_url(field: &'static str, info: Option<&AudioInfo>) -> Result<()> {
    if let Some(info) = info {
        validate_optional_remote_url(field, &info.url)?;
    }
    Ok(())
}

fn validate_optional_remote_url(field: &'static str, url: &str) -> Result<()> {
    if url.trim().is_empty() {
        return Ok(());
    }
    validate_required_remote_url(field, url)
}

fn validate_required_remote_url(field: &'static str, url: &str) -> Result<()> {
    if is_safe_remote_url(url) {
        return Ok(());
    }
    Err(FlareError::localized(
        ErrorCode::InvalidParameter,
        format!("{field}.invalid_url"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::ContentBuilder;
    use flare_proto::common::Message;

    fn message() -> IMMessage {
        IMMessage::new(Message {
            conversation_id: "conv-1".to_string(),
            server_id: "server-1".to_string(),
            ..Default::default()
        })
    }

    #[test]
    fn rejects_oversized_encoded_content_before_decoding() {
        let mut message = message();
        message.encoded_content = vec![0; 1024 * 1024 + 1];

        let err = MessageContentPolicy
            .validate_outbound_message(&message)
            .expect_err("oversized encoded content must fail");

        assert_eq!(err.code(), Some(ErrorCode::ResourceExhausted));
    }

    #[test]
    fn rejects_too_many_attributes() {
        let mut message = message();
        for index in 0..65 {
            message
                .attributes
                .insert(format!("key-{index}"), "value".to_string());
        }

        let err = MessageContentPolicy
            .validate_outbound_message(&message)
            .expect_err("too many attributes must fail");

        assert_eq!(err.code(), Some(ErrorCode::ResourceExhausted));
    }

    #[test]
    fn rejects_executable_link_card_url() {
        let mut message = message();
        message.encoded_content = ContentBuilder::link_card("javascript:alert(1)")
            .build()
            .encode();

        let err = MessageContentPolicy
            .validate_outbound_message(&message)
            .expect_err("unsafe link card url must fail");

        assert_eq!(err.code(), Some(ErrorCode::InvalidParameter));
    }

    #[test]
    fn accepts_https_link_card_url() {
        let mut message = message();
        message.encoded_content = ContentBuilder::link_card("https://flare.test/post")
            .link_card_thumbnail_url("https://flare.test/thumb.png")
            .build()
            .encode();

        MessageContentPolicy
            .validate_outbound_message(&message)
            .expect("https link card url must pass");
    }

    #[test]
    fn rejects_nested_forward_unsafe_content() {
        let unsafe_content = ContentBuilder::link_card("https://flare.test/post")
            .link_card_thumbnail_url("data:text/html,<script></script>")
            .build()
            .inner;
        let mut message = message();
        message.encoded_content = ContentBuilder::forward(
            flare_proto::common::ForwardMode::Merged,
            None,
            vec![flare_proto::common::ForwardItem {
                content: Some(unsafe_content),
                ..Default::default()
            }],
        )
        .build()
        .encode();

        let err = MessageContentPolicy
            .validate_outbound_message(&message)
            .expect_err("nested unsafe content must fail");

        assert_eq!(err.code(), Some(ErrorCode::InvalidParameter));
    }
}
