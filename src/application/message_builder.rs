//! 消息构建服务：封装 `ContentBuilder + MessageBuilder` 组合，产出 `IMMessage`。

use crate::error::{ErrorCode, FlareError, Result};
use crate::model::content_builder::{BuiltContent, ContentBuilder};
use crate::model::message::IMMessage;
use crate::model::message_builder::MessageBuilder;

pub struct MessageBuilderService;

impl MessageBuilderService {
    pub fn build_text(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        text: impl AsRef<str>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        let content = ContentBuilder::text(text.as_ref()).build();
        let mut builder = MessageBuilder::new(conversation_id, sender_id).content(content);
        if let Some(channel_id) = channel_id {
            builder = builder.channel(channel_id).single_chat();
        }
        Ok(IMMessage::new(builder.build()?))
    }

    pub fn build_quote(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        quoted_message_id: impl Into<String>,
        text: impl AsRef<str>,
        quoted_sender_id: Option<&str>,
        quoted_text_preview: Option<&str>,
        quoted_content: Option<BuiltContent>,
    ) -> Result<IMMessage> {
        let quoted_message_id = quoted_message_id.into();
        if quoted_message_id.trim().is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "sdk.message.quote.invalid_quoted_message_id",
            ));
        }
        let Some(quoted_content) = quoted_content else {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "sdk.message.quote.missing_quoted_content",
            ));
        };

        let quote_preview = quoted_text_preview.unwrap_or("").to_string();
        let current = ContentBuilder::text(text.as_ref()).build();
        let mut quote_builder = ContentBuilder::quote(quoted_message_id.clone())
            .quoted_text_preview(&quote_preview)
            .current(current);
        if let Some(sender_id) = quoted_sender_id {
            if !sender_id.trim().is_empty() {
                quote_builder = quote_builder.quoted_sender_id(sender_id.trim());
            }
        }
        quote_builder = quote_builder.quoted(quoted_content);
        let content = quote_builder.build();
        let message = MessageBuilder::new(conversation_id, sender_id)
            .content(content)
            .extra("content_text", text.as_ref())
            .build()?;
        Ok(IMMessage::new(message))
    }

    pub fn build_thread_reply(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        thread_id: impl Into<String>,
        text: impl AsRef<str>,
    ) -> Result<IMMessage> {
        let content = ContentBuilder::thread(thread_id).build();
        let message = MessageBuilder::new(conversation_id, sender_id)
            .content(content)
            .extra("content_text", text.as_ref())
            .build()?;
        Ok(IMMessage::new(message))
    }

    pub fn build_forward(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        message_ids: Vec<String>,
    ) -> Result<IMMessage> {
        let content = ContentBuilder::forward(message_ids).build();
        let message = MessageBuilder::new(conversation_id, sender_id)
            .content(content)
            .build()?;
        Ok(IMMessage::new(message))
    }

    pub fn build_with_content(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        content: BuiltContent,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        let mut builder = MessageBuilder::new(conversation_id, sender_id).content(content);
        if let Some(channel_id) = channel_id {
            builder = builder.channel(channel_id).single_chat();
        }
        Ok(IMMessage::new(builder.build()?))
    }

    pub fn build_image(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        image_id: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::image(image_id).build(),
            channel_id,
        )
    }

    /// 原图与缩略图使用不同本地路径或 file_id 时，发送阶段会分别上传。
    pub fn build_image_with_thumbnail(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        source_media_id: impl Into<String>,
        thumbnail_media_id: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::image_with_thumbnail(source_media_id, thumbnail_media_id).build(),
            channel_id,
        )
    }

    pub fn build_video(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        video_id: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::video(video_id).build(),
            channel_id,
        )
    }

    pub fn build_audio(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        audio_id: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::audio(audio_id).build(),
            channel_id,
        )
    }

    pub fn build_file(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        file_id: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::file(file_id).build(),
            channel_id,
        )
    }

    pub fn build_location(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        longitude: f64,
        latitude: f64,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::location(longitude, latitude).build(),
            channel_id,
        )
    }

    pub fn build_card(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        user_id: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::card(user_id).build(),
            channel_id,
        )
    }

    pub fn build_sticker(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        sticker_id: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::sticker(sticker_id).build(),
            channel_id,
        )
    }

    pub fn build_emoji(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        emoji: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::emoji(emoji).build(),
            channel_id,
        )
    }

    pub fn build_gif(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        gif_id: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::gif(gif_id).build(),
            channel_id,
        )
    }

    pub fn build_link_card(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        url: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::link_card(url).build(),
            channel_id,
        )
    }

    pub fn build_mini_program(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        app_id: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::mini_program(app_id).build(),
            channel_id,
        )
    }

    pub fn build_rich_text(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        body: impl Into<String>,
        format: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::rich_text(body, format).build(),
            channel_id,
        )
    }

    pub fn build_markdown(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        text: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::markdown(text).build(),
            channel_id,
        )
    }

    pub fn build_system(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        event_kind: impl Into<String>,
        body: impl Into<String>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::system(event_kind, body).build(),
            None,
        )
    }

    pub fn build_notification(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::notification(title, body).build(),
            None,
        )
    }

    pub fn build_vote(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        vote_id: impl Into<String>,
        title: impl Into<String>,
        options: Vec<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::vote(vote_id, title, options).build(),
            channel_id,
        )
    }

    pub fn build_task(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        task_id: impl Into<String>,
        title: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::task(task_id, title).build(),
            channel_id,
        )
    }

    pub fn build_schedule(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        schedule_id: impl Into<String>,
        title: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::schedule(schedule_id, title).build(),
            channel_id,
        )
    }

    pub fn build_announcement(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::announcement(title, body).build(),
            None,
        )
    }

    pub fn build_custom(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        r#type: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::custom(r#type).build(),
            channel_id,
        )
    }

    pub fn build_placeholder(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::placeholder(reason).build(),
            None,
        )
    }
}
