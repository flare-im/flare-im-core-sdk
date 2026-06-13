use crate::model::decoder::decode_content_bytes;
use crate::model::message::IMMessage;
use crate::model::message_elem::Elem;
use crate::shared::error::{ErrorCode, FlareError, Result};

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
        if let Some(Elem::Quote(quote)) = message.content.as_ref() {
            if quote.quoted_message_id.trim().is_empty() {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "sdk.message.quote.missing_quoted_message_id",
                ));
            }
            if quote.quoted_content.is_none() {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "sdk.message.quote.missing_quoted_content",
                ));
            }
            if quote.current_content.is_none() {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "sdk.message.quote.missing_current_content",
                ));
            }
            if matches!(quote.current_content.as_deref(), Some(Elem::Quote(_))) {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "sdk.message.quote.current_content_quote_not_allowed",
                ));
            }
            return Ok(());
        }

        if message.encoded_content.is_empty() {
            if is_quote_message {
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
        if let Some(flare_proto::common::message_content::Content::Quote(quote)) =
            decoded.as_content()
        {
            if quote.quoted_message_id.trim().is_empty() {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "sdk.message.quote.missing_quoted_message_id",
                ));
            }
            let quoted_ok = quote
                .quoted_content
                .as_ref()
                .and_then(|content| content.content.as_ref())
                .is_some();
            if !quoted_ok {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "sdk.message.quote.missing_quoted_content",
                ));
            }
            let current_content = quote
                .current_content
                .as_ref()
                .and_then(|content| content.content.as_ref());
            if current_content.is_none() {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "sdk.message.quote.missing_current_content",
                ));
            }
            if matches!(
                current_content,
                Some(flare_proto::common::message_content::Content::Quote(_))
            ) {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "sdk.message.quote.current_content_quote_not_allowed",
                ));
            }
            return Ok(());
        }

        if is_quote_message {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "sdk.message.quote.content_type_mismatch",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
