//! 消息内容构建器 — 与 message_content.proto 对应，构建 MessageContent

use flare_proto::common::{
    AnnouncementContent, AudioContent, AudioInfo, CardContent, CustomContent, EmojiContent,
    FileContent, ForwardContent, GifContent, ImageContent, ImageGroupContent, ImageInfo,
    LinkCardContent, LocationContent, MarkdownContent, Mention, MentionType, MessageContent,
    MessageType, MiniProgramContent, NotificationContent, PlaceholderContent, QuoteContent,
    RichTextContent, ScheduleContent, StickerContent, SystemContent, TaskContent, TextContent,
    ThreadContent, VideoContent, VideoInfo, VoteContent,
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
        };
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Image(
                ImageContent {
                    source: Some(source),
                    thumbnail: None,
                    description: String::new(),
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
        };
        let thumbnail = ImageInfo {
            uuid: tid.clone(),
            image_id: tid,
            url: String::new(),
            mime_type: String::new(),
            size: 0,
            width: 0,
            height: 0,
        };
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Image(
                ImageContent {
                    source: Some(source),
                    thumbnail: Some(thumbnail),
                    description: String::new(),
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
                _ => {}
            }
        }
        self
    }

    pub fn location(longitude: f64, latitude: f64) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Location(
                LocationContent {
                    longitude,
                    latitude,
                    address: String::new(),
                    description: String::new(),
                    poi_id: String::new(),
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

    pub fn card(user_id: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Card(
                CardContent {
                    user_id: user_id.into(),
                    nickname: String::new(),
                    avatar_url: String::new(),
                    description: String::new(),
                    extra: std::collections::HashMap::new(),
                },
            )),
        };
        Self {
            message_type: MessageType::Card,
            content: Some(content),
        }
    }

    pub fn nickname(mut self, n: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Card(ref mut card)) =
                c.content
            {
                card.nickname = n.into();
            }
        }
        self
    }

    pub fn avatar_url(mut self, u: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Card(ref mut card)) =
                c.content
            {
                card.avatar_url = u.into();
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
                    width: 0,
                    height: 0,
                    extra: std::collections::HashMap::new(),
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
            if let Some(flare_proto::common::message_content::Content::Sticker(ref mut s)) =
                c.content
            {
                s.width = w;
                s.height = h;
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

    pub fn gif(gif_id: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Gif(
                GifContent {
                    gif_id: gif_id.into(),
                    url: String::new(),
                    thumbnail: None,
                    duration_ms: 0,
                    width: 0,
                    height: 0,
                },
            )),
        };
        Self {
            message_type: MessageType::Gif,
            content: Some(content),
        }
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
                q.current_content = if is_quote { None } else { Some(Box::new(content)) };
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

    pub fn forward(message_ids: Vec<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Forward(
                ForwardContent {
                    message_ids,
                    forward_reason: String::new(),
                    forwarded_previews: vec![],
                },
            )),
        };
        Self {
            message_type: MessageType::MergeForward,
            content: Some(content),
        }
    }

    pub fn forward_reason(mut self, s: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            if let Some(flare_proto::common::message_content::Content::Forward(ref mut f)) =
                c.content
            {
                f.forward_reason = s.into();
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

    pub fn title(mut self, t: impl Into<String>) -> Self {
        if let Some(ref mut c) = self.content {
            match c.content {
                Some(flare_proto::common::message_content::Content::LinkCard(ref mut l)) => {
                    l.title = t.into();
                }
                Some(flare_proto::common::message_content::Content::MiniProgram(ref mut m)) => {
                    m.title = t.into();
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

    pub fn rich_text(body: impl Into<String>, format: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::RichText(
                RichTextContent {
                    content: body.into(),
                    format: format.into(),
                    mentions: vec![],
                    metadata: std::collections::HashMap::new(),
                },
            )),
        };
        Self {
            message_type: MessageType::RichText,
            content: Some(content),
        }
    }

    pub fn markdown(text: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Markdown(
                MarkdownContent {
                    text: text.into(),
                    mentions: vec![],
                    metadata: std::collections::HashMap::new(),
                },
            )),
        };
        Self {
            message_type: MessageType::Markdown,
            content: Some(content),
        }
    }

    pub fn image_group(images: Vec<ImageInfo>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::ImageGroup(
                ImageGroupContent {
                    images,
                    description: String::new(),
                    metadata: std::collections::HashMap::new(),
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
                },
            )),
        };
        Self {
            message_type: MessageType::Poll,
            content: Some(content),
        }
    }

    pub fn task(task_id: impl Into<String>, title: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Task(
                TaskContent {
                    task_id: task_id.into(),
                    title: title.into(),
                    status: String::new(),
                    metadata: std::collections::HashMap::new(),
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

    pub fn schedule(schedule_id: impl Into<String>, title: impl Into<String>) -> Self {
        let content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Schedule(
                ScheduleContent {
                    schedule_id: schedule_id.into(),
                    title: title.into(),
                    start_time: None,
                    end_time: None,
                    metadata: std::collections::HashMap::new(),
                },
            )),
        };
        Self {
            message_type: MessageType::Schedule,
            content: Some(content),
        }
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
