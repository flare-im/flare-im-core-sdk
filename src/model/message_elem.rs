//! 消息 content 解码后的可序列化结构，供各接入层（Tauri / FFI / 其他）按消息类型获取对应结构数据。
//! 与 message_content.proto 的 Content oneof 一一对应，使用 camelCase 与 contentType 标签，便于 JSON 序列化。

use serde::{Deserialize, Serialize};

use crate::model::decoder::DecodedContent;
use crate::util::date::{ms_to_prost_timestamp, prost_timestamp_to_ms};
use flare_proto::common::message_content::Content as ProtoContent;

// ---------- 通用子结构 ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MentionElem {
    pub r#type: i32,
    pub user_id: String,
    pub user_ids: Vec<String>,
    pub role_id: String,
    pub start: i32,
    pub length: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInfoElem {
    pub uuid: String,
    pub url: String,
    pub mime_type: String,
    pub size: i64,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoInfoElem {
    pub uuid: String,
    pub url: String,
    pub mime_type: String,
    pub size: i64,
    pub duration_ms: i64,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioInfoElem {
    pub uuid: String,
    pub url: String,
    pub mime_type: String,
    pub size: i64,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePreviewElem {
    pub message_id: String,
    pub sender_id: String,
    pub r#type: i32,
    pub text: String,
    /// 毫秒时间戳
    pub time: u64,
}

// ---------- 各 Content 类型的 Elem 结构 ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextElem {
    pub text: String,
    pub mentions: Vec<MentionElem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageElem {
    pub image_id: String,
    pub source: Option<ImageInfoElem>,
    pub thumbnail: Option<ImageInfoElem>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoElem {
    pub video_id: String,
    pub source: Option<VideoInfoElem>,
    pub cover: Option<ImageInfoElem>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioElem {
    pub audio_id: String,
    pub source: Option<AudioInfoElem>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileElem {
    pub file_id: String,
    pub file_name: String,
    pub mime_type: String,
    pub file_size: i64,
    pub url: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationElem {
    pub longitude: f64,
    pub latitude: f64,
    pub address: String,
    pub description: String,
    pub poi_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardElem {
    pub user_id: String,
    pub nickname: String,
    pub avatar_url: String,
    pub description: String,
    pub extra: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StickerElem {
    pub sticker_id: String,
    pub url: String,
    pub width: i32,
    pub height: i32,
    pub extra: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmojiElem {
    pub emoji: String,
    pub description: String,
    pub extra: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GifElem {
    pub gif_id: String,
    pub url: String,
    pub thumbnail: Option<ImageInfoElem>,
    pub duration_ms: i64,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteElem {
    pub quoted_message_id: String,
    pub quoted_sender_id: String,
    pub quoted_text_preview: String,
    pub quoted_content: Option<Box<Elem>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkCardElem {
    pub url: String,
    pub title: String,
    pub description: String,
    pub thumbnail_url: String,
    pub site_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardElem {
    pub message_ids: Vec<String>,
    pub forward_reason: String,
    pub forwarded_previews: Vec<MessagePreviewElem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadElem {
    pub thread_id: String,
    pub thread_title: String,
    pub root_content: Option<Box<Elem>>,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiniProgramElem {
    pub app_id: String,
    pub title: String,
    pub page_path: String,
    pub thumbnail_url: String,
    pub extra: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RichTextElem {
    pub content: String,
    pub format: String,
    pub mentions: Vec<MentionElem>,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkdownElem {
    pub text: String,
    pub mentions: Vec<MentionElem>,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageGroupElem {
    pub images: Vec<ImageInfoElem>,
    pub description: String,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemElem {
    pub event_kind: String,
    pub body: String,
    pub data: std::collections::HashMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationElem {
    pub title: String,
    pub body: String,
    pub notification_type: String,
    pub data: std::collections::HashMap<String, String>,
    pub target_user_ids: Vec<String>,
    pub target_role_id: String,
    pub notify_all: bool,
    pub persistent: bool,
    pub show_in_list: bool,
    pub show_badge: bool,
    pub play_sound: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoteElem {
    pub vote_id: String,
    pub title: String,
    pub options: Vec<String>,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskElem {
    pub task_id: String,
    pub title: String,
    pub status: String,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleElem {
    pub schedule_id: String,
    pub title: String,
    /// 毫秒时间戳
    pub start_time: u64,
    /// 毫秒时间戳
    pub end_time: u64,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnouncementElem {
    pub title: String,
    pub body: String,
    pub pinned: bool,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomElem {
    pub r#type: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub payload: Vec<u8>,
    pub description: String,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceholderElem {
    pub reason: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub payload: Vec<u8>,
    pub fallback_text: String,
    pub metadata: std::collections::HashMap<String, String>,
}

/// 解码后的消息内容枚举，各接入层可根据 contentType 取得对应结构并序列化（如 JSON）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "contentType", rename_all = "camelCase")]
pub enum Elem {
    Text(TextElem),
    Image(ImageElem),
    Video(VideoElem),
    Audio(AudioElem),
    File(FileElem),
    Location(LocationElem),
    Card(CardElem),
    Sticker(StickerElem),
    Emoji(EmojiElem),
    Gif(GifElem),
    Quote(QuoteElem),
    LinkCard(LinkCardElem),
    Forward(ForwardElem),
    Thread(ThreadElem),
    MiniProgram(MiniProgramElem),
    RichText(RichTextElem),
    Markdown(MarkdownElem),
    ImageGroup(ImageGroupElem),
    System(SystemElem),
    Notification(NotificationElem),
    Vote(VoteElem),
    Task(TaskElem),
    Schedule(ScheduleElem),
    Announcement(AnnouncementElem),
    Custom(CustomElem),
    Placeholder(PlaceholderElem),
}

fn image_info_out(o: Option<&flare_proto::common::ImageInfo>) -> Option<ImageInfoElem> {
    let i = o?;
    Some(ImageInfoElem {
        uuid: i.uuid.clone(),
        url: i.url.clone(),
        mime_type: i.mime_type.clone(),
        size: i.size,
        width: i.width,
        height: i.height,
    })
}

fn video_info_out(o: Option<&flare_proto::common::VideoInfo>) -> Option<VideoInfoElem> {
    let v = o?;
    Some(VideoInfoElem {
        uuid: v.uuid.clone(),
        url: v.url.clone(),
        mime_type: v.mime_type.clone(),
        size: v.size,
        duration_ms: v.duration_ms,
        width: v.width,
        height: v.height,
    })
}

fn audio_info_out(o: Option<&flare_proto::common::AudioInfo>) -> Option<AudioInfoElem> {
    let a = o?;
    Some(AudioInfoElem {
        uuid: a.uuid.clone(),
        url: a.url.clone(),
        mime_type: a.mime_type.clone(),
        size: a.size,
        duration_ms: a.duration_ms,
    })
}

fn mention_out(m: &flare_proto::common::Mention) -> MentionElem {
    MentionElem {
        r#type: m.r#type,
        user_id: m.user_id.clone(),
        user_ids: m.user_ids.clone(),
        role_id: m.role_id.clone(),
        start: m.start,
        length: m.length,
    }
}

fn message_preview_out(p: &flare_proto::common::MessagePreview) -> MessagePreviewElem {
    message_preview_from_proto(p)
}

/// 从 proto MessagePreview 转为可序列化的 MessagePreviewElem（供会话列表等使用）
pub fn message_preview_from_proto(p: &flare_proto::common::MessagePreview) -> MessagePreviewElem {
    MessagePreviewElem {
        message_id: p.message_id.clone(),
        sender_id: p.sender_id.clone(),
        r#type: p.r#type,
        text: p.text.clone(),
        time: prost_timestamp_to_ms(p.time.as_ref()),
    }
}

/// MessagePreviewElem 转为 proto MessagePreview（持久化/回写用）
pub fn message_preview_to_proto(e: &MessagePreviewElem) -> flare_proto::common::MessagePreview {
    let time = ms_to_prost_timestamp(e.time);
    flare_proto::common::MessagePreview {
        message_id: e.message_id.clone(),
        sender_id: e.sender_id.clone(),
        r#type: e.r#type,
        text: e.text.clone(),
        time,
    }
}

fn message_content_to_out(mc: &flare_proto::common::MessageContent) -> Option<Elem> {
    let c = mc.content.as_ref()?;
    proto_content_to_out(c)
}

fn proto_content_to_out(c: &ProtoContent) -> Option<Elem> {
    Some(match c {
        ProtoContent::Text(t) => Elem::Text(TextElem {
            text: t.text.clone(),
            mentions: t.mentions.iter().map(mention_out).collect(),
        }),
        ProtoContent::Image(i) => Elem::Image(ImageElem {
            image_id: i.image_id.clone(),
            source: image_info_out(i.source.as_ref()),
            thumbnail: image_info_out(i.thumbnail.as_ref()),
            description: i.description.clone(),
        }),
        ProtoContent::Video(v) => Elem::Video(VideoElem {
            video_id: v.video_id.clone(),
            source: video_info_out(v.source.as_ref()),
            cover: image_info_out(v.cover.as_ref()),
            description: v.description.clone(),
        }),
        ProtoContent::Audio(a) => Elem::Audio(AudioElem {
            audio_id: a.audio_id.clone(),
            source: audio_info_out(a.source.as_ref()),
            description: a.description.clone(),
        }),
        ProtoContent::File(f) => Elem::File(FileElem {
            file_id: f.file_id.clone(),
            file_name: f.file_name.clone(),
            mime_type: f.mime_type.clone(),
            file_size: f.file_size,
            url: f.url.clone(),
            description: f.description.clone(),
        }),
        ProtoContent::Location(l) => Elem::Location(LocationElem {
            longitude: l.longitude,
            latitude: l.latitude,
            address: l.address.clone(),
            description: l.description.clone(),
            poi_id: l.poi_id.clone(),
        }),
        ProtoContent::Card(card) => Elem::Card(CardElem {
            user_id: card.user_id.clone(),
            nickname: card.nickname.clone(),
            avatar_url: card.avatar_url.clone(),
            description: card.description.clone(),
            extra: card.extra.clone(),
        }),
        ProtoContent::Sticker(s) => Elem::Sticker(StickerElem {
            sticker_id: s.sticker_id.clone(),
            url: s.url.clone(),
            width: s.width,
            height: s.height,
            extra: s.extra.clone(),
        }),
        ProtoContent::Emoji(e) => Elem::Emoji(EmojiElem {
            emoji: e.emoji.clone(),
            description: e.description.clone(),
            extra: e.extra.clone(),
        }),
        ProtoContent::Gif(g) => Elem::Gif(GifElem {
            gif_id: g.gif_id.clone(),
            url: g.url.clone(),
            thumbnail: image_info_out(g.thumbnail.as_ref()),
            duration_ms: g.duration_ms,
            width: g.width,
            height: g.height,
        }),
        ProtoContent::Quote(q) => Elem::Quote(QuoteElem {
            quoted_message_id: q.quoted_message_id.clone(),
            quoted_sender_id: q.quoted_sender_id.clone(),
            quoted_text_preview: q.quoted_text_preview.clone(),
            quoted_content: q
                .quoted_content
                .as_ref()
                .and_then(|mc| message_content_to_out(mc).map(Box::new)),
        }),
        ProtoContent::LinkCard(l) => Elem::LinkCard(LinkCardElem {
            url: l.url.clone(),
            title: l.title.clone(),
            description: l.description.clone(),
            thumbnail_url: l.thumbnail_url.clone(),
            site_name: l.site_name.clone(),
        }),
        ProtoContent::Forward(f) => Elem::Forward(ForwardElem {
            message_ids: f.message_ids.clone(),
            forward_reason: f.forward_reason.clone(),
            forwarded_previews: f
                .forwarded_previews
                .iter()
                .map(message_preview_out)
                .collect(),
        }),
        ProtoContent::Thread(t) => Elem::Thread(ThreadElem {
            thread_id: t.thread_id.clone(),
            thread_title: t.thread_title.clone(),
            root_content: t
                .root_content
                .as_ref()
                .and_then(|mc| message_content_to_out(mc).map(Box::new)),
            metadata: t.metadata.clone(),
        }),
        ProtoContent::MiniProgram(m) => Elem::MiniProgram(MiniProgramElem {
            app_id: m.app_id.clone(),
            title: m.title.clone(),
            page_path: m.page_path.clone(),
            thumbnail_url: m.thumbnail_url.clone(),
            extra: m.extra.clone(),
        }),
        ProtoContent::RichText(r) => Elem::RichText(RichTextElem {
            content: r.content.clone(),
            format: r.format.clone(),
            mentions: r.mentions.iter().map(mention_out).collect(),
            metadata: r.metadata.clone(),
        }),
        ProtoContent::Markdown(m) => Elem::Markdown(MarkdownElem {
            text: m.text.clone(),
            mentions: m.mentions.iter().map(mention_out).collect(),
            metadata: m.metadata.clone(),
        }),
        ProtoContent::ImageGroup(ig) => Elem::ImageGroup(ImageGroupElem {
            images: ig
                .images
                .iter()
                .filter_map(|i| image_info_out(Some(i)))
                .collect(),
            description: ig.description.clone(),
            metadata: ig.metadata.clone(),
        }),
        ProtoContent::System(s) => Elem::System(SystemElem {
            event_kind: s.event_kind.clone(),
            body: s.body.clone(),
            data: s.data.clone(),
            payload: s.payload.clone(),
        }),
        ProtoContent::Notification(n) => Elem::Notification(NotificationElem {
            title: n.title.clone(),
            body: n.body.clone(),
            notification_type: n.notification_type.clone(),
            data: n.data.clone(),
            target_user_ids: n.target_user_ids.clone(),
            target_role_id: n.target_role_id.clone(),
            notify_all: n.notify_all,
            persistent: n.persistent,
            show_in_list: n.show_in_list,
            show_badge: n.show_badge,
            play_sound: n.play_sound,
        }),
        ProtoContent::Vote(v) => Elem::Vote(VoteElem {
            vote_id: v.vote_id.clone(),
            title: v.title.clone(),
            options: v.options.clone(),
            metadata: v.metadata.clone(),
        }),
        ProtoContent::Task(t) => Elem::Task(TaskElem {
            task_id: t.task_id.clone(),
            title: t.title.clone(),
            status: t.status.clone(),
            metadata: t.metadata.clone(),
        }),
        ProtoContent::Schedule(s) => Elem::Schedule(ScheduleElem {
            schedule_id: s.schedule_id.clone(),
            title: s.title.clone(),
            start_time: prost_timestamp_to_ms(s.start_time.as_ref()),
            end_time: prost_timestamp_to_ms(s.end_time.as_ref()),
            metadata: s.metadata.clone(),
        }),
        ProtoContent::Announcement(a) => Elem::Announcement(AnnouncementElem {
            title: a.title.clone(),
            body: a.body.clone(),
            pinned: a.pinned,
            metadata: a.metadata.clone(),
        }),
        ProtoContent::Custom(c) => Elem::Custom(CustomElem {
            r#type: c.r#type.clone(),
            payload: c.payload.clone(),
            description: c.description.clone(),
            metadata: c.metadata.clone(),
        }),
        ProtoContent::Placeholder(p) => Elem::Placeholder(PlaceholderElem {
            reason: p.reason.clone(),
            payload: p.payload.clone(),
            fallback_text: p.fallback_text.clone(),
            metadata: p.metadata.clone(),
        }),
    })
}

fn mention_in(m: &MentionElem) -> flare_proto::common::Mention {
    flare_proto::common::Mention {
        r#type: m.r#type,
        user_id: m.user_id.clone(),
        user_ids: m.user_ids.clone(),
        role_id: m.role_id.clone(),
        start: m.start,
        length: m.length,
    }
}

fn image_info_in(e: &ImageInfoElem) -> flare_proto::common::ImageInfo {
    flare_proto::common::ImageInfo {
        uuid: e.uuid.clone(),
        url: e.url.clone(),
        mime_type: e.mime_type.clone(),
        size: e.size,
        width: e.width,
        height: e.height,
    }
}

fn video_info_in(v: &VideoInfoElem) -> flare_proto::common::VideoInfo {
    flare_proto::common::VideoInfo {
        uuid: v.uuid.clone(),
        url: v.url.clone(),
        mime_type: v.mime_type.clone(),
        size: v.size,
        duration_ms: v.duration_ms,
        width: v.width,
        height: v.height,
    }
}

fn audio_info_in(a: &AudioInfoElem) -> flare_proto::common::AudioInfo {
    flare_proto::common::AudioInfo {
        uuid: a.uuid.clone(),
        url: a.url.clone(),
        mime_type: a.mime_type.clone(),
        size: a.size,
        duration_ms: a.duration_ms,
    }
}

/// `Elem` → `message_content.proto` 的 `Content` oneof（与 [proto_content_to_out] 互逆）。
fn elem_to_proto_content(elem: &Elem) -> ProtoContent {
    match elem {
        Elem::Text(t) => ProtoContent::Text(flare_proto::common::TextContent {
            text: t.text.clone(),
            mentions: t.mentions.iter().map(mention_in).collect(),
        }),
        Elem::Image(i) => ProtoContent::Image(flare_proto::common::ImageContent {
            image_id: i.image_id.clone(),
            source: i.source.as_ref().map(image_info_in),
            thumbnail: i.thumbnail.as_ref().map(image_info_in),
            description: i.description.clone(),
        }),
        Elem::Video(v) => ProtoContent::Video(flare_proto::common::VideoContent {
            video_id: v.video_id.clone(),
            source: v.source.as_ref().map(video_info_in),
            cover: v.cover.as_ref().map(image_info_in),
            description: v.description.clone(),
        }),
        Elem::Audio(a) => ProtoContent::Audio(flare_proto::common::AudioContent {
            audio_id: a.audio_id.clone(),
            source: a.source.as_ref().map(audio_info_in),
            description: a.description.clone(),
        }),
        Elem::File(f) => ProtoContent::File(flare_proto::common::FileContent {
            file_id: f.file_id.clone(),
            file_name: f.file_name.clone(),
            mime_type: f.mime_type.clone(),
            file_size: f.file_size,
            url: f.url.clone(),
            description: f.description.clone(),
        }),
        Elem::Location(l) => ProtoContent::Location(flare_proto::common::LocationContent {
            longitude: l.longitude,
            latitude: l.latitude,
            address: l.address.clone(),
            description: l.description.clone(),
            poi_id: l.poi_id.clone(),
        }),
        Elem::Card(card) => ProtoContent::Card(flare_proto::common::CardContent {
            user_id: card.user_id.clone(),
            nickname: card.nickname.clone(),
            avatar_url: card.avatar_url.clone(),
            description: card.description.clone(),
            extra: card.extra.clone(),
        }),
        Elem::Sticker(s) => ProtoContent::Sticker(flare_proto::common::StickerContent {
            sticker_id: s.sticker_id.clone(),
            url: s.url.clone(),
            width: s.width,
            height: s.height,
            extra: s.extra.clone(),
        }),
        Elem::Emoji(e) => ProtoContent::Emoji(flare_proto::common::EmojiContent {
            emoji: e.emoji.clone(),
            description: e.description.clone(),
            extra: e.extra.clone(),
        }),
        Elem::Gif(g) => ProtoContent::Gif(flare_proto::common::GifContent {
            gif_id: g.gif_id.clone(),
            url: g.url.clone(),
            thumbnail: g.thumbnail.as_ref().map(image_info_in),
            duration_ms: g.duration_ms,
            width: g.width,
            height: g.height,
        }),
        Elem::Quote(q) => ProtoContent::Quote(Box::new(flare_proto::common::QuoteContent {
            quoted_message_id: q.quoted_message_id.clone(),
            quoted_sender_id: q.quoted_sender_id.clone(),
            quoted_text_preview: q.quoted_text_preview.clone(),
            quoted_content: q
                .quoted_content
                .as_ref()
                .map(|b| Box::new(elem_to_message_content(b))),
        })),
        Elem::LinkCard(l) => ProtoContent::LinkCard(flare_proto::common::LinkCardContent {
            url: l.url.clone(),
            title: l.title.clone(),
            description: l.description.clone(),
            thumbnail_url: l.thumbnail_url.clone(),
            site_name: l.site_name.clone(),
        }),
        Elem::Forward(f) => ProtoContent::Forward(flare_proto::common::ForwardContent {
            message_ids: f.message_ids.clone(),
            forward_reason: f.forward_reason.clone(),
            forwarded_previews: f
                .forwarded_previews
                .iter()
                .map(message_preview_to_proto)
                .collect(),
        }),
        Elem::Thread(te) => ProtoContent::Thread(Box::new(flare_proto::common::ThreadContent {
            thread_id: te.thread_id.clone(),
            thread_title: te.thread_title.clone(),
            root_content: te
                .root_content
                .as_ref()
                .map(|b| Box::new(elem_to_message_content(b))),
            metadata: te.metadata.clone(),
        })),
        Elem::MiniProgram(m) => ProtoContent::MiniProgram(flare_proto::common::MiniProgramContent {
            app_id: m.app_id.clone(),
            title: m.title.clone(),
            page_path: m.page_path.clone(),
            thumbnail_url: m.thumbnail_url.clone(),
            extra: m.extra.clone(),
        }),
        Elem::RichText(r) => ProtoContent::RichText(flare_proto::common::RichTextContent {
            content: r.content.clone(),
            format: r.format.clone(),
            mentions: r.mentions.iter().map(mention_in).collect(),
            metadata: r.metadata.clone(),
        }),
        Elem::Markdown(m) => ProtoContent::Markdown(flare_proto::common::MarkdownContent {
            text: m.text.clone(),
            mentions: m.mentions.iter().map(mention_in).collect(),
            metadata: m.metadata.clone(),
        }),
        Elem::ImageGroup(ig) => ProtoContent::ImageGroup(flare_proto::common::ImageGroupContent {
            images: ig.images.iter().map(image_info_in).collect(),
            description: ig.description.clone(),
            metadata: ig.metadata.clone(),
        }),
        Elem::System(s) => ProtoContent::System(flare_proto::common::SystemContent {
            event_kind: s.event_kind.clone(),
            body: s.body.clone(),
            data: s.data.clone(),
            payload: s.payload.clone(),
        }),
        Elem::Notification(n) => {
            ProtoContent::Notification(flare_proto::common::NotificationContent {
                title: n.title.clone(),
                body: n.body.clone(),
                notification_type: n.notification_type.clone(),
                data: n.data.clone(),
                target_user_ids: n.target_user_ids.clone(),
                target_role_id: n.target_role_id.clone(),
                notify_all: n.notify_all,
                persistent: n.persistent,
                show_in_list: n.show_in_list,
                show_badge: n.show_badge,
                play_sound: n.play_sound,
            })
        },
        Elem::Vote(v) => ProtoContent::Vote(flare_proto::common::VoteContent {
            vote_id: v.vote_id.clone(),
            title: v.title.clone(),
            options: v.options.clone(),
            metadata: v.metadata.clone(),
        }),
        Elem::Task(t) => ProtoContent::Task(flare_proto::common::TaskContent {
            task_id: t.task_id.clone(),
            title: t.title.clone(),
            status: t.status.clone(),
            metadata: t.metadata.clone(),
        }),
        Elem::Schedule(s) => ProtoContent::Schedule(flare_proto::common::ScheduleContent {
            schedule_id: s.schedule_id.clone(),
            title: s.title.clone(),
            start_time: ms_to_prost_timestamp(s.start_time),
            end_time: ms_to_prost_timestamp(s.end_time),
            metadata: s.metadata.clone(),
        }),
        Elem::Announcement(a) => ProtoContent::Announcement(flare_proto::common::AnnouncementContent {
            title: a.title.clone(),
            body: a.body.clone(),
            pinned: a.pinned,
            metadata: a.metadata.clone(),
        }),
        Elem::Custom(c) => ProtoContent::Custom(flare_proto::common::CustomContent {
            r#type: c.r#type.clone(),
            payload: c.payload.clone(),
            description: c.description.clone(),
            metadata: c.metadata.clone(),
        }),
        Elem::Placeholder(p) => ProtoContent::Placeholder(flare_proto::common::PlaceholderContent {
            reason: p.reason.clone(),
            payload: p.payload.clone(),
            fallback_text: p.fallback_text.clone(),
            metadata: p.metadata.clone(),
        }),
    }
}

/// 将展示用 `Elem` 编码为协议层 `MessageContent`。
///
/// 用于 `IMMessage.content_bytes` 为空但仍有解码后 `content` 的场景（例如 Tauri：`content_bytes` 对 JSON `skip`，`sdk_send` 仅回传 `content`）。
pub fn elem_to_message_content(elem: &Elem) -> flare_proto::common::MessageContent {
    flare_proto::common::MessageContent {
        content: Some(elem_to_proto_content(elem)),
    }
}

/// 从 DecodedContent 转为可序列化结构（供 Tauri/FFI/其他接入层序列化为 JSON 等）；解析失败或 Unknown 返回 None。
pub fn decoded_content_to_elem(decoded: &DecodedContent) -> Option<Elem> {
    let c = decoded.as_content()?;
    proto_content_to_out(c)
}
