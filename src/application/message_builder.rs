//! 消息构建服务：封装 `ContentBuilder + MessageBuilder` 组合，产出 `IMMessage`。

use crate::error::{ErrorCode, FlareError, Result};
use crate::model::content_builder::{BuiltContent, ContentBuilder, DEFAULT_STICKER_DISPLAY_SIDE};
use crate::model::message::IMMessage;
use crate::model::message_builder::MessageBuilder;
use crate::model::message_elem::{elem_plain_summary, elem_to_message_content};
use crate::util::date::ms_to_prost_timestamp;
use flare_proto::common::{ForwardItem, ForwardMode, ImageInfo};

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
            .extra("contentText", text.as_ref())
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
            .extra("contentText", text.as_ref())
            .build()?;
        Ok(IMMessage::new(message))
    }

    /// `merge == false` 时必须恰好一条源消息（单条转发，`ForwardMode::Single`）。
    pub fn build_forward(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        merge: bool,
        title: impl Into<String>,
        sources: &[IMMessage],
    ) -> Result<IMMessage> {
        if sources.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "sdk.message.forward.empty_sources",
            ));
        }
        let mode = if merge {
            ForwardMode::Merged
        } else if sources.len() == 1 {
            ForwardMode::Single
        } else {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "sdk.message.forward.single_requires_one",
            ));
        };
        let items: Result<Vec<ForwardItem>> = sources.iter().map(forward_item_from_source).collect();
        let items = items?;
        let title_str = title.into();
        let title_opt = if title_str.trim().is_empty() {
            None
        } else {
            Some(title_str)
        };
        let content = ContentBuilder::forward(mode, title_opt, items).build();
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

    pub fn build_image_group(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        images: Vec<ImageInfo>,
        description: impl Into<String>,
        metadata: std::collections::HashMap<String, String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        if images.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "sdk.message.image_group.empty_images",
            ));
        }
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::image_group_with_details(images, description, metadata).build(),
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
        address: impl Into<String>,
        title: impl Into<String>,
        zoom: Option<u8>,
        snapshot_url: Option<String>,
        snapshot_local_path: Option<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::location(latitude, longitude)
                .address(address)
                .title(title)
                .location_zoom(zoom)
                .location_snapshot_url(snapshot_url)
                .location_snapshot_local_path(snapshot_local_path)
                .build(),
            channel_id,
        )
    }

    pub fn build_card(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        id: impl Into<String>,
        card_type: impl Into<String>,
        title: impl Into<String>,
        subtitle: impl Into<String>,
        avatar: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_with_content(
            conversation_id,
            sender_id,
            ContentBuilder::card(id)
                .card_type(card_type)
                .title(title)
                .subtitle(subtitle)
                .avatar(avatar)
                .build(),
            channel_id,
        )
    }

    pub fn build_sticker(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        sticker_id: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_sticker_with(
            conversation_id,
            sender_id,
            sticker_id,
            channel_id,
            None,
            None,
            0,
            0,
            None,
        )
    }

    /// 构建贴纸消息（`package_id` / `format` 与 proto `StickerContent` 一致）
    pub fn build_sticker_with(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        sticker_id: impl Into<String>,
        channel_id: Option<&str>,
        package_id: Option<&str>,
        url: Option<&str>,
        width: i32,
        height: i32,
        sticker_format: Option<&str>,
    ) -> Result<IMMessage> {
        let mut b = ContentBuilder::sticker(sticker_id);
        if let Some(p) = package_id.filter(|s| !s.trim().is_empty()) {
            b = b.package_id(p);
        }
        if let Some(u) = url.filter(|s| !s.trim().is_empty()) {
            b = b.url(u);
        }
        let w = if width > 0 {
            width
        } else {
            DEFAULT_STICKER_DISPLAY_SIDE
        };
        let h = if height > 0 {
            height
        } else {
            DEFAULT_STICKER_DISPLAY_SIDE
        };
        b = b.size(w, h);
        if let Some(f) = sticker_format.filter(|s| !s.trim().is_empty()) {
            b = b.sticker_format(f);
        }
        Self::build_with_content(conversation_id, sender_id, b.build(), channel_id)
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

    pub fn build_link_card(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        url: impl Into<String>,
        channel_id: Option<&str>,
        title: Option<&str>,
        description: Option<&str>,
        thumbnail_url: Option<&str>,
        site_name: Option<&str>,
    ) -> Result<IMMessage> {
        let mut b = ContentBuilder::link_card(url);
        if let Some(t) = title.map(str::trim).filter(|s| !s.is_empty()) {
            b = b.title(t);
        }
        if let Some(d) = description.map(str::trim).filter(|s| !s.is_empty()) {
            b = b.description(d);
        }
        if let Some(u) = thumbnail_url.map(str::trim).filter(|s| !s.is_empty()) {
            b = b.link_card_thumbnail_url(u);
        }
        if let Some(s) = site_name.map(str::trim).filter(|s| !s.is_empty()) {
            b = b.link_card_site_name(s);
        }
        Self::build_with_content(conversation_id, sender_id, b.build(), channel_id)
    }

    pub fn build_mini_program(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        app_id: impl Into<String>,
        channel_id: Option<&str>,
        title: Option<&str>,
        page_path: Option<&str>,
        thumbnail_url: Option<&str>,
        extra: Option<std::collections::HashMap<String, String>>,
    ) -> Result<IMMessage> {
        let mut b = ContentBuilder::mini_program(app_id);
        if let Some(t) = title.map(str::trim).filter(|s| !s.is_empty()) {
            b = b.title(t);
        }
        if let Some(p) = page_path.map(str::trim).filter(|s| !s.is_empty()) {
            b = b.page_path(p);
        }
        if let Some(u) = thumbnail_url.map(str::trim).filter(|s| !s.is_empty()) {
            b = b.mini_program_thumbnail_url(u);
        }
        if let Some(e) = extra.filter(|m| !m.is_empty()) {
            b = b.mini_program_extend_extra(e);
        }
        Self::build_with_content(conversation_id, sender_id, b.build(), channel_id)
    }

    /// 富文本（Rich Doc 主存储）。`content_schema == rich_doc` 时校验 `doc_json` 根结构。
    pub fn build_rich_doc(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        doc_json: impl Into<String>,
        content_schema: impl Into<String>,
        plain_text: impl Into<String>,
        channel_id: Option<&str>,
        input_format: Option<&str>,
        input_format_version: Option<i32>,
        source_payload: Option<std::collections::HashMap<String, String>>,
        title: Option<&str>,
        search_text: Option<&str>,
        render_hints_json: Option<&str>,
    ) -> Result<IMMessage> {
        let mut cb = ContentBuilder::try_rich_doc(doc_json, content_schema, plain_text).map_err(
            |e| {
                FlareError::localized(
                    ErrorCode::InvalidParameter,
                    format!("sdk.message.rich_doc_v2.invalid: {e}"),
                )
            },
        )?;
        if let Some(f) = input_format {
            cb = cb.rich_text_input_format(f);
        }
        if let Some(v) = input_format_version {
            cb = cb.rich_text_input_format_version(v);
        }
        if let Some(map) = source_payload {
            for (k, v) in map {
                cb = cb.rich_text_source_payload_entry(k, v);
            }
        }
        cb = cb.rich_text_title(title.map(|s| s.to_string()));
        cb = cb.rich_text_search_text(search_text.map(|s| s.to_string()));
        cb = cb.rich_text_render_hints_json(render_hints_json.map(|s| s.to_string()));
        Self::build_with_content(conversation_id, sender_id, cb.build(), channel_id)
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
        participant_user_ids: Option<Vec<String>>,
    ) -> Result<IMMessage> {
        let mut b = ContentBuilder::vote(vote_id, title, options);
        if let Some(p) = participant_user_ids.filter(|v| !v.is_empty()) {
            b = b.vote_participant_user_ids(p);
        }
        Self::build_with_content(conversation_id, sender_id, b.build(), channel_id)
    }

    pub fn build_task(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        task_id: impl Into<String>,
        title: impl Into<String>,
        channel_id: Option<&str>,
        status: Option<&str>,
        participant_user_ids: Option<Vec<String>>,
    ) -> Result<IMMessage> {
        let mut b = ContentBuilder::task(task_id, title);
        if let Some(s) = status.map(str::trim).filter(|s| !s.is_empty()) {
            b = b.status(s);
        }
        if let Some(p) = participant_user_ids.filter(|v| !v.is_empty()) {
            b = b.task_participant_user_ids(p);
        }
        Self::build_with_content(conversation_id, sender_id, b.build(), channel_id)
    }

    pub fn build_schedule(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        schedule_id: impl Into<String>,
        title: impl Into<String>,
        channel_id: Option<&str>,
        start_time_ms: i64,
        end_time_ms: i64,
        participant_user_ids: Option<Vec<String>>,
    ) -> Result<IMMessage> {
        let mut b = ContentBuilder::schedule(schedule_id, title).schedule_times_ms(start_time_ms, end_time_ms);
        if let Some(p) = participant_user_ids.filter(|v| !v.is_empty()) {
            b = b.schedule_participant_user_ids(p);
        }
        Self::build_with_content(conversation_id, sender_id, b.build(), channel_id)
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

fn forward_item_from_source(source: &IMMessage) -> Result<ForwardItem> {
    let elem = source.content.as_ref().ok_or_else(|| {
        FlareError::localized(
            ErrorCode::InvalidParameter,
            "sdk.message.forward.missing_content",
        )
    })?;
    let inner_mc = elem_to_message_content(elem);
    let plain = elem_plain_summary(elem);
    let stable_id = {
        let sid = source.server_id.trim();
        if !sid.is_empty() {
            source.server_id.clone()
        } else {
            source.client_msg_id.clone()
        }
    };
    Ok(ForwardItem {
        source_message_id: Some(stable_id),
        source_conversation_id: Some(source.conversation_id.clone()),
        source_sender_id: Some(source.sender_id.clone()),
        source_sender_name: if source.sender_name.trim().is_empty() {
            None
        } else {
            Some(source.sender_name.clone())
        },
        source_message_time: ms_to_prost_timestamp(source.timestamp),
        message_type: source.message_type,
        plain_text: plain,
        content: Some(inner_mc),
    })
}
