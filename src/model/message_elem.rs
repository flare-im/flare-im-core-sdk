//! 消息 content 解码后的可序列化结构，供各接入层（Tauri / FFI / 其他）按消息类型获取对应结构数据。
//! 与 message_content.proto 的 Content oneof 一一对应；JSON 字段为 camelCase（如 `contentType`），接入层不再做命名转换。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::model::decoder::DecodedContent;
use crate::model::preview_storage::{
    PreviewStoragePayload, decode_or_user_text, keys, localizable_notification_preview,
    localizable_system_preview,
};
use flare_proto::common::ImageFormat;
use flare_proto::common::message_content::Content as ProtoContent;

const APP_CARD_MINI_PROGRAM: &str = "mini_program";
const APP_CARD_VOTE: &str = "vote";
const APP_CARD_TASK: &str = "task";
const APP_CARD_SCHEDULE: &str = "schedule";
const APP_CARD_ANNOUNCEMENT: &str = "announcement";

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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub source: Option<ImageInfoElem>,
    pub thumbnail: Option<ImageInfoElem>,
    pub description: String,
    /// 动图时长（毫秒），可选
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
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

/// 与 `LocationContent` / 业务 `LocationMessageContent` 对齐（JSON camelCase）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub struct EmojiElem {
    pub emoji: String,
    pub description: String,
    pub extra: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteElem {
    pub quoted_message_id: String,
    pub quoted_sender_id: String,
    pub quoted_text_preview: String,
    pub quoted_content: Option<Box<Elem>>,
    pub current_content: Option<Box<Elem>>,
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
#[serde(rename_all = "camelCase")]
pub struct ForwardElem {
    /// `ForwardMode` 枚举值（与 proto 一致）
    pub mode: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub items: Vec<ForwardItemElem>,
}

/// 将 [`Elem`] 转为存储用 i18n 载荷（稳定 `k` + 参数 `a`）。
pub fn elem_preview_storage_payload(elem: &Elem) -> PreviewStoragePayload {
    use Elem::*;
    match elem {
        Text(t) => {
            let mut a = Map::new();
            a.insert("t".into(), Value::String(t.text.clone()));
            PreviewStoragePayload {
                k: keys::USER_TEXT.to_string(),
                a,
            }
        }
        RichText(r) => {
            let mut a = Map::new();
            if let Some(title) = r.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                a.insert("title".into(), Value::String(title.to_string()));
            }
            let body = r.plain_text.trim();
            if !body.is_empty() {
                a.insert("body".into(), Value::String(body.to_string()));
            }
            PreviewStoragePayload {
                k: keys::RICH_TEXT.to_string(),
                a,
            }
        }
        File(f) => {
            let mut a = Map::new();
            if !f.file_name.is_empty() {
                a.insert("n".into(), Value::String(f.file_name.clone()));
            }
            PreviewStoragePayload {
                k: keys::FILE.to_string(),
                a,
            }
        }
        Image(i) => {
            let mut a = Map::new();
            if image_elem_is_motion(i) {
                a.insert("m".into(), Value::Bool(true));
            }
            if !i.description.trim().is_empty() {
                a.insert("d".into(), Value::String(i.description.clone()));
            }
            PreviewStoragePayload {
                k: keys::IMAGE.to_string(),
                a,
            }
        }
        Video(v) => {
            let mut a = Map::new();
            if !v.description.trim().is_empty() {
                a.insert("d".into(), Value::String(v.description.clone()));
            }
            PreviewStoragePayload {
                k: keys::VIDEO.to_string(),
                a,
            }
        }
        Audio(aud) => {
            let mut a = Map::new();
            if !aud.description.trim().is_empty() {
                a.insert("d".into(), Value::String(aud.description.clone()));
            }
            PreviewStoragePayload {
                k: keys::AUDIO.to_string(),
                a,
            }
        }
        Location(l) => {
            let mut a = Map::new();
            let label = if !l.title.is_empty() {
                l.title.as_str()
            } else if !l.address.is_empty() {
                l.address.as_str()
            } else {
                ""
            };
            if !label.is_empty() {
                a.insert("label".into(), Value::String(label.to_string()));
            }
            PreviewStoragePayload {
                k: keys::LOCATION.to_string(),
                a,
            }
        }
        Card(c) => {
            let mut a = Map::new();
            let label = if !c.title.is_empty() {
                c.title.as_str()
            } else if !c.id.is_empty() {
                c.id.as_str()
            } else {
                ""
            };
            if !label.is_empty() {
                a.insert("label".into(), Value::String(label.to_string()));
            }
            PreviewStoragePayload {
                k: keys::CARD.to_string(),
                a,
            }
        }
        Sticker(_) => PreviewStoragePayload {
            k: keys::STICKER.to_string(),
            a: Map::new(),
        },
        Emoji(e) => {
            let mut a = Map::new();
            a.insert("e".into(), Value::String(e.emoji.clone()));
            PreviewStoragePayload {
                k: keys::EMOJI.to_string(),
                a,
            }
        }
        Quote(q) => {
            if let Some(ref c) = q.current_content {
                let inner = elem_preview_storage_payload(c);
                if !inner.is_empty_for_last_preview() {
                    let mut a = Map::new();
                    a.insert(
                        "inner".into(),
                        serde_json::to_value(&inner).unwrap_or(Value::Null),
                    );
                    return PreviewStoragePayload {
                        k: keys::QUOTE.to_string(),
                        a,
                    };
                }
            }
            if !q.quoted_text_preview.trim().is_empty() {
                decode_or_user_text(&q.quoted_text_preview)
            } else {
                PreviewStoragePayload {
                    k: keys::QUOTE.to_string(),
                    a: Map::new(),
                }
            }
        }
        LinkCard(l) => {
            let mut a = Map::new();
            if !l.title.trim().is_empty() {
                a.insert("t".into(), Value::String(l.title.clone()));
            }
            PreviewStoragePayload {
                k: keys::LINK.to_string(),
                a,
            }
        }
        Forward(f) => {
            let n = f.items.len();
            if n == 0 {
                PreviewStoragePayload {
                    k: keys::FORWARD_EMPTY.to_string(),
                    a: Map::new(),
                }
            } else if n == 1 {
                if let Some(ref c) = f.items[0].content {
                    elem_preview_storage_payload(c)
                } else {
                    decode_or_user_text(&f.items[0].plain_text)
                }
            } else {
                let mut a = Map::new();
                a.insert("n".into(), json!(n as u64));
                if let Some(ref c) = f.items[0].content {
                    let first = elem_preview_storage_payload(c);
                    if let Ok(v) = serde_json::to_value(&first) {
                        a.insert("first".into(), v);
                    }
                } else if !f.items[0].plain_text.is_empty() {
                    let first = decode_or_user_text(&f.items[0].plain_text);
                    if let Ok(v) = serde_json::to_value(&first) {
                        a.insert("first".into(), v);
                    }
                }
                PreviewStoragePayload {
                    k: keys::FORWARD_MANY.to_string(),
                    a,
                }
            }
        }
        Thread(t) => {
            let mut a = Map::new();
            if !t.thread_title.trim().is_empty() {
                a.insert("t".into(), Value::String(t.thread_title.clone()));
            }
            PreviewStoragePayload {
                k: keys::THREAD.to_string(),
                a,
            }
        }
        MiniProgram(m) => {
            let mut a = Map::new();
            if !m.title.trim().is_empty() {
                a.insert("t".into(), Value::String(m.title.clone()));
            }
            PreviewStoragePayload {
                k: keys::MINI_PROGRAM.to_string(),
                a,
            }
        }
        ImageGroup(_) => PreviewStoragePayload {
            k: keys::IMAGE_GROUP.to_string(),
            a: Map::new(),
        },
        System(s) => localizable_system_preview(&s.event_kind, &s.body, &s.data),
        Notification(n) => {
            localizable_notification_preview(&n.notification_type, &n.title, &n.body, &n.data)
        }
        Vote(_) => PreviewStoragePayload {
            k: keys::VOTE.to_string(),
            a: Map::new(),
        },
        Task(t) => {
            let mut a = Map::new();
            if !t.title.trim().is_empty() {
                a.insert("t".into(), Value::String(t.title.clone()));
            }
            PreviewStoragePayload {
                k: keys::TASK.to_string(),
                a,
            }
        }
        Schedule(_) => PreviewStoragePayload {
            k: keys::SCHEDULE.to_string(),
            a: Map::new(),
        },
        Announcement(ann) => {
            let mut map = Map::new();
            if !ann.title.trim().is_empty() {
                map.insert("t".into(), Value::String(ann.title.clone()));
            }
            PreviewStoragePayload {
                k: keys::ANNOUNCEMENT.to_string(),
                a: map,
            }
        }
        Custom(c) => {
            let mut a = Map::new();
            if !c.description.trim().is_empty() {
                a.insert("d".into(), Value::String(c.description.clone()));
            }
            PreviewStoragePayload {
                k: keys::CUSTOM.to_string(),
                a,
            }
        }
        Placeholder(p) => {
            let mut a = Map::new();
            if !p.fallback_text.trim().is_empty() {
                a.insert("t".into(), Value::String(p.fallback_text.clone()));
            }
            PreviewStoragePayload {
                k: keys::PLACEHOLDER.to_string(),
                a,
            }
        }
    }
}

/// 列表/转发条目摘要：JSON 字符串形态，与 [`elem_preview_storage_payload`] 一致；供 `ForwardItem.plain_text`、`quote_preview` 等持久化。
pub fn elem_plain_summary(elem: &Elem) -> String {
    serde_json::to_string(&elem_preview_storage_payload(elem)).unwrap_or_default()
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

/// 与 `RichTextContent` 一致：Rich Doc JSON 主存储 + 必填 `plain_text`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub participant_user_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskElem {
    pub task_id: String,
    pub title: String,
    pub status: String,
    pub metadata: std::collections::HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub participant_user_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

/// 解码后的消息内容枚举；JSON 为 `#[serde(tag = "contentType", rename_all = "snake_case")]`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "contentType", rename_all = "snake_case")]
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
    i.source.as_ref().is_some_and(image_info_elem_is_motion)
}

/// 解码前 proto `ImageContent` 是否视为动图（与 [image_elem_is_motion] 语义一致）
pub fn proto_image_content_is_motion(i: &flare_proto::common::ImageContent) -> bool {
    i.source.as_ref().is_some_and(|s| {
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

fn split_attr_list(value: Option<&String>) -> Vec<String> {
    value
        .map(|s| {
            s.lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn app_card_payload(card: &flare_proto::common::AppCardContent) -> Map<String, Value> {
    serde_json::from_slice::<Value>(&card.payload)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn payload_string(payload: &Map<String, Value>, key: &str, fallback: Option<&String>) -> String {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| fallback.cloned())
        .unwrap_or_default()
}

fn payload_string_vec(
    payload: &Map<String, Value>,
    key: &str,
    fallback: Option<&String>,
) -> Vec<String> {
    payload
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_else(|| split_attr_list(fallback))
}

fn payload_u64(payload: &Map<String, Value>, key: &str, fallback: Option<&String>) -> u64 {
    payload
        .get(key)
        .and_then(Value::as_u64)
        .or_else(|| fallback.and_then(|s| s.parse::<u64>().ok()))
        .unwrap_or_default()
}

fn payload_bool(payload: &Map<String, Value>, key: &str, fallback: Option<&String>) -> bool {
    payload
        .get(key)
        .and_then(Value::as_bool)
        .or_else(|| fallback.map(|v| v == "true" || v == "1"))
        .unwrap_or(false)
}

fn app_card_to_elem(card: &flare_proto::common::AppCardContent) -> Elem {
    let payload = app_card_payload(card);
    match card.card_type.as_str() {
        APP_CARD_MINI_PROGRAM => {
            let mut extra = card.attributes.clone();
            let page_path = extra.remove("page_path").unwrap_or_default();
            Elem::MiniProgram(MiniProgramElem {
                app_id: card.app_id.clone(),
                title: card.title.clone(),
                page_path: payload_string(&payload, "page_path", Some(&page_path)),
                thumbnail_url: card.thumbnail_url.clone(),
                extra,
            })
        }
        APP_CARD_VOTE => Elem::Vote(VoteElem {
            vote_id: payload_string(&payload, "vote_id", card.attributes.get("vote_id")),
            title: card.title.clone(),
            options: payload_string_vec(&payload, "options", card.attributes.get("options")),
            metadata: card.attributes.clone(),
            participant_user_ids: payload_string_vec(
                &payload,
                "participant_user_ids",
                card.attributes.get("participant_user_ids"),
            ),
        }),
        APP_CARD_TASK => Elem::Task(TaskElem {
            task_id: payload_string(&payload, "task_id", card.attributes.get("task_id")),
            title: card.title.clone(),
            status: payload_string(&payload, "status", card.attributes.get("status")),
            metadata: card.attributes.clone(),
            participant_user_ids: payload_string_vec(
                &payload,
                "participant_user_ids",
                card.attributes.get("participant_user_ids"),
            ),
        }),
        APP_CARD_SCHEDULE => Elem::Schedule(ScheduleElem {
            schedule_id: payload_string(
                &payload,
                "schedule_id",
                card.attributes.get("schedule_id"),
            ),
            title: card.title.clone(),
            start_time: payload_u64(
                &payload,
                "start_time_ms",
                card.attributes.get("start_time_ms"),
            ),
            end_time: payload_u64(&payload, "end_time_ms", card.attributes.get("end_time_ms")),
            metadata: card.attributes.clone(),
            participant_user_ids: payload_string_vec(
                &payload,
                "participant_user_ids",
                card.attributes.get("participant_user_ids"),
            ),
        }),
        APP_CARD_ANNOUNCEMENT => Elem::Announcement(AnnouncementElem {
            title: card.title.clone(),
            body: card.summary.clone(),
            pinned: payload_bool(&payload, "pinned", card.attributes.get("pinned")),
            metadata: card.attributes.clone(),
        }),
        _ => Elem::Custom(CustomElem {
            r#type: card.card_type.clone(),
            payload: card.payload.clone(),
            description: if card.summary.is_empty() {
                card.title.clone()
            } else {
                card.summary.clone()
            },
            metadata: card.attributes.clone(),
        }),
    }
}

fn app_card_content(
    app_id: impl Into<String>,
    card_type: impl Into<String>,
    title: impl Into<String>,
) -> flare_proto::common::AppCardContent {
    flare_proto::common::AppCardContent {
        app_id: app_id.into(),
        card_type: card_type.into(),
        title: title.into(),
        subtitle: String::new(),
        summary: String::new(),
        thumbnail_url: String::new(),
        payload_schema: String::new(),
        payload: Vec::new(),
        actions: Vec::new(),
        attributes: std::collections::HashMap::new(),
    }
}

fn set_app_card_json_payload(card: &mut flare_proto::common::AppCardContent, payload: Value) {
    card.payload_schema = "json".to_string();
    card.payload = serde_json::to_vec(&payload).unwrap_or_default();
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
        time: proto_ms_to_u64(p.created_at),
    }
}

/// MessagePreviewElem 转为 proto MessagePreview（持久化/回写用）
pub fn message_preview_to_proto(e: &MessagePreviewElem) -> flare_proto::common::MessagePreview {
    flare_proto::common::MessagePreview {
        message_id: e.message_id.clone(),
        sender_id: e.sender_id.clone(),
        r#type: e.r#type,
        text: e.text.clone(),
        created_at: elem_ms_to_i64(e.time),
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
            extra: card.attributes.clone(),
        }),
        ProtoContent::Sticker(s) => Elem::Sticker(StickerElem {
            sticker_id: s.sticker_id.clone(),
            package_id: s.package_id.clone(),
            url: s.url.clone(),
            width: s.width,
            height: s.height,
            format: s.format.clone(),
            extra: s.attributes.clone(),
        }),
        ProtoContent::Emoji(e) => Elem::Emoji(EmojiElem {
            emoji: e.emoji.clone(),
            description: e.description.clone(),
            extra: e.attributes.clone(),
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
                        it.source_message_created_at.and_then(|ms| {
                            let ms = proto_ms_to_u64(ms);
                            if ms == 0 { None } else { Some(ms) }
                        })
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
            metadata: t.attributes.clone(),
        }),
        ProtoContent::AppCard(a) => app_card_to_elem(a),
        ProtoContent::RichText(r) => Elem::RichText(RichTextElem {
            doc_json: r.doc_json.clone(),
            content_schema: r.content_schema.clone(),
            plain_text: r.plain_text.clone(),
            input_format: r.input_format.clone(),
            input_format_version: r.input_format_version,
            source_payload: r.source_attributes.clone(),
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
            metadata: ig.attributes.clone(),
        }),
        ProtoContent::System(s) => Elem::System(SystemElem {
            event_kind: s.event_kind.clone(),
            body: s.body.clone(),
            data: s.attributes.clone(),
            payload: s.payload.clone(),
        }),
        ProtoContent::Notification(n) => Elem::Notification(NotificationElem {
            title: n.title.clone(),
            body: n.body.clone(),
            notification_type: n.notification_type.clone(),
            data: n.attributes.clone(),
            target_user_ids: n.target_user_ids.clone(),
            target_role_id: n.target_role_id.clone(),
            notify_all: n.notify_all,
            persistent: n.persistent,
            show_in_list: n.show_in_list,
            show_badge: n.show_badge,
            play_sound: n.play_sound,
        }),
        ProtoContent::Custom(c) => Elem::Custom(CustomElem {
            r#type: c.r#type.clone(),
            payload: c.payload.clone(),
            description: c.description.clone(),
            metadata: c.attributes.clone(),
        }),
        ProtoContent::Placeholder(p) => Elem::Placeholder(PlaceholderElem {
            reason: p.reason.clone(),
            payload: p.payload.clone(),
            fallback_text: p.fallback_text.clone(),
            metadata: p.attributes.clone(),
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
            zoom: l.zoom.map(i32::from),
            snapshot_url: l.snapshot_url.clone(),
            snapshot_local_path: l.snapshot_local_path.clone(),
        }),
        Elem::Card(card) => ProtoContent::Card(flare_proto::common::CardContent {
            card_type: card.card_type.clone(),
            id: card.id.clone(),
            title: card.title.clone(),
            subtitle: card.subtitle.clone(),
            avatar: card.avatar.clone(),
            attributes: card.extra.clone(),
        }),
        Elem::Sticker(s) => ProtoContent::Sticker(flare_proto::common::StickerContent {
            sticker_id: s.sticker_id.clone(),
            url: s.url.clone(),
            width: s.width,
            height: s.height,
            attributes: s.extra.clone(),
            package_id: s.package_id.clone(),
            format: s.format.clone(),
        }),
        Elem::Emoji(e) => ProtoContent::Emoji(flare_proto::common::EmojiContent {
            emoji: e.emoji.clone(),
            description: e.description.clone(),
            attributes: e.extra.clone(),
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
                    source_message_created_at: it.source_message_time_ms.map(elem_ms_to_i64),
                    message_type: it.message_type,
                    plain_text: it.plain_text.clone(),
                    content: it.content.as_ref().map(|e| elem_to_message_content(e)),
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
            attributes: te.metadata.clone(),
        })),
        Elem::MiniProgram(m) => {
            let mut card =
                app_card_content(m.app_id.clone(), APP_CARD_MINI_PROGRAM, m.title.clone());
            card.thumbnail_url = m.thumbnail_url.clone();
            card.attributes = m.extra.clone();
            set_app_card_json_payload(&mut card, json!({ "page_path": m.page_path.clone() }));
            ProtoContent::AppCard(card)
        }
        Elem::RichText(r) => ProtoContent::RichText(flare_proto::common::RichTextContent {
            doc_json: r.doc_json.clone(),
            content_schema: r.content_schema.clone(),
            plain_text: r.plain_text.clone(),
            input_format: r.input_format.clone(),
            source_attributes: r.source_payload.clone(),
            input_format_version: r.input_format_version,
            title: r.title.clone(),
            search_text: r.search_text.clone(),
            render_hints_json: r.render_hints_json.clone(),
        }),
        Elem::ImageGroup(ig) => ProtoContent::ImageGroup(flare_proto::common::ImageGroupContent {
            images: ig.images.iter().map(image_info_in).collect(),
            description: ig.description.clone(),
            attributes: ig.metadata.clone(),
        }),
        Elem::System(s) => ProtoContent::System(flare_proto::common::SystemContent {
            event_kind: s.event_kind.clone(),
            body: s.body.clone(),
            attributes: s.data.clone(),
            payload: s.payload.clone(),
        }),
        Elem::Notification(n) => {
            ProtoContent::Notification(flare_proto::common::NotificationContent {
                title: n.title.clone(),
                body: n.body.clone(),
                notification_type: n.notification_type.clone(),
                attributes: n.data.clone(),
                target_user_ids: n.target_user_ids.clone(),
                target_role_id: n.target_role_id.clone(),
                notify_all: n.notify_all,
                persistent: n.persistent,
                show_in_list: n.show_in_list,
                show_badge: n.show_badge,
                play_sound: n.play_sound,
            })
        }
        Elem::Vote(v) => {
            let mut card = app_card_content(String::new(), APP_CARD_VOTE, v.title.clone());
            card.summary = v.options.join(", ");
            card.attributes = v.metadata.clone();
            set_app_card_json_payload(
                &mut card,
                json!({
                    "vote_id": v.vote_id.clone(),
                    "options": v.options.clone(),
                    "participant_user_ids": v.participant_user_ids.clone(),
                }),
            );
            ProtoContent::AppCard(card)
        }
        Elem::Task(t) => {
            let mut card = app_card_content(String::new(), APP_CARD_TASK, t.title.clone());
            card.attributes = t.metadata.clone();
            set_app_card_json_payload(
                &mut card,
                json!({
                    "task_id": t.task_id.clone(),
                    "status": t.status.clone(),
                    "participant_user_ids": t.participant_user_ids.clone(),
                }),
            );
            ProtoContent::AppCard(card)
        }
        Elem::Schedule(s) => {
            let mut card = app_card_content(String::new(), APP_CARD_SCHEDULE, s.title.clone());
            card.attributes = s.metadata.clone();
            set_app_card_json_payload(
                &mut card,
                json!({
                    "schedule_id": s.schedule_id.clone(),
                    "start_time_ms": s.start_time,
                    "end_time_ms": s.end_time,
                    "participant_user_ids": s.participant_user_ids.clone(),
                }),
            );
            ProtoContent::AppCard(card)
        }
        Elem::Announcement(a) => {
            let mut card = app_card_content(String::new(), APP_CARD_ANNOUNCEMENT, a.title.clone());
            card.summary = a.body.clone();
            card.attributes = a.metadata.clone();
            set_app_card_json_payload(&mut card, json!({ "pinned": a.pinned }));
            ProtoContent::AppCard(card)
        }
        Elem::Custom(c) => ProtoContent::Custom(flare_proto::common::CustomContent {
            r#type: c.r#type.clone(),
            payload: c.payload.clone(),
            description: c.description.clone(),
            attributes: c.metadata.clone(),
        }),
        Elem::Placeholder(p) => {
            ProtoContent::Placeholder(flare_proto::common::PlaceholderContent {
                reason: p.reason.clone(),
                payload: p.payload.clone(),
                fallback_text: p.fallback_text.clone(),
                attributes: p.metadata.clone(),
            })
        }
    }
}

/// 将展示用 `Elem` 编码为协议层 `MessageContent`。
///
/// 用于 `IMMessage.encoded_content` 为空但仍有解码后 `content` 的场景（例如 Tauri：`content_bytes` 对 JSON `skip`，`sdk_send` 仅回传 `content`）。
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
