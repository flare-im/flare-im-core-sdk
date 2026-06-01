//! 消息构建服务：封装 `ContentBuilder + MessageBuilder` 组合，产出 `IMMessage`。

use crate::error::{ErrorCode, FlareError, Result};
use crate::model::content_builder::{BuiltContent, ContentBuilder, DEFAULT_STICKER_DISPLAY_SIDE};
use crate::model::message::IMMessage;
use crate::model::message_builder::MessageBuilder;
use crate::model::message_elem::{elem_plain_summary, elem_to_message_content};
use crate::util::date::ms_to_prost_timestamp;
use flare_proto::common::{ForwardItem, ForwardMode, ImageInfo};
use std::collections::HashMap;

pub struct MessageBuilderService;

#[derive(Clone, Debug)]
pub struct BuildLocationRequest {
    pub conversation_id: String,
    pub sender_id: String,
    pub longitude: f64,
    pub latitude: f64,
    pub address: String,
    pub title: String,
    pub zoom: Option<u8>,
    pub snapshot_url: Option<String>,
    pub snapshot_local_path: Option<String>,
    pub channel_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct BuildCardRequest {
    pub conversation_id: String,
    pub sender_id: String,
    pub id: String,
    pub card_type: String,
    pub title: String,
    pub subtitle: String,
    pub avatar: String,
    pub channel_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct BuildStickerRequest {
    pub conversation_id: String,
    pub sender_id: String,
    pub sticker_id: String,
    pub channel_id: Option<String>,
    pub package_id: Option<String>,
    pub url: Option<String>,
    pub width: i32,
    pub height: i32,
    pub sticker_format: Option<String>,
}

#[derive(Clone, Debug)]
pub struct BuildLinkCardRequest {
    pub conversation_id: String,
    pub sender_id: String,
    pub url: String,
    pub channel_id: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub thumbnail_url: Option<String>,
    pub site_name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct BuildMiniProgramRequest {
    pub conversation_id: String,
    pub sender_id: String,
    pub app_id: String,
    pub channel_id: Option<String>,
    pub title: Option<String>,
    pub page_path: Option<String>,
    pub thumbnail_url: Option<String>,
    pub extra: Option<HashMap<String, String>>,
}

#[derive(Clone, Debug)]
pub struct BuildRichDocRequest {
    pub conversation_id: String,
    pub sender_id: String,
    pub doc_json: String,
    pub content_schema: String,
    pub plain_text: String,
    pub channel_id: Option<String>,
    pub input_format: Option<String>,
    pub input_format_version: Option<i32>,
    pub source_payload: Option<HashMap<String, String>>,
    pub title: Option<String>,
    pub search_text: Option<String>,
    pub render_hints_json: Option<String>,
}

#[derive(Clone, Debug)]
pub struct BuildScheduleRequest {
    pub conversation_id: String,
    pub sender_id: String,
    pub schedule_id: String,
    pub title: String,
    pub channel_id: Option<String>,
    pub start_time_ms: i64,
    pub end_time_ms: i64,
    pub participant_user_ids: Option<Vec<String>>,
}

impl MessageBuilderService {
    pub fn build_text(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        text: impl AsRef<str>,
        channel_id: Option<&str>,
        mention_all: bool,
    ) -> Result<IMMessage> {
        let text_str = text.as_ref();
        let mut content_builder = ContentBuilder::text(text_str);
        if mention_all {
            let len = text_str.chars().count().min(i32::MAX as usize) as i32;
            content_builder = content_builder.mention_all(0, len.max(1));
        }
        let content = content_builder.build();
        let mut builder = MessageBuilder::new(conversation_id, sender_id).content(content);
        if mention_all {
            builder = builder.extra("mention_all", "true");
        }
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
        if let Some(sender_id) = quoted_sender_id
            && !sender_id.trim().is_empty()
        {
            quote_builder = quote_builder.quoted_sender_id(sender_id.trim());
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
        let items: Result<Vec<ForwardItem>> =
            sources.iter().map(forward_item_from_source).collect();
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

    pub fn build_location(request: BuildLocationRequest) -> Result<IMMessage> {
        Self::build_with_content(
            request.conversation_id,
            request.sender_id,
            ContentBuilder::location(request.latitude, request.longitude)
                .address(request.address)
                .title(request.title)
                .location_zoom(request.zoom)
                .location_snapshot_url(request.snapshot_url)
                .location_snapshot_local_path(request.snapshot_local_path)
                .build(),
            request.channel_id.as_deref(),
        )
    }

    pub fn build_card(request: BuildCardRequest) -> Result<IMMessage> {
        Self::build_with_content(
            request.conversation_id,
            request.sender_id,
            ContentBuilder::card(request.id)
                .card_type(request.card_type)
                .title(request.title)
                .subtitle(request.subtitle)
                .avatar(request.avatar)
                .build(),
            request.channel_id.as_deref(),
        )
    }

    pub fn build_sticker(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        sticker_id: impl Into<String>,
        channel_id: Option<&str>,
    ) -> Result<IMMessage> {
        Self::build_sticker_with(BuildStickerRequest {
            conversation_id: conversation_id.into(),
            sender_id: sender_id.into(),
            sticker_id: sticker_id.into(),
            channel_id: channel_id.map(ToOwned::to_owned),
            package_id: None,
            url: None,
            width: 0,
            height: 0,
            sticker_format: None,
        })
    }

    /// 构建贴纸消息（`package_id` / `format` 与 proto `StickerContent` 一致）
    pub fn build_sticker_with(request: BuildStickerRequest) -> Result<IMMessage> {
        let mut b = ContentBuilder::sticker(request.sticker_id);
        if let Some(p) = request
            .package_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            b = b.package_id(p);
        }
        if let Some(u) = request.url.as_deref().filter(|s| !s.trim().is_empty()) {
            b = b.url(u);
        }
        let w = if request.width > 0 {
            request.width
        } else {
            DEFAULT_STICKER_DISPLAY_SIDE
        };
        let h = if request.height > 0 {
            request.height
        } else {
            DEFAULT_STICKER_DISPLAY_SIDE
        };
        b = b.size(w, h);
        if let Some(f) = request
            .sticker_format
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            b = b.sticker_format(f);
        }
        Self::build_with_content(
            request.conversation_id,
            request.sender_id,
            b.build(),
            request.channel_id.as_deref(),
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

    pub fn build_link_card(request: BuildLinkCardRequest) -> Result<IMMessage> {
        let mut b = ContentBuilder::link_card(request.url);
        if let Some(t) = request
            .title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            b = b.title(t);
        }
        if let Some(d) = request
            .description
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            b = b.description(d);
        }
        if let Some(u) = request
            .thumbnail_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            b = b.link_card_thumbnail_url(u);
        }
        if let Some(s) = request
            .site_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            b = b.link_card_site_name(s);
        }
        Self::build_with_content(
            request.conversation_id,
            request.sender_id,
            b.build(),
            request.channel_id.as_deref(),
        )
    }

    pub fn build_mini_program(request: BuildMiniProgramRequest) -> Result<IMMessage> {
        let mut b = ContentBuilder::mini_program(request.app_id);
        if let Some(t) = request
            .title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            b = b.title(t);
        }
        if let Some(p) = request
            .page_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            b = b.page_path(p);
        }
        if let Some(u) = request
            .thumbnail_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            b = b.mini_program_thumbnail_url(u);
        }
        if let Some(e) = request.extra.filter(|m| !m.is_empty()) {
            b = b.mini_program_extend_extra(e);
        }
        Self::build_with_content(
            request.conversation_id,
            request.sender_id,
            b.build(),
            request.channel_id.as_deref(),
        )
    }

    /// 富文本（Rich Doc 主存储）。`content_schema == rich_doc` 时校验 `doc_json` 根结构。
    pub fn build_rich_doc(request: BuildRichDocRequest) -> Result<IMMessage> {
        let mut cb = ContentBuilder::try_rich_doc(
            request.doc_json,
            request.content_schema,
            request.plain_text,
        )
        .map_err(|e| {
            FlareError::localized(
                ErrorCode::InvalidParameter,
                format!("sdk.message.rich_doc_v2.invalid: {e}"),
            )
        })?;
        if let Some(f) = request.input_format {
            cb = cb.rich_text_input_format(f);
        }
        if let Some(v) = request.input_format_version {
            cb = cb.rich_text_input_format_version(v);
        }
        if let Some(map) = request.source_payload {
            for (k, v) in map {
                cb = cb.rich_text_source_payload_entry(k, v);
            }
        }
        cb = cb.rich_text_title(request.title);
        cb = cb.rich_text_search_text(request.search_text);
        cb = cb.rich_text_render_hints_json(request.render_hints_json);
        Self::build_with_content(
            request.conversation_id,
            request.sender_id,
            cb.build(),
            request.channel_id.as_deref(),
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

    pub fn build_schedule(request: BuildScheduleRequest) -> Result<IMMessage> {
        let mut b = ContentBuilder::schedule(request.schedule_id, request.title)
            .schedule_times_ms(request.start_time_ms, request.end_time_ms);
        if let Some(p) = request.participant_user_ids.filter(|v| !v.is_empty()) {
            b = b.schedule_participant_user_ids(p);
        }
        Self::build_with_content(
            request.conversation_id,
            request.sender_id,
            b.build(),
            request.channel_id.as_deref(),
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
