use crate::error::{ErrorCode, FlareError, Result};
use crate::model::decoder::{DecodedContent, decode_content_bytes};
use crate::model::message::IMMessage;
use crate::model::message_elem::Elem;

pub struct MessageContentPolicy;

impl MessageContentPolicy {
    pub fn validate_outbound_message(&self, message: &IMMessage) -> Result<()> {
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

        if message.content_bytes.is_empty() {
            if is_quote_message {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "sdk.message.quote.missing_content",
                ));
            }
            return Ok(());
        }

        let decoded = decode_content_bytes(&message.content_bytes).map_err(|_| {
            FlareError::localized(
                ErrorCode::InvalidParameter,
                "sdk.message.invalid_content_encoding",
            )
        })?;
        if let DecodedContent::Content(flare_proto::common::message_content::Content::Quote(
            quote,
        )) = decoded
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
