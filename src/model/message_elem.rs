//! 消息 content 解码后的可序列化结构，供各接入层（Tauri / FFI / 其他）按消息类型获取对应结构数据。
//! 与 message_content.proto 的 Content oneof 一一对应；JSON 字段与标签为 snake_case（如 `content_type`），接入层可自行转 camelCase。

use serde::{Deserialize, Serialize};

use crate::model::decoder::DecodedContent;
use crate::util::date::{ms_to_prost_timestamp, prost_timestamp_to_ms};
use flare_proto::common::ImageFormat;
use flare_proto::common::message_content::Content as ProtoContent;

// ---------- 通用子结构 ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MentionElem {
    pub r#type: i32,
    pub user_id: String,
    pub user_ids: Vec<String>,
    pub role_id: String,
    pub start: i32,
    pub length: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageInfoElem {
    pub uuid: String,
    /// 媒体存储侧稳定 id（与 proto `ImageInfo.image_id` 一致；展示时走 GetFileUrl）
    #[serde(default)]
    pub image_id: String,
    pub url: String,
    pub mime_type: String,
    pub size: i64,
    pub width: i32,
    pub height: i32,
    /// 与 proto `ImageInfo.format` 一致（`ImageFormat` 枚举整型）
    #[serde(default)]
    pub format: i32,
    #[serde(default)]
    pub animated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct AudioInfoElem {
    pub uuid: String,
    pub url: String,
    pub mime_type: String,
    pub size: i64,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct TextElem {
    pub text: String,
    pub mentions: Vec<MentionElem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageElem {
    pub source: Option<ImageInfoElem>,
    pub thumbnail: Option<ImageInfoElem>,
    pub description: String,
    /// 动图时长（毫秒），可选
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoElem {
    pub video_id: String,
    pub source: Option<VideoInfoElem>,
    pub cover: Option<ImageInfoElem>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioElem {
    pub audio_id: String,
    pub source: Option<AudioInfoElem>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileElem {
    pub file_id: String,
    pub file_name: String,
    pub mime_type: String,
    pub file_size: i64,
    pub url: String,
    pub description: String,
}

/// 与 `LocationContent` / 业务 `LocationMessageContent` 对齐（JSON camelCase）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationElem {
    pub latitude: f64,
    pub longitude: f64,
    pub title: String,
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_local_path: Option<String>,
}

/// 与 `CardContent` 一致；JSON camelCase（如 `cardType`）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardElem {
    #[serde(default)]
    pub card_type: String,
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub avatar: String,
    #[serde(default)]
    pub extra: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StickerElem {
    pub sticker_id: String,
    #[serde(default)]
    pub package_id: String,
    pub url: String,
    pub width: i32,
    pub height: i32,
    /// 如 webp、png（JSON 键 `format`）
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub extra: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmojiElem {
    pub emoji: String,
    pub description: String,
    pub extra: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteElem {
    pub quoted_message_id: String,
    pub quoted_sender_id: String,
    pub quoted_text_preview: String,
    pub quoted_content: Option<Box<Elem>>,
    pub current_content: Option<Box<Elem>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkCardElem {
    pub url: String,
    pub title: String,
    pub description: String,
    pub thumbnail_url: String,
    pub site_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardItemElem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sender_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sender_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_message_time_ms: Option<u64>,
    pub message_type: i32,
    pub plain_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Box<Elem>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardElem {
    /// `ForwardMode` 枚举值（与 proto 一致）
    pub mode: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub items: Vec<ForwardItemElem>,
}

/// 列表/转发条目摘要：与 decoder 的 proto 预览语义对齐，供构建 `ForwardItem.plain_text`。
pub fn elem_plain_summary(elem: &Elem) -> String {
    use Elem::*;
    match elem {
        Text(t) => t.text.clone(),
        RichText(r) => {
            let title = r.title.as_deref().map(str::trim).filter(|t| !t.is_empty());
            let body = r.plain_text.trim();
            match (title, !body.is_empty()) {
                (Some(t), true) => format!("{t} {}", body),
                (Some(t), false) => t.to_string(),
                (None, true) => body.to_string(),
                (None, false) => "[富文本]".to_string(),
            }
        }
        File(f) => {
            if f.file_name.is_empty() {
                "[文件]".to_string()
            } else {
                format!("[文件] {}", f.file_name)
            }
        }
        Image(i) => {
            if image_elem_is_motion(i) {
                "[动图]".to_string()
            } else if !i.description.trim().is_empty() {
                i.description.clone()
            } else {
                "[图片]".to_string()
            }
        }
        Video(v) => {
            if !v.description.trim().is_empty() {
                v.description.clone()
            } else {
                "[视频]".to_string()
            }
        }
        Audio(a) => {
            if !a.description.trim().is_empty() {
                a.description.clone()
            } else {
                "[语音]".to_string()
            }
        }
        Location(l) => {
            let label = if !l.title.is_empty() {
                l.title.as_str()
            } else if !l.address.is_empty() {
                l.address.as_str()
            } else {
                ""
            };
            if label.is_empty() {
                "[位置]".to_string()
            } else {
                format!("[位置] {}", label)
            }
        }
        Card(c) => {
            let label = if !c.title.is_empty() {
                c.title.as_str()
            } else if !c.id.is_empty() {
                c.id.as_str()
            } else {
                ""
            };
            if label.is_empty() {
                "[名片]".to_string()
            } else {
                format!("[名片] {}", label)
            }
        }
        Sticker(_) => "[贴纸]".to_string(),
        Emoji(e) => e.emoji.clone(),
        Quote(q) => q
            .current_content
            .as_deref()
            .map(elem_plain_summary)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| q.quoted_text_preview.clone()),
        LinkCard(l) => {
            if !l.title.trim().is_empty() {
                l.title.clone()
            } else {
                "[链接]".to_string()
            }
        }
        Forward(f) => {
            let n = f.items.len();
            if n == 0 {
                "[转发]".to_string()
            } else if n == 1 {
                f.items[0].plain_text.clone()
            } else {
                format!("[转发] {n} 条消息")
            }
        }
        Thread(t) => {
            if !t.thread_title.trim().is_empty() {
                t.thread_title.clone()
            } else {
                "[话题]".to_string()
            }
        }
        MiniProgram(m) => {
            if !m.title.trim().is_empty() {
                m.title.clone()
            } else {
                "[小程序]".to_string()
            }
        }
        ImageGroup(_) => "[多图]".to_string(),
        System(s) => {
            if !s.body.trim().is_empty() {
                s.body.clone()
            } else {
                "[系统消息]".to_string()
            }
        }
        Notification(n) => {
            if !n.body.trim().is_empty() {
                n.body.clone()
            } else if !n.title.trim().is_empty() {
                n.title.clone()
            } else {
                "[通知]".to_string()
            }
        }
        Vote(_) => "[投票]".to_string(),
        Task(t) => {
            if !t.title.trim().is_empty() {
                format!("[任务] {}", t.title)
            } else {
                "[任务]".to_string()
            }
        }
        Schedule(_) => "[日程]".to_string(),
        Announcement(a) => {
            if !a.title.trim().is_empty() {
                format!("[公告] {}", a.title)
            } else {
                "[公告]".to_string()
            }
        }
        Custom(c) => {
            if !c.description.trim().is_empty() {
                c.description.clone()
            } else {
                "[自定义]".to_string()
            }
        }
        Placeholder(p) => {
            if !p.fallback_text.trim().is_empty() {
                p.fallback_text.clone()
            } else {
                "[占位]".to_string()
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadElem {
    pub thread_id: String,
    pub thread_title: String,
    pub root_content: Option<Box<Elem>>,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiniProgramElem {
    pub app_id: String,
    pub title: String,
    pub page_path: String,
    pub thumbnail_url: String,
    pub extra: std::collections::HashMap<String, String>,
}

/// 与 `RichTextContent` 一致：Rich Doc JSON 主存储 + 必填 `plain_text`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RichTextElem {
    pub doc_json: String,
    pub content_schema: String,
    pub plain_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_format_version: Option<i32>,
    #[serde(default)]
    pub source_payload: std::collections::HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// SDK 派生检索串（可选；与 proto `search_text` 对齐）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_text: Option<String>,
    /// SDK 派生渲染提示 JSON 字符串（可选）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_hints_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageGroupElem {
    pub images: Vec<ImageInfoElem>,
    pub description: String,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemElem {
    pub event_kind: String,
    pub body: String,
    pub data: std::collections::HashMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct VoteElem {
    pub vote_id: String,
    pub title: String,
    pub options: Vec<String>,
    pub metadata: std::collections::HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub participant_user_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskElem {
    pub task_id: String,
    pub title: String,
    pub status: String,
    pub metadata: std::collections::HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub participant_user_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleElem {
    pub schedule_id: String,
    pub title: String,
    /// 毫秒时间戳（与 proto `start_time_ms` / `end_time_ms` 一致）
    pub start_time: u64,
    pub end_time: u64,
    pub metadata: std::collections::HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub participant_user_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnouncementElem {
    pub title: String,
    pub body: String,
    pub pinned: bool,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomElem {
    pub r#type: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub payload: Vec<u8>,
    pub description: String,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceholderElem {
    pub reason: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub payload: Vec<u8>,
    pub fallback_text: String,
    pub metadata: std::collections::HashMap<String, String>,
}

/// 解码后的消息内容枚举；JSON 为 `#[serde(tag = "content_type", rename_all = "snake_case")]`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "content_type", rename_all = "snake_case")]
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
    Quote(QuoteElem),
    LinkCard(LinkCardElem),
    Forward(ForwardElem),
    Thread(ThreadElem),
    MiniProgram(MiniProgramElem),
    RichText(RichTextElem),
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

/// 是否按「动图」展示（GIF/APNG/animated 标记等）
pub fn image_info_elem_is_motion(s: &ImageInfoElem) -> bool {
    if s.animated {
        return true;
    }
    if s.format == ImageFormat::Gif as i32 || s.format == ImageFormat::Apng as i32 {
        return true;
    }
    let m = s.mime_type.to_lowercase();
    m.contains("gif") || m.contains("apng")
}

/// `ImageContent` 层级：源图是否为动图类
pub fn image_elem_is_motion(i: &ImageElem) -> bool {
    i.source
        .as_ref()
        .map_or(false, image_info_elem_is_motion)
}

/// 解码前 proto `ImageContent` 是否视为动图（与 [image_elem_is_motion] 语义一致）
pub fn proto_image_content_is_motion(i: &flare_proto::common::ImageContent) -> bool {
    i.source.as_ref().map_or(false, |s| {
        if s.animated {
            return true;
        }
        if s.format == ImageFormat::Gif as i32 || s.format == ImageFormat::Apng as i32 {
            return true;
        }
        let m = s.mime_type.to_lowercase();
        m.contains("gif") || m.contains("apng")
    })
}

#[inline]
fn proto_ms_to_u64(ms: i64) -> u64 {
    if ms > 0 { ms as u64 } else { 0 }
}

#[inline]
fn elem_ms_to_i64(ms: u64) -> i64 {
    i64::try_from(ms).unwrap_or(i64::MAX)
}

fn image_info_out(o: Option<&flare_proto::common::ImageInfo>) -> Option<ImageInfoElem> {
    let i = o?;
    Some(ImageInfoElem {
        uuid: i.uuid.clone(),
        image_id: i.image_id.clone(),
        url: i.url.clone(),
        mime_type: i.mime_type.clone(),
        size: i.size,
        width: i.width,
        height: i.height,
        format: i.format,
        animated: i.animated,
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
            source: image_info_out(i.source.as_ref()),
            thumbnail: image_info_out(i.thumbnail.as_ref()),
            description: i.description.clone(),
            duration_ms: i.duration_ms,
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
            latitude: l.latitude,
            longitude: l.longitude,
            title: l.title.clone(),
            address: l.address.clone(),
            zoom: l.zoom.map(|z| z.clamp(0, 255) as u8),
            snapshot_url: l.snapshot_url.clone(),
            snapshot_local_path: l.snapshot_local_path.clone(),
        }),
        ProtoContent::Card(card) => Elem::Card(CardElem {
            card_type: card.card_type.clone(),
            id: card.id.clone(),
            title: card.title.clone(),
            subtitle: card.subtitle.clone(),
            avatar: card.avatar.clone(),
            extra: card.extra.clone(),
        }),
        ProtoContent::Sticker(s) => Elem::Sticker(StickerElem {
            sticker_id: s.sticker_id.clone(),
            package_id: s.package_id.clone(),
            url: s.url.clone(),
            width: s.width,
            height: s.height,
            format: s.format.clone(),
            extra: s.extra.clone(),
        }),
        ProtoContent::Emoji(e) => Elem::Emoji(EmojiElem {
            emoji: e.emoji.clone(),
            description: e.description.clone(),
            extra: e.extra.clone(),
        }),
        ProtoContent::Quote(q) => Elem::Quote(QuoteElem {
            quoted_message_id: q.quoted_message_id.clone(),
            quoted_sender_id: q.quoted_sender_id.clone(),
            quoted_text_preview: q.quoted_text_preview.clone(),
            quoted_content: q
                .quoted_content
                .as_ref()
                .and_then(|mc| message_content_to_out(mc).map(Box::new)),
            current_content: q
                .current_content
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
            mode: f.mode,
            title: f.title.clone(),
            items: f
                .items
                .iter()
                .map(|it| ForwardItemElem {
                    source_message_id: it.source_message_id.clone(),
                    source_conversation_id: it.source_conversation_id.clone(),
                    source_sender_id: it.source_sender_id.clone(),
                    source_sender_name: it.source_sender_name.clone(),
                    source_message_time_ms: {
                        let ms = prost_timestamp_to_ms(it.source_message_time.as_ref());
                        if ms == 0 {
                            None
                        } else {
                            Some(ms)
                        }
                    },
                    message_type: it.message_type,
                    plain_text: it.plain_text.clone(),
                    content: it
                        .content
                        .as_ref()
                        .and_then(message_content_to_out)
                        .map(Box::new),
                })
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
            doc_json: r.doc_json.clone(),
            content_schema: r.content_schema.clone(),
            plain_text: r.plain_text.clone(),
            input_format: r.input_format.clone(),
            input_format_version: r.input_format_version,
            source_payload: r.source_payload.clone(),
            title: r.title.clone(),
            search_text: r.search_text.clone(),
            render_hints_json: r.render_hints_json.clone(),
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
            participant_user_ids: v.participant_user_ids.clone(),
        }),
        ProtoContent::Task(t) => Elem::Task(TaskElem {
            task_id: t.task_id.clone(),
            title: t.title.clone(),
            status: t.status.clone(),
            metadata: t.metadata.clone(),
            participant_user_ids: t.participant_user_ids.clone(),
        }),
        ProtoContent::Schedule(s) => Elem::Schedule(ScheduleElem {
            schedule_id: s.schedule_id.clone(),
            title: s.title.clone(),
            start_time: proto_ms_to_u64(s.start_time_ms),
            end_time: proto_ms_to_u64(s.end_time_ms),
            metadata: s.metadata.clone(),
            participant_user_ids: s.participant_user_ids.clone(),
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
        image_id: e.image_id.clone(),
        url: e.url.clone(),
        mime_type: e.mime_type.clone(),
        size: e.size,
        width: e.width,
        height: e.height,
        format: e.format,
        animated: e.animated,
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
            source: i.source.as_ref().map(image_info_in),
            thumbnail: i.thumbnail.as_ref().map(image_info_in),
            description: i.description.clone(),
            duration_ms: i.duration_ms,
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
            title: l.title.clone(),
            zoom: l.zoom.map(|z| i32::from(z)),
            snapshot_url: l.snapshot_url.clone(),
            snapshot_local_path: l.snapshot_local_path.clone(),
        }),
        Elem::Card(card) => ProtoContent::Card(flare_proto::common::CardContent {
            card_type: card.card_type.clone(),
            id: card.id.clone(),
            title: card.title.clone(),
            subtitle: card.subtitle.clone(),
            avatar: card.avatar.clone(),
            extra: card.extra.clone(),
        }),
        Elem::Sticker(s) => ProtoContent::Sticker(flare_proto::common::StickerContent {
            sticker_id: s.sticker_id.clone(),
            url: s.url.clone(),
            width: s.width,
            height: s.height,
            extra: s.extra.clone(),
            package_id: s.package_id.clone(),
            format: s.format.clone(),
        }),
        Elem::Emoji(e) => ProtoContent::Emoji(flare_proto::common::EmojiContent {
            emoji: e.emoji.clone(),
            description: e.description.clone(),
            extra: e.extra.clone(),
        }),
        Elem::Quote(q) => ProtoContent::Quote(Box::new(flare_proto::common::QuoteContent {
            quoted_message_id: q.quoted_message_id.clone(),
            quoted_sender_id: q.quoted_sender_id.clone(),
            quoted_text_preview: q.quoted_text_preview.clone(),
            quoted_content: q
                .quoted_content
                .as_ref()
                .map(|b| Box::new(elem_to_message_content(b))),
            current_content: q
                .current_content
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
            mode: f.mode,
            title: f.title.clone(),
            items: f
                .items
                .iter()
                .map(|it| flare_proto::common::ForwardItem {
                    source_message_id: it.source_message_id.clone(),
                    source_conversation_id: it.source_conversation_id.clone(),
                    source_sender_id: it.source_sender_id.clone(),
                    source_sender_name: it.source_sender_name.clone(),
                    source_message_time: it
                        .source_message_time_ms
                        .and_then(ms_to_prost_timestamp),
                    message_type: it.message_type,
                    plain_text: it.plain_text.clone(),
                    content: it
                        .content
                        .as_ref()
                        .map(|e| elem_to_message_content(e)),
                })
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
            doc_json: r.doc_json.clone(),
            content_schema: r.content_schema.clone(),
            plain_text: r.plain_text.clone(),
            input_format: r.input_format.clone(),
            source_payload: r.source_payload.clone(),
            input_format_version: r.input_format_version,
            title: r.title.clone(),
            search_text: r.search_text.clone(),
            render_hints_json: r.render_hints_json.clone(),
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
            participant_user_ids: v.participant_user_ids.clone(),
        }),
        Elem::Task(t) => ProtoContent::Task(flare_proto::common::TaskContent {
            task_id: t.task_id.clone(),
            title: t.title.clone(),
            status: t.status.clone(),
            metadata: t.metadata.clone(),
            participant_user_ids: t.participant_user_ids.clone(),
        }),
        Elem::Schedule(s) => ProtoContent::Schedule(flare_proto::common::ScheduleContent {
            schedule_id: s.schedule_id.clone(),
            title: s.title.clone(),
            start_time_ms: elem_ms_to_i64(s.start_time),
            end_time_ms: elem_ms_to_i64(s.end_time),
            metadata: s.metadata.clone(),
            participant_user_ids: s.participant_user_ids.clone(),
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
