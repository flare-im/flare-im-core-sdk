//! Type-safe builders for all message content types defined in `message_content.proto`.
//!
//! ```ignore
//! use flare_im_core_sdk::model::content_builder::ContentBuilder;
//!
//! // 文本消息（带 @提及）
//! let content = ContentBuilder::text("Hello @Alice!")
//!     .mention_user("alice_id", 6, 6)
//!     .build();
//!
//! // 图片消息
//! let content = ContentBuilder::image("img_123")
//!     .source(ImageInfo { url: "https://cdn.example.com/a.jpg".into(), ..Default::default() })
//!     .build();
//!
//! // 自定义消息
//! let content = ContentBuilder::custom("red_packet")
//!     .payload(b"...".to_vec())
//!     .description("恭喜发财")
//!     .build();
//! ```

use flare_proto::common::{
    MessageContent, MessageType, message_content,
    TextContent, Mention, MentionType,
    ImageContent, ImageInfo,
    VideoContent, VideoInfo,
    AudioContent, AudioInfo,
    FileContent, LocationContent, CardContent,
    StickerContent, EmojiContent, GifContent,
    QuoteContent, LinkCardContent, ForwardContent, MessagePreview,
    ThreadContent, MiniProgramContent,
    RichTextContent, MarkdownContent, ImageGroupContent,
    SystemContent, NotificationContent,
    VoteContent, TaskContent, ScheduleContent, AnnouncementContent,
    CustomContent, PlaceholderContent,
};

/// 构建后的消息内容 — 同时携带 MessageType 和 MessageContent
#[derive(Clone, Debug)]
pub struct BuiltContent {
    pub message_type: MessageType,
    pub content: MessageContent,
}

impl BuiltContent {
    /// 将 MessageContent 编码为 bytes（用于 Message.content 字段）
    pub fn encode(&self) -> Vec<u8> {
        use prost::Message;
        self.content.encode_to_vec()
    }
}

/// 消息内容构建入口
pub struct ContentBuilder;

impl ContentBuilder {
    // ── 1-15: 基础内容 ──────────────────────────────────────

    pub fn text(text: impl Into<String>) -> TextContentBuilder {
        TextContentBuilder { text: text.into(), mentions: Vec::new() }
    }

    pub fn image(image_id: impl Into<String>) -> ImageContentBuilder {
        ImageContentBuilder {
            inner: ImageContent { image_id: image_id.into(), ..Default::default() },
        }
    }

    pub fn video(video_id: impl Into<String>) -> VideoContentBuilder {
        VideoContentBuilder {
            inner: VideoContent { video_id: video_id.into(), ..Default::default() },
        }
    }

    pub fn audio(audio_id: impl Into<String>) -> AudioContentBuilder {
        AudioContentBuilder {
            inner: AudioContent { audio_id: audio_id.into(), ..Default::default() },
        }
    }

    pub fn file(file_id: impl Into<String>) -> FileContentBuilder {
        FileContentBuilder {
            inner: FileContent { file_id: file_id.into(), ..Default::default() },
        }
    }

    pub fn location(longitude: f64, latitude: f64) -> LocationContentBuilder {
        LocationContentBuilder {
            inner: LocationContent { longitude, latitude, ..Default::default() },
        }
    }

    pub fn card(user_id: impl Into<String>) -> CardContentBuilder {
        CardContentBuilder {
            inner: CardContent { user_id: user_id.into(), ..Default::default() },
        }
    }

    pub fn sticker(sticker_id: impl Into<String>) -> StickerContentBuilder {
        StickerContentBuilder {
            inner: StickerContent { sticker_id: sticker_id.into(), ..Default::default() },
        }
    }

    pub fn emoji(emoji: impl Into<String>) -> EmojiContentBuilder {
        EmojiContentBuilder {
            inner: EmojiContent { emoji: emoji.into(), ..Default::default() },
        }
    }

    pub fn gif(gif_id: impl Into<String>) -> GifContentBuilder {
        GifContentBuilder {
            inner: GifContent { gif_id: gif_id.into(), ..Default::default() },
        }
    }

    pub fn quote(quoted_message_id: impl Into<String>) -> QuoteContentBuilder {
        QuoteContentBuilder {
            inner: QuoteContent { quoted_message_id: quoted_message_id.into(), ..Default::default() },
        }
    }

    pub fn link_card(url: impl Into<String>) -> LinkCardContentBuilder {
        LinkCardContentBuilder {
            inner: LinkCardContent { url: url.into(), ..Default::default() },
        }
    }

    pub fn forward(message_ids: Vec<String>) -> ForwardContentBuilder {
        ForwardContentBuilder {
            inner: ForwardContent { message_ids, ..Default::default() },
        }
    }

    pub fn thread(thread_id: impl Into<String>) -> ThreadContentBuilder {
        ThreadContentBuilder {
            inner: ThreadContent { thread_id: thread_id.into(), ..Default::default() },
        }
    }

    pub fn mini_program(app_id: impl Into<String>) -> MiniProgramContentBuilder {
        MiniProgramContentBuilder {
            inner: MiniProgramContent { app_id: app_id.into(), ..Default::default() },
        }
    }

    // ── 30-32: 富媒体 ──────────────────────────────────────

    pub fn rich_text(content: impl Into<String>, format: impl Into<String>) -> RichTextContentBuilder {
        RichTextContentBuilder {
            inner: RichTextContent { content: content.into(), format: format.into(), ..Default::default() },
        }
    }

    pub fn markdown(text: impl Into<String>) -> MarkdownContentBuilder {
        MarkdownContentBuilder {
            inner: MarkdownContent { text: text.into(), ..Default::default() },
        }
    }

    pub fn image_group(images: Vec<ImageInfo>) -> ImageGroupContentBuilder {
        ImageGroupContentBuilder {
            inner: ImageGroupContent { images, ..Default::default() },
        }
    }

    // ── 60-61: 系统与通知 ───────────────────────────────────

    pub fn system(event_kind: impl Into<String>, body: impl Into<String>) -> SystemContentBuilder {
        SystemContentBuilder {
            inner: SystemContent { event_kind: event_kind.into(), body: body.into(), ..Default::default() },
        }
    }

    pub fn notification(title: impl Into<String>, body: impl Into<String>) -> NotificationContentBuilder {
        NotificationContentBuilder {
            inner: NotificationContent { title: title.into(), body: body.into(), ..Default::default() },
        }
    }

    // ── 80-83: 平台推荐业务 ─────────────────────────────────

    pub fn vote(vote_id: impl Into<String>, title: impl Into<String>, options: Vec<String>) -> VoteContentBuilder {
        VoteContentBuilder {
            inner: VoteContent { vote_id: vote_id.into(), title: title.into(), options, ..Default::default() },
        }
    }

    pub fn task(task_id: impl Into<String>, title: impl Into<String>) -> TaskContentBuilder {
        TaskContentBuilder {
            inner: TaskContent { task_id: task_id.into(), title: title.into(), ..Default::default() },
        }
    }

    pub fn schedule(schedule_id: impl Into<String>, title: impl Into<String>) -> ScheduleContentBuilder {
        ScheduleContentBuilder {
            inner: ScheduleContent { schedule_id: schedule_id.into(), title: title.into(), ..Default::default() },
        }
    }

    pub fn announcement(title: impl Into<String>, body: impl Into<String>) -> AnnouncementContentBuilder {
        AnnouncementContentBuilder {
            inner: AnnouncementContent { title: title.into(), body: body.into(), ..Default::default() },
        }
    }

    // ── 100: 自定义 ─────────────────────────────────────────

    pub fn custom(r#type: impl Into<String>) -> CustomContentBuilder {
        CustomContentBuilder {
            inner: CustomContent { r#type: r#type.into(), ..Default::default() },
        }
    }

    // ── 101-115: 平台能力 ───────────────────────────────────

    pub fn placeholder(reason: impl Into<String>) -> PlaceholderContentBuilder {
        PlaceholderContentBuilder {
            inner: PlaceholderContent { reason: reason.into(), ..Default::default() },
        }
    }
}

// =============================================================================
// 各类型 Builder
// =============================================================================

// ── TextContentBuilder ──────────────────────────────────────

pub struct TextContentBuilder {
    text: String,
    mentions: Vec<Mention>,
}

impl TextContentBuilder {
    pub fn mention_user(mut self, user_id: impl Into<String>, start: i32, length: i32) -> Self {
        self.mentions.push(Mention {
            r#type: MentionType::User as i32,
            user_id: user_id.into(),
            start, length,
            ..Default::default()
        });
        self
    }

    pub fn mention_users(mut self, user_ids: Vec<String>, start: i32, length: i32) -> Self {
        self.mentions.push(Mention {
            r#type: MentionType::Multi as i32,
            user_ids,
            start, length,
            ..Default::default()
        });
        self
    }

    pub fn mention_all(mut self, start: i32, length: i32) -> Self {
        self.mentions.push(Mention {
            r#type: MentionType::All as i32,
            start, length,
            ..Default::default()
        });
        self
    }

    pub fn mention_role(mut self, role_id: impl Into<String>, start: i32, length: i32) -> Self {
        self.mentions.push(Mention {
            r#type: MentionType::Role as i32,
            role_id: role_id.into(),
            start, length,
            ..Default::default()
        });
        self
    }

    pub fn mention(mut self, mention: Mention) -> Self {
        self.mentions.push(mention);
        self
    }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Text,
            content: MessageContent {
                content: Some(message_content::Content::Text(TextContent {
                    text: self.text,
                    mentions: self.mentions,
                })),
            },
        }
    }
}

// ── ImageContentBuilder ─────────────────────────────────────

pub struct ImageContentBuilder {
    inner: ImageContent,
}

impl ImageContentBuilder {
    pub fn source(mut self, info: ImageInfo) -> Self { self.inner.source = Some(info); self }
    pub fn thumbnail(mut self, info: ImageInfo) -> Self { self.inner.thumbnail = Some(info); self }
    pub fn description(mut self, v: impl Into<String>) -> Self { self.inner.description = v.into(); self }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Image,
            content: MessageContent { content: Some(message_content::Content::Image(self.inner)) },
        }
    }
}

// ── VideoContentBuilder ─────────────────────────────────────

pub struct VideoContentBuilder {
    inner: VideoContent,
}

impl VideoContentBuilder {
    pub fn source(mut self, info: VideoInfo) -> Self { self.inner.source = Some(info); self }
    pub fn cover(mut self, info: ImageInfo) -> Self { self.inner.cover = Some(info); self }
    pub fn description(mut self, v: impl Into<String>) -> Self { self.inner.description = v.into(); self }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Video,
            content: MessageContent { content: Some(message_content::Content::Video(self.inner)) },
        }
    }
}

// ── AudioContentBuilder ─────────────────────────────────────

pub struct AudioContentBuilder {
    inner: AudioContent,
}

impl AudioContentBuilder {
    pub fn source(mut self, info: AudioInfo) -> Self { self.inner.source = Some(info); self }
    pub fn description(mut self, v: impl Into<String>) -> Self { self.inner.description = v.into(); self }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Audio,
            content: MessageContent { content: Some(message_content::Content::Audio(self.inner)) },
        }
    }
}

// ── FileContentBuilder ──────────────────────────────────────

pub struct FileContentBuilder {
    inner: FileContent,
}

impl FileContentBuilder {
    pub fn file_name(mut self, v: impl Into<String>) -> Self { self.inner.file_name = v.into(); self }
    pub fn mime_type(mut self, v: impl Into<String>) -> Self { self.inner.mime_type = v.into(); self }
    pub fn file_size(mut self, v: i64) -> Self { self.inner.file_size = v; self }
    pub fn url(mut self, v: impl Into<String>) -> Self { self.inner.url = v.into(); self }
    pub fn description(mut self, v: impl Into<String>) -> Self { self.inner.description = v.into(); self }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::File,
            content: MessageContent { content: Some(message_content::Content::File(self.inner)) },
        }
    }
}

// ── LocationContentBuilder ──────────────────────────────────

pub struct LocationContentBuilder {
    inner: LocationContent,
}

impl LocationContentBuilder {
    pub fn address(mut self, v: impl Into<String>) -> Self { self.inner.address = v.into(); self }
    pub fn description(mut self, v: impl Into<String>) -> Self { self.inner.description = v.into(); self }
    pub fn poi_id(mut self, v: impl Into<String>) -> Self { self.inner.poi_id = v.into(); self }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Location,
            content: MessageContent { content: Some(message_content::Content::Location(self.inner)) },
        }
    }
}

// ── CardContentBuilder ──────────────────────────────────────

pub struct CardContentBuilder {
    inner: CardContent,
}

impl CardContentBuilder {
    pub fn nickname(mut self, v: impl Into<String>) -> Self { self.inner.nickname = v.into(); self }
    pub fn avatar_url(mut self, v: impl Into<String>) -> Self { self.inner.avatar_url = v.into(); self }
    pub fn description(mut self, v: impl Into<String>) -> Self { self.inner.description = v.into(); self }
    pub fn extra(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.extra.insert(key.into(), value.into()); self
    }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Card,
            content: MessageContent { content: Some(message_content::Content::Card(self.inner)) },
        }
    }
}

// ── StickerContentBuilder ───────────────────────────────────

pub struct StickerContentBuilder {
    inner: StickerContent,
}

impl StickerContentBuilder {
    pub fn url(mut self, v: impl Into<String>) -> Self { self.inner.url = v.into(); self }
    pub fn size(mut self, width: i32, height: i32) -> Self { self.inner.width = width; self.inner.height = height; self }
    pub fn extra(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.extra.insert(key.into(), value.into()); self
    }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Sticker,
            content: MessageContent { content: Some(message_content::Content::Sticker(self.inner)) },
        }
    }
}

// ── EmojiContentBuilder ─────────────────────────────────────

pub struct EmojiContentBuilder {
    inner: EmojiContent,
}

impl EmojiContentBuilder {
    pub fn description(mut self, v: impl Into<String>) -> Self { self.inner.description = v.into(); self }
    pub fn extra(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.extra.insert(key.into(), value.into()); self
    }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Emoji,
            content: MessageContent { content: Some(message_content::Content::Emoji(self.inner)) },
        }
    }
}

// ── GifContentBuilder ───────────────────────────────────────

pub struct GifContentBuilder {
    inner: GifContent,
}

impl GifContentBuilder {
    pub fn url(mut self, v: impl Into<String>) -> Self { self.inner.url = v.into(); self }
    pub fn thumbnail(mut self, info: ImageInfo) -> Self { self.inner.thumbnail = Some(info); self }
    pub fn duration_ms(mut self, v: i64) -> Self { self.inner.duration_ms = v; self }
    pub fn size(mut self, width: i32, height: i32) -> Self { self.inner.width = width; self.inner.height = height; self }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Gif,
            content: MessageContent { content: Some(message_content::Content::Gif(self.inner)) },
        }
    }
}

// ── QuoteContentBuilder ─────────────────────────────────────

pub struct QuoteContentBuilder {
    inner: QuoteContent,
}

impl QuoteContentBuilder {
    pub fn quoted_sender_id(mut self, v: impl Into<String>) -> Self { self.inner.quoted_sender_id = v.into(); self }
    pub fn quoted_text_preview(mut self, v: impl Into<String>) -> Self { self.inner.quoted_text_preview = v.into(); self }
    pub fn quoted_content(mut self, content: MessageContent) -> Self { self.inner.quoted_content = Some(Box::new(content)); self }
    /// 使用 BuiltContent 设置被引用内容
    pub fn quoted_built_content(mut self, built: BuiltContent) -> Self { self.inner.quoted_content = Some(Box::new(built.content)); self }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Quote,
            content: MessageContent { content: Some(message_content::Content::Quote(Box::new(self.inner))) },
        }
    }
}

// ── LinkCardContentBuilder ──────────────────────────────────

pub struct LinkCardContentBuilder {
    inner: LinkCardContent,
}

impl LinkCardContentBuilder {
    pub fn title(mut self, v: impl Into<String>) -> Self { self.inner.title = v.into(); self }
    pub fn description(mut self, v: impl Into<String>) -> Self { self.inner.description = v.into(); self }
    pub fn thumbnail_url(mut self, v: impl Into<String>) -> Self { self.inner.thumbnail_url = v.into(); self }
    pub fn site_name(mut self, v: impl Into<String>) -> Self { self.inner.site_name = v.into(); self }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::LinkCard,
            content: MessageContent { content: Some(message_content::Content::LinkCard(self.inner)) },
        }
    }
}

// ── ForwardContentBuilder ───────────────────────────────────

pub struct ForwardContentBuilder {
    inner: ForwardContent,
}

impl ForwardContentBuilder {
    pub fn forward_reason(mut self, v: impl Into<String>) -> Self { self.inner.forward_reason = v.into(); self }
    pub fn add_preview(mut self, preview: MessagePreview) -> Self { self.inner.forwarded_previews.push(preview); self }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::MergeForward,
            content: MessageContent { content: Some(message_content::Content::Forward(self.inner)) },
        }
    }
}

// ── ThreadContentBuilder ────────────────────────────────────

pub struct ThreadContentBuilder {
    inner: ThreadContent,
}

impl ThreadContentBuilder {
    pub fn thread_title(mut self, v: impl Into<String>) -> Self { self.inner.thread_title = v.into(); self }
    pub fn root_content(mut self, content: MessageContent) -> Self { self.inner.root_content = Some(Box::new(content)); self }
    pub fn root_built_content(mut self, built: BuiltContent) -> Self { self.inner.root_content = Some(Box::new(built.content)); self }
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.metadata.insert(key.into(), value.into()); self
    }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Thread,
            content: MessageContent { content: Some(message_content::Content::Thread(Box::new(self.inner))) },
        }
    }
}

// ── MiniProgramContentBuilder ───────────────────────────────

pub struct MiniProgramContentBuilder {
    inner: MiniProgramContent,
}

impl MiniProgramContentBuilder {
    pub fn title(mut self, v: impl Into<String>) -> Self { self.inner.title = v.into(); self }
    pub fn page_path(mut self, v: impl Into<String>) -> Self { self.inner.page_path = v.into(); self }
    pub fn thumbnail_url(mut self, v: impl Into<String>) -> Self { self.inner.thumbnail_url = v.into(); self }
    pub fn extra(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.extra.insert(key.into(), value.into()); self
    }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::MiniProgram,
            content: MessageContent { content: Some(message_content::Content::MiniProgram(self.inner)) },
        }
    }
}

// ── RichTextContentBuilder ──────────────────────────────────

pub struct RichTextContentBuilder {
    inner: RichTextContent,
}

impl RichTextContentBuilder {
    pub fn mention(mut self, mention: Mention) -> Self { self.inner.mentions.push(mention); self }
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.metadata.insert(key.into(), value.into()); self
    }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::RichText,
            content: MessageContent { content: Some(message_content::Content::RichText(self.inner)) },
        }
    }
}

// ── MarkdownContentBuilder ──────────────────────────────────

pub struct MarkdownContentBuilder {
    inner: MarkdownContent,
}

impl MarkdownContentBuilder {
    pub fn mention(mut self, mention: Mention) -> Self { self.inner.mentions.push(mention); self }
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.metadata.insert(key.into(), value.into()); self
    }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Markdown,
            content: MessageContent { content: Some(message_content::Content::Markdown(self.inner)) },
        }
    }
}

// ── ImageGroupContentBuilder ────────────────────────────────

pub struct ImageGroupContentBuilder {
    inner: ImageGroupContent,
}

impl ImageGroupContentBuilder {
    pub fn add_image(mut self, info: ImageInfo) -> Self { self.inner.images.push(info); self }
    pub fn description(mut self, v: impl Into<String>) -> Self { self.inner.description = v.into(); self }
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.metadata.insert(key.into(), value.into()); self
    }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::ImageGroup,
            content: MessageContent { content: Some(message_content::Content::ImageGroup(self.inner)) },
        }
    }
}

// ── SystemContentBuilder ────────────────────────────────────

pub struct SystemContentBuilder {
    inner: SystemContent,
}

impl SystemContentBuilder {
    pub fn data(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.data.insert(key.into(), value.into()); self
    }
    pub fn payload(mut self, v: Vec<u8>) -> Self { self.inner.payload = v; self }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::System,
            content: MessageContent { content: Some(message_content::Content::System(self.inner)) },
        }
    }
}

// ── NotificationContentBuilder ──────────────────────────────

pub struct NotificationContentBuilder {
    inner: NotificationContent,
}

impl NotificationContentBuilder {
    pub fn notification_type(mut self, v: impl Into<String>) -> Self { self.inner.notification_type = v.into(); self }
    pub fn data(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.data.insert(key.into(), value.into()); self
    }
    pub fn target_user_ids(mut self, ids: Vec<String>) -> Self { self.inner.target_user_ids = ids; self }
    pub fn target_role_id(mut self, v: impl Into<String>) -> Self { self.inner.target_role_id = v.into(); self }
    pub fn notify_all(mut self, v: bool) -> Self { self.inner.notify_all = v; self }
    pub fn persistent(mut self, v: bool) -> Self { self.inner.persistent = v; self }
    pub fn show_in_list(mut self, v: bool) -> Self { self.inner.show_in_list = v; self }
    pub fn show_badge(mut self, v: bool) -> Self { self.inner.show_badge = v; self }
    pub fn play_sound(mut self, v: bool) -> Self { self.inner.play_sound = v; self }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Notification,
            content: MessageContent { content: Some(message_content::Content::Notification(self.inner)) },
        }
    }
}

// ── VoteContentBuilder ──────────────────────────────────────

pub struct VoteContentBuilder {
    inner: VoteContent,
}

impl VoteContentBuilder {
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.metadata.insert(key.into(), value.into()); self
    }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Poll,
            content: MessageContent { content: Some(message_content::Content::Vote(self.inner)) },
        }
    }
}

// ── TaskContentBuilder ──────────────────────────────────────

pub struct TaskContentBuilder {
    inner: TaskContent,
}

impl TaskContentBuilder {
    pub fn status(mut self, v: impl Into<String>) -> Self { self.inner.status = v.into(); self }
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.metadata.insert(key.into(), value.into()); self
    }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Task,
            content: MessageContent { content: Some(message_content::Content::Task(self.inner)) },
        }
    }
}

// ── ScheduleContentBuilder ──────────────────────────────────

pub struct ScheduleContentBuilder {
    inner: ScheduleContent,
}

impl ScheduleContentBuilder {
    pub fn start_time(mut self, ts: prost_types::Timestamp) -> Self { self.inner.start_time = Some(ts); self }
    pub fn end_time(mut self, ts: prost_types::Timestamp) -> Self { self.inner.end_time = Some(ts); self }
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.metadata.insert(key.into(), value.into()); self
    }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Schedule,
            content: MessageContent { content: Some(message_content::Content::Schedule(self.inner)) },
        }
    }
}

// ── AnnouncementContentBuilder ──────────────────────────────

pub struct AnnouncementContentBuilder {
    inner: AnnouncementContent,
}

impl AnnouncementContentBuilder {
    pub fn pinned(mut self, v: bool) -> Self { self.inner.pinned = v; self }
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.metadata.insert(key.into(), value.into()); self
    }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Announcement,
            content: MessageContent { content: Some(message_content::Content::Announcement(self.inner)) },
        }
    }
}

// ── CustomContentBuilder ────────────────────────────────────

pub struct CustomContentBuilder {
    inner: CustomContent,
}

impl CustomContentBuilder {
    pub fn payload(mut self, v: Vec<u8>) -> Self { self.inner.payload = v; self }
    pub fn description(mut self, v: impl Into<String>) -> Self { self.inner.description = v.into(); self }
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.metadata.insert(key.into(), value.into()); self
    }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::Custom,
            content: MessageContent { content: Some(message_content::Content::Custom(self.inner)) },
        }
    }
}

// ── PlaceholderContentBuilder ───────────────────────────────

pub struct PlaceholderContentBuilder {
    inner: PlaceholderContent,
}

impl PlaceholderContentBuilder {
    pub fn payload(mut self, v: Vec<u8>) -> Self { self.inner.payload = v; self }
    pub fn fallback_text(mut self, v: impl Into<String>) -> Self { self.inner.fallback_text = v.into(); self }
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.metadata.insert(key.into(), value.into()); self
    }

    pub fn build(self) -> BuiltContent {
        BuiltContent {
            message_type: MessageType::E2ePlaceholder,
            content: MessageContent { content: Some(message_content::Content::Placeholder(self.inner)) },
        }
    }
}
