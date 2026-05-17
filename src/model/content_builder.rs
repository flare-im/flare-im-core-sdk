//! 消息内容构建器 — 与 message_content.proto 对应，构建 MessageContent

use crate::rich_doc_v2::pipeline::CONTENT_SCHEMA_RICH_DOC;
use crate::rich_doc_v2::{RichDocV2Error, validate_doc_json};
use flare_proto::common::{
    AnnouncementContent, AudioContent, AudioInfo, CardContent, CustomContent, EmojiContent,
    FileContent, ForwardContent, ForwardItem, ForwardMode, ImageContent, ImageFormat,
    ImageGroupContent, ImageInfo, LinkCardContent, LocationContent, Mention, MentionType,
    MessageContent, MessageType, MiniProgramContent, NotificationContent, PlaceholderContent,
    QuoteContent, RichTextContent, ScheduleContent, StickerContent, SystemContent, TaskContent,
    TextContent, ThreadContent, VideoContent, VideoInfo, VoteContent,
};

/// 已构建的消息内容（协议层 MessageContent + 类型，便于 encode 与预览）
#[derive(Clone, Debug)]
pub struct BuiltContent {
    pub message_type: MessageType,
    pub inner: MessageContent,
}

impl BuiltContent {
    pub fn new(message_type: MessageType, inner: MessageContent) -> Self {
        Self {
            message_type,
            inner,
        }
    }

    /// 编码为 bytes（写入 Message.content）
    pub fn encode(&self) -> Vec<u8> {
        use prost::Message;
        let mut buf = Vec::with_capacity(self.inner.encoded_len());
        let _ = self.inner.encode(&mut buf);
        buf
    }
}

/// 贴纸消息未指定宽高时，SDK 使用的默认展示边长（正方形占位，与示例客户端一致）。
pub const DEFAULT_STICKER_DISPLAY_SIDE: i32 = 120;

/// 消息内容构建器
#[derive(Clone, Debug, Default)]
pub struct ContentBuilder {
    message_type: MessageType,
    content: Option<MessageContent>,
}

impl ContentBuilder {
    pub fn text(text: impl Into<String>) -> Self {
        let text = text.into();
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Text(
                TextContent {
                    text: text.clone(),
                    mentions: vec![],
                },
            )),
        };
        Self {
            message_type: MessageType::Text,
            content: Some(content),
        }
    }

    pub fn mention_all(mut self, start: i32, length: i32) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Text(ref mut t)) = c.content
            {
                t.mentions.push(Mention {
                    r#type: MentionType::All as i32,
                    user_id: String::new(),
                    user_ids: vec![],
                    role_id: String::new(),
                    start,
                    length,
                });
            }
        }
        self
    }

    pub fn mention_user(mut self, user_id: impl Into<String>, start: i32, length: i32) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Text(ref mut t)) = c.content
            {
                t.mentions.push(Mention {
                    r#type: MentionType::User as i32,
                    user_id: user_id.into(),
                    user_ids: vec![],
                    role_id: String::new(),
                    start,
                    length,
                });
            }
        }
        self
    }

    /// 原图稳定 id（本地路径或已上传 file_id），写入 `source.image_id` / `source.uuid`。
    pub fn image(primary_media_id: impl Into<String>) -> Self {
        let id = primary_media_id.into();
        let source = ImageInfo {
            uuid: id.clone(),
            image_id: id,
            url: String::new(),
            mime_type: String::new(),
            size: 0,
            width: 0,
            height: 0,
            format: ImageFormat::Unspecified as i32,
            animated: false,
        };
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Image(
                ImageContent {
                    source: Some(source),
                    thumbnail: None,
                    description: String::new(),
                    duration_ms: None,
                },
            )),
        };
        Self {
            message_type: MessageType::Image,
            content: Some(content),
        }
    }

    /// 原图 + 缩略图各自稳定 id（可指向不同本地路径，发送时分别上传）。
    pub fn image_with_thumbnail(
        source_media_id: impl Into<String>,
        thumbnail_media_id: impl Into<String>,
    ) -> Self {
        let sid = source_media_id.into();
        let tid = thumbnail_media_id.into();
        let source = ImageInfo {
            uuid: sid.clone(),
            image_id: sid,
            url: String::new(),
            mime_type: String::new(),
            size: 0,
            width: 0,
            height: 0,
            format: ImageFormat::Unspecified as i32,
            animated: false,
        };
        let thumbnail = ImageInfo {
            uuid: tid.clone(),
            image_id: tid,
            url: String::new(),
            mime_type: String::new(),
            size: 0,
            width: 0,
            height: 0,
            format: ImageFormat::Unspecified as i32,
            animated: false,
        };
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Image(
                ImageContent {
                    source: Some(source),
                    thumbnail: Some(thumbnail),
                    description: String::new(),
                    duration_ms: None,
                },
            )),
        };
        Self {
            message_type: MessageType::Image,
            content: Some(content),
        }
    }

    pub fn source(mut self, info: ImageInfo) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Image(ref mut i)) = c.content
            {
                i.source = Some(info);
            }
        }
        self
    }

    pub fn image_thumbnail(mut self, info: ImageInfo) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Image(ref mut i)) = c.content
            {
                i.thumbnail = Some(info);
            }
        }
        self
    }

    pub fn video_source(mut self, info: VideoInfo) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Video(ref mut v)) = c.content
            {
                v.source = Some(info);
            }
        }
        self
    }

    pub fn audio_source(mut self, info: AudioInfo) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Audio(ref mut a)) = c.content
            {
                a.source = Some(info);
            }
        }
        self
    }

    pub fn description(mut self, d: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            match c.content {
                Some(flare_proto::common::message_content::Content::Image(ref mut i)) => {
                    i.description = d.into();
                }
                Some(flare_proto::common::message_content::Content::Video(ref mut v)) => {
                    v.description = d.into();
                }
                Some(flare_proto::common::message_content::Content::Audio(ref mut a)) => {
                    a.description = d.into();
                }
                Some(flare_proto::common::message_content::Content::LinkCard(ref mut l)) => {
                    l.description = d.into();
                }
                Some(flare_proto::common::message_content::Content::Custom(ref mut cu)) => {
                    cu.description = d.into();
                }
                _ => {}
            }
        }
        self
    }

    pub fn video(video_id: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Video(
                VideoContent {
                    video_id: video_id.into(),
                    source: None,
                    cover: None,
                    description: String::new(),
                },
            )),
        };
        Self {
            message_type: MessageType::Video,
            content: Some(content),
        }
    }

    pub fn audio(audio_id: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Audio(
                AudioContent {
                    audio_id: audio_id.into(),
                    source: None,
                    description: String::new(),
                },
            )),
        };
        Self {
            message_type: MessageType::Audio,
            content: Some(content),
        }
    }

    pub fn file(file_id: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::File(
                FileContent {
                    file_id: file_id.into(),
                    file_name: String::new(),
                    mime_type: String::new(),
                    file_size: 0,
                    url: String::new(),
                    description: String::new(),
                },
            )),
        };
        Self {
            message_type: MessageType::File,
            content: Some(content),
        }
    }

    pub fn file_name(mut self, n: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::File(ref mut f)) = c.content
            {
                f.file_name = n.into();
            }
        }
        self
    }

    pub fn mime_type(mut self, m: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::File(ref mut f)) = c.content
            {
                f.mime_type = m.into();
            }
        }
        self
    }

    pub fn file_size(mut self, s: i64) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::File(ref mut f)) = c.content
            {
                f.file_size = s;
            }
        }
        self
    }

    pub fn url(mut self, u: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            match c.content {
                Some(flare_proto::common::message_content::Content::File(ref mut f)) => {
                    f.url = u.into();
                }
                Some(flare_proto::common::message_content::Content::Sticker(ref mut s)) => {
                    s.url = u.into();
                }
                Some(flare_proto::common::message_content::Content::Image(ref mut img)) => {
                    if let Some(ref mut s) = img.source {
                        s.url = u.into();
                    }
                }
                _ => {}
            }
        }
        self
    }

    pub fn location(latitude: f64, longitude: f64) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Location(
                LocationContent {
                    longitude,
                    latitude,
                    address: String::new(),
                    title: String::new(),
                    zoom: None,
                    snapshot_url: None,
                    snapshot_local_path: None,
                },
            )),
        };
        Self {
            message_type: MessageType::Location,
            content: Some(content),
        }
    }

    pub fn address(mut self, a: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Location(ref mut l)) =
                c.content
            {
                l.address = a.into();
            }
        }
        self
    }

    pub fn location_zoom(mut self, zoom: Option<u8>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Location(ref mut l)) =
                c.content
            {
                l.zoom = zoom.map(|z| i32::from(z));
            }
        }
        self
    }

    pub fn location_snapshot_url(mut self, url: Option<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Location(ref mut l)) =
                c.content
            {
                l.snapshot_url = url;
            }
        }
        self
    }

    pub fn location_snapshot_local_path(mut self, path: Option<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Location(ref mut l)) =
                c.content
            {
                l.snapshot_local_path = path;
            }
        }
        self
    }

    pub fn card(id: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Card(
                CardContent {
                    id: id.into(),
                    title: String::new(),
                    avatar: String::new(),
                    subtitle: String::new(),
                    extra: std::collections::HashMap::new(),
                    card_type: String::new(),
                },
            )),
        };
        Self {
            message_type: MessageType::Card,
            content: Some(content),
        }
    }

    pub fn card_type(mut self, t: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Card(ref mut card)) =
                c.content
            {
                card.card_type = t.into();
            }
        }
        self
    }

    pub fn subtitle(mut self, s: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Card(ref mut card)) =
                c.content
            {
                card.subtitle = s.into();
            }
        }
        self
    }

    pub fn avatar(mut self, u: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Card(ref mut card)) =
                c.content
            {
                card.avatar = u.into();
            }
        }
        self
    }

    pub fn sticker(sticker_id: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Sticker(
                StickerContent {
                    sticker_id: sticker_id.into(),
                    url: String::new(),
                    width: DEFAULT_STICKER_DISPLAY_SIDE,
                    height: DEFAULT_STICKER_DISPLAY_SIDE,
                    extra: std::collections::HashMap::new(),
                    package_id: String::new(),
                    format: String::new(),
                },
            )),
        };
        Self {
            message_type: MessageType::Sticker,
            content: Some(content),
        }
    }

    pub fn size(mut self, w: i32, h: i32) -> Self {
        if let Some(ref mut c) = self.content {
            match c.content {
                Some(flare_proto::common::message_content::Content::Sticker(ref mut s)) => {
                    s.width = w;
                    s.height = h;
                }
                Some(flare_proto::common::message_content::Content::Image(ref mut img)) => {
                    if let Some(ref mut s) = img.source {
                        s.width = w;
                        s.height = h;
                    }
                }
                _ => {}
            }
        }
        self
    }

    pub fn package_id(mut self, p: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Sticker(ref mut s)) =
                c.content
            {
                s.package_id = p.into();
            }
        }
        self
    }

    /// 贴纸资源格式（如 `webp`、`png`），写入 proto `format`
    pub fn sticker_format(mut self, f: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Sticker(ref mut s)) =
                c.content
            {
                s.format = f.into();
            }
        }
        self
    }

    pub fn emoji(emoji: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Emoji(
                EmojiContent {
                    emoji: emoji.into(),
                    description: String::new(),
                    extra: std::collections::HashMap::new(),
                },
            )),
        };
        Self {
            message_type: MessageType::Emoji,
            content: Some(content),
        }
    }

    /// 本地/资源库动图 id，落库为 **IMAGE** + `ImageContent`（GIF + animated，无缩略图）。
    pub fn gif(gif_id: impl Into<String>) -> Self {
        let id = gif_id.into();
        let source = ImageInfo {
            uuid: id.clone(),
            image_id: id,
            url: String::new(),
            mime_type: "image/gif".into(),
            size: 0,
            width: 0,
            height: 0,
            format: ImageFormat::Gif as i32,
            animated: true,
        };
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Image(
                ImageContent {
                    source: Some(source),
                    thumbnail: None,
                    description: String::new(),
                    duration_ms: None,
                },
            )),
        };
        Self {
            message_type: MessageType::Image,
            content: Some(content),
        }
    }

    /// 动图时长（毫秒），写入 `ImageContent.duration_ms`
    pub fn image_duration_ms(mut self, ms: i64) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Image(ref mut i)) = c.content
            {
                i.duration_ms = Some(ms);
            }
        }
        self
    }

    pub fn quote(quoted_message_id: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Quote(
                std::boxed::Box::new(QuoteContent {
                    quoted_message_id: quoted_message_id.into(),
                    quoted_sender_id: String::new(),
                    quoted_text_preview: String::new(),
                    quoted_content: None,
                    current_content: None,
                }),
            )),
        };
        Self {
            message_type: MessageType::Quote,
            content: Some(content),
        }
    }

    pub fn quoted_sender_id(mut self, s: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Quote(ref mut q)) = c.content
            {
                q.quoted_sender_id = s.into();
            }
        }
        self
    }

    pub fn quoted_text_preview(mut self, s: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Quote(ref mut q)) = c.content
            {
                q.quoted_text_preview = s.into();
            }
        }
        self
    }

    pub fn quoted_content(mut self, content: MessageContent) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Quote(ref mut q)) = c.content
            {
                q.quoted_content = Some(Box::new(content));
            }
        }
        self
    }

    pub fn quoted(mut self, content: BuiltContent) -> Self {
        self = self.quoted_content(content.inner);
        self
    }

    pub fn current_text(mut self, s: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Quote(ref mut q)) = c.content
            {
                q.current_content = Some(Box::new(MessageContent {
                    content: Some(flare_proto::common::message_content::Content::Text(
                        flare_proto::common::TextContent {
                            text: s.into(),
                            mentions: Vec::new(),
                        },
                    )),
                }));
            }
        }
        self
    }

    pub fn current_content(mut self, content: MessageContent) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Quote(ref mut q)) = c.content
            {
                // 防止出现 Quote 套 Quote，避免展示与摘要链路歧义。
                let is_quote = matches!(
                    content.content,
                    Some(flare_proto::common::message_content::Content::Quote(_))
                );
                q.current_content = if is_quote {
                    None
                } else {
                    Some(Box::new(content))
                };
            }
        }
        self
    }

    pub fn current(mut self, content: BuiltContent) -> Self {
        if content.message_type == MessageType::Quote {
            return self;
        }
        self = self.current_content(content.inner);
        self
    }

    pub fn thread(thread_id: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Thread(
                std::boxed::Box::new(ThreadContent {
                    thread_id: thread_id.into(),
                    thread_title: String::new(),
                    root_content: None,
                    metadata: std::collections::HashMap::new(),
                }),
            )),
        };
        Self {
            message_type: MessageType::Thread,
            content: Some(content),
        }
    }

    pub fn thread_title(mut self, s: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Thread(ref mut t)) =
                c.content
            {
                t.thread_title = s.into();
            }
        }
        self
    }

    pub fn forward(mode: ForwardMode, title: Option<String>, items: Vec<ForwardItem>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Forward(
                ForwardContent {
                    mode: mode as i32,
                    title,
                    items,
                },
            )),
        };
        Self {
            message_type: MessageType::MergeForward,
            content: Some(content),
        }
    }

    pub fn forward_title(mut self, s: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Forward(ref mut f)) =
                c.content
            {
                let t = s.into();
                f.title = if t.trim().is_empty() { None } else { Some(t) };
            }
        }
        self
    }

    pub fn link_card(url: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::LinkCard(
                LinkCardContent {
                    url: url.into(),
                    title: String::new(),
                    description: String::new(),
                    thumbnail_url: String::new(),
                    site_name: String::new(),
                },
            )),
        };
        Self {
            message_type: MessageType::LinkCard,
            content: Some(content),
        }
    }

    pub fn link_card_thumbnail_url(mut self, u: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::LinkCard(ref mut l)) =
                c.content
            {
                l.thumbnail_url = u.into();
            }
        }
        self
    }

    pub fn link_card_site_name(mut self, s: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::LinkCard(ref mut l)) =
                c.content
            {
                l.site_name = s.into();
            }
        }
        self
    }

    pub fn title(mut self, t: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            match c.content {
                Some(flare_proto::common::message_content::Content::LinkCard(ref mut l)) => {
                    l.title = t.into();
                }
                Some(flare_proto::common::message_content::Content::MiniProgram(ref mut m)) => {
                    m.title = t.into();
                }
                Some(flare_proto::common::message_content::Content::Location(ref mut loc)) => {
                    loc.title = t.into();
                }
                Some(flare_proto::common::message_content::Content::Card(ref mut card)) => {
                    card.title = t.into();
                }
                _ => {}
            }
        }
        self
    }

    pub fn page_path(mut self, p: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::MiniProgram(ref mut m)) =
                c.content
            {
                m.page_path = p.into();
            }
        }
        self
    }

    pub fn mini_program(app_id: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::MiniProgram(
                MiniProgramContent {
                    app_id: app_id.into(),
                    title: String::new(),
                    page_path: String::new(),
                    thumbnail_url: String::new(),
                    extra: std::collections::HashMap::new(),
                },
            )),
        };
        Self {
            message_type: MessageType::MiniProgram,
            content: Some(content),
        }
    }

    pub fn mini_program_thumbnail_url(mut self, u: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::MiniProgram(ref mut m)) =
                c.content
            {
                m.thumbnail_url = u.into();
            }
        }
        self
    }

    /// 合并 `MiniProgramContent.extra`（跳过空 key）。
    pub fn mini_program_extend_extra(
        mut self,
        entries: std::collections::HashMap<String, String>,
    ) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::MiniProgram(ref mut m)) =
                c.content
            {
                for (k, v) in entries {
                    let k = k.trim().to_string();
                    if !k.is_empty() {
                        m.extra.insert(k, v);
                    }
                }
            }
        }
        self
    }

    /// 构建富文本：`doc_json` 在 `content_schema == rich_doc` 时走 [`crate::rich_doc_v2::validate_doc_json`]。
    pub fn try_rich_doc(
        doc_json: impl Into<String>,
        content_schema: impl Into<String>,
        plain_text: impl Into<String>,
    ) -> Result<Self, RichDocV2Error> {
        let doc_json = doc_json.into();
        let content_schema = content_schema.into();
        if content_schema == CONTENT_SCHEMA_RICH_DOC {
            validate_doc_json(&doc_json)?;
        }
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::RichText(
                RichTextContent {
                    doc_json,
                    content_schema,
                    plain_text: plain_text.into(),
                    input_format: None,
                    source_payload: std::collections::HashMap::new(),
                    input_format_version: None,
                    title: None,
                    search_text: None,
                    render_hints_json: None,
                },
            )),
        };
        Ok(Self {
            message_type: MessageType::RichText,
            content: Some(content),
        })
    }

    pub fn rich_text_input_format(mut self, v: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::RichText(ref mut r)) =
                c.content
            {
                r.input_format = Some(v.into());
            }
        }
        self
    }

    pub fn rich_text_input_format_version(mut self, v: i32) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::RichText(ref mut r)) =
                c.content
            {
                r.input_format_version = Some(v);
            }
        }
        self
    }

    pub fn rich_text_source_payload_entry(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::RichText(ref mut r)) =
                c.content
            {
                r.source_payload.insert(key.into(), value.into());
            }
        }
        self
    }

    /// 长文标题（`RichTextContent.title`）；`None` 或空串会写入 `None`。
    pub fn rich_text_title(mut self, title: Option<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::RichText(ref mut r)) =
                c.content
            {
                r.title = title
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
            }
        }
        self
    }

    /// SDK 派生检索串（`RichTextContent.search_text`）。
    pub fn rich_text_search_text(mut self, search_text: Option<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::RichText(ref mut r)) =
                c.content
            {
                r.search_text = search_text
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
            }
        }
        self
    }

    /// 渲染提示 JSON 字符串（`RichTextContent.render_hints_json`）。
    pub fn rich_text_render_hints_json(mut self, render_hints_json: Option<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::RichText(ref mut r)) =
                c.content
            {
                r.render_hints_json = render_hints_json
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
            }
        }
        self
    }

    pub fn image_group(images: Vec<ImageInfo>) -> Self {
        Self::image_group_with_details(images, String::new(), std::collections::HashMap::new())
    }

    pub fn image_group_with_details(
        images: Vec<ImageInfo>,
        description: impl Into<String>,
        metadata: std::collections::HashMap<String, String>,
    ) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::ImageGroup(
                ImageGroupContent {
                    images,
                    description: description.into(),
                    metadata,
                },
            )),
        };
        Self {
            message_type: MessageType::ImageGroup,
            content: Some(content),
        }
    }

    pub fn system(event_kind: impl Into<String>, body: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::System(
                SystemContent {
                    event_kind: event_kind.into(),
                    body: body.into(),
                    data: std::collections::HashMap::new(),
                    payload: vec![],
                },
            )),
        };
        Self {
            message_type: MessageType::System,
            content: Some(content),
        }
    }

    pub fn data(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::System(ref mut s)) =
                c.content
            {
                s.data.insert(key.into(), value.into());
            }
        }
        self
    }

    pub fn notification(title: impl Into<String>, body: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Notification(
                NotificationContent {
                    title: title.into(),
                    body: body.into(),
                    notification_type: String::new(),
                    data: std::collections::HashMap::new(),
                    target_user_ids: vec![],
                    target_role_id: String::new(),
                    notify_all: false,
                    persistent: false,
                    show_in_list: false,
                    show_badge: false,
                    play_sound: false,
                },
            )),
        };
        Self {
            message_type: MessageType::Notification,
            content: Some(content),
        }
    }

    pub fn notification_type(mut self, t: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Notification(ref mut n)) =
                c.content
            {
                n.notification_type = t.into();
            }
        }
        self
    }

    pub fn persistent(mut self, v: bool) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Notification(ref mut n)) =
                c.content
            {
                n.persistent = v;
            }
        }
        self
    }

    pub fn show_badge(mut self, v: bool) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Notification(ref mut n)) =
                c.content
            {
                n.show_badge = v;
            }
        }
        self
    }

    pub fn vote(
        vote_id: impl Into<String>,
        title: impl Into<String>,
        options: Vec<String>,
    ) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Vote(
                VoteContent {
                    vote_id: vote_id.into(),
                    title: title.into(),
                    options,
                    metadata: std::collections::HashMap::new(),
                    participant_user_ids: vec![],
                },
            )),
        };
        Self {
            message_type: MessageType::Poll,
            content: Some(content),
        }
    }

    pub fn vote_participant_user_ids(mut self, ids: Vec<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Vote(ref mut v)) = c.content
            {
                v.participant_user_ids = ids;
            }
        }
        self
    }

    pub fn task(task_id: impl Into<String>, title: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Task(
                TaskContent {
                    task_id: task_id.into(),
                    title: title.into(),
                    status: String::new(),
                    metadata: std::collections::HashMap::new(),
                    participant_user_ids: vec![],
                },
            )),
        };
        Self {
            message_type: MessageType::Task,
            content: Some(content),
        }
    }

    pub fn status(mut self, s: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Task(ref mut t)) = c.content
            {
                t.status = s.into();
            }
        }
        self
    }

    pub fn task_participant_user_ids(mut self, ids: Vec<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Task(ref mut t)) = c.content
            {
                t.participant_user_ids = ids;
            }
        }
        self
    }

    pub fn schedule(schedule_id: impl Into<String>, title: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Schedule(
                ScheduleContent {
                    schedule_id: schedule_id.into(),
                    title: title.into(),
                    start_time_ms: 0,
                    end_time_ms: 0,
                    metadata: std::collections::HashMap::new(),
                    participant_user_ids: vec![],
                },
            )),
        };
        Self {
            message_type: MessageType::Schedule,
            content: Some(content),
        }
    }

    pub fn schedule_times_ms(mut self, start_ms: i64, end_ms: i64) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Schedule(ref mut s)) =
                c.content
            {
                s.start_time_ms = start_ms;
                s.end_time_ms = end_ms;
            }
        }
        self
    }

    pub fn schedule_participant_user_ids(mut self, ids: Vec<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Schedule(ref mut sch)) =
                c.content
            {
                sch.participant_user_ids = ids;
            }
        }
        self
    }

    pub fn announcement(title: impl Into<String>, body: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Announcement(
                AnnouncementContent {
                    title: title.into(),
                    body: body.into(),
                    pinned: false,
                    metadata: std::collections::HashMap::new(),
                },
            )),
        };
        Self {
            message_type: MessageType::Announcement,
            content: Some(content),
        }
    }

    pub fn pinned(mut self, v: bool) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Announcement(ref mut a)) =
                c.content
            {
                a.pinned = v;
            }
        }
        self
    }

    pub fn placeholder(reason: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Placeholder(
                PlaceholderContent {
                    reason: reason.into(),
                    payload: vec![],
                    fallback_text: String::new(),
                    metadata: std::collections::HashMap::new(),
                },
            )),
        };
        Self {
            message_type: MessageType::E2ePlaceholder,
            content: Some(content),
        }
    }

    pub fn fallback_text(mut self, s: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Placeholder(ref mut p)) =
                c.content
            {
                p.fallback_text = s.into();
            }
        }
        self
    }

    pub fn custom(r#type: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Custom(
                CustomContent {
                    r#type: r#type.into(),
                    payload: vec![],
                    description: String::new(),
                    metadata: std::collections::HashMap::new(),
                },
            )),
        };
        Self {
            message_type: MessageType::Custom,
            content: Some(content),
        }
    }

    pub fn payload(mut self, p: Vec<u8>) -> Self {
        if let Some(ref mut c) = self.content {
            match c.content {
                Some(flare_proto::common::message_content::Content::Custom(ref mut cu)) => {
                    cu.payload = p;
                }
                Some(flare_proto::common::message_content::Content::Placeholder(ref mut ph)) => {
                    ph.payload = p;
                }
                _ => {}
            }
        }
        self
    }

    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            match c.content {
                Some(flare_proto::common::message_content::Content::Custom(ref mut cu)) => {
                    cu.metadata.insert(key.into(), value.into());
                }
                _ => {}
            }
        }
        self
    }

    pub fn build(self) -> BuiltContent {
        let content = self
            .content
            .unwrap_or_else(|| MessageContent { content: None });
        BuiltContent::new(self.message_type, content)
    }
}
