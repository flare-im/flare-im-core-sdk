//! 消息构建处理器 — 使用 [MessageBuilder] 与 [ContentBuilder] 构建消息，返回 [IMMessage]。
//! 构建结果与 [message_elem::Elem] 解码结构一致（IMMessage 内 content 为解码后的 Elem）。

use crate::error::Result;
use crate::model::content_builder::{BuiltContent, ContentBuilder};
use crate::model::message::IMMessage;
use crate::model::message_builder::MessageBuilder;

/// 消息构建处理器：无状态，根据会话 id、发送者与内容规格构建 IMMessage。
/// 所有构建方法均要求传入 conversation_id（会话 id）。
pub struct MessageBuilderHandler;

impl MessageBuilderHandler {
    /// 构建纯文本消息，返回 IMMessage。
    pub fn build_text(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        text: impl AsRef<str>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        let content = ContentBuilder::text(text.as_ref()).build();
        let mut b = MessageBuilder::new(conversation_id, sender_id).content(content);
        if let Some(cid) = channel_id {
            b = b.channel(cid).single_chat();
        }
        let msg = b.build()?;
        Ok(IMMessage::new(msg))
    }

    /// 构建引用消息，返回 IMMessage。
    pub fn build_quote(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        quoted_message_id: impl Into<String>,
        text: impl AsRef<str>,
        quoted_text_preview: Option<&str>,
    ) -> Result<IMMessage> {
        let content = ContentBuilder::quote(quoted_message_id)
            .quoted_text_preview(quoted_text_preview.unwrap_or(""))
            .build();
        let msg = MessageBuilder::new(conversation_id, sender_id)
            .content(content)
            .extra("content_text", text.as_ref())
            .build()?;
        Ok(IMMessage::new(msg))
    }

    /// 构建话题回复，返回 IMMessage。
    pub fn build_thread_reply(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        thread_id: impl Into<String>,
        text: impl AsRef<str>,
    ) -> Result<IMMessage> {
        let content = ContentBuilder::thread(thread_id).build();
        let msg = MessageBuilder::new(conversation_id, sender_id)
            .content(content)
            .extra("content_text", text.as_ref())
            .build()?;
        Ok(IMMessage::new(msg))
    }

    /// 构建合并转发消息，返回 IMMessage。
    pub fn build_forward(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        message_ids: Vec<String>,
    ) -> Result<IMMessage> {
        let content = ContentBuilder::forward(message_ids).build();
        let msg = MessageBuilder::new(conversation_id, sender_id)
            .content(content)
            .build()?;
        Ok(IMMessage::new(msg))
    }

    /// 使用已构建的 BuiltContent 构建消息，返回 IMMessage。
    pub fn build_with_content(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        content: BuiltContent,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        let mut b = MessageBuilder::new(conversation_id, sender_id).content(content);
        if let Some(cid) = channel_id {
            b = b.channel(cid).single_chat();
        }
        let msg = b.build()?;
        Ok(IMMessage::new(msg))
    }

    // ---------- 各类型便捷入口（均委托 ContentBuilder + MessageBuilder，返回 IMMessage）----------

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
