//! 会话列表摘要、`messages.text`、`quote_preview` 等持久化字段使用的**稳定 i18n 载荷**（非展示文案）。
//!
//! 序列化形态（单列字符串，如 SQLite `TEXT`）：
//! `{"k":"im.preview.image","a":{"d":"说明","m":true}}`
//!
//! - **`k`**：应用端用于查表翻译的稳定 key（勿改语义；新增类型请追加新 key）。
//! - **`a`**：可选参数；常用短键：`t` 正文/标题类、`d` 说明、`n` 文件名或数量、`label` 位置/名片展示标签、`title`/`body` 富文本、`e` emoji 字符、`m` 布尔（如动图）、`inner`/`first` 嵌套对象（同为 `PreviewStoragePayload` 形状）。
//!
//! 历史数据若为**非 JSON 纯文本**，应用端应原样展示；新写入均为上述 JSON。

use serde::{Deserialize, Serialize};
use serde_json::Map;
use serde_json::Value;

/// 稳定预览类型 key（供各端 i18n 映射）。
pub mod keys {
    pub const USER_TEXT: &str = "im.preview.user_text";
    pub const RICH_TEXT: &str = "im.preview.rich_text";
    pub const FILE: &str = "im.preview.file";
    pub const IMAGE: &str = "im.preview.image";
    pub const VIDEO: &str = "im.preview.video";
    pub const AUDIO: &str = "im.preview.audio";
    pub const LOCATION: &str = "im.preview.location";
    pub const CARD: &str = "im.preview.card";
    pub const STICKER: &str = "im.preview.sticker";
    pub const EMOJI: &str = "im.preview.emoji";
    pub const QUOTE: &str = "im.preview.quote";
    pub const LINK: &str = "im.preview.link";
    pub const FORWARD_EMPTY: &str = "im.preview.forward_empty";
    pub const FORWARD_MANY: &str = "im.preview.forward_many";
    pub const THREAD: &str = "im.preview.thread";
    pub const MINI_PROGRAM: &str = "im.preview.mini_program";
    pub const IMAGE_GROUP: &str = "im.preview.image_group";
    pub const SYSTEM: &str = "im.preview.system";
    pub const NOTIFICATION: &str = "im.preview.notification";
    pub const VOTE: &str = "im.preview.vote";
    pub const TASK: &str = "im.preview.task";
    pub const SCHEDULE: &str = "im.preview.schedule";
    pub const ANNOUNCEMENT: &str = "im.preview.announcement";
    pub const CUSTOM: &str = "im.preview.custom";
    pub const PLACEHOLDER: &str = "im.preview.placeholder";
    /// 解码失败或 `MessageContent` 无 oneof 时使用。
    pub const UNKNOWN: &str = "im.preview.unknown";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewStoragePayload {
    pub k: String,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub a: Map<String, Value>,
}

impl PreviewStoragePayload {
    /// 纯文本且 `t` 为空时，会话列表等场景视为「无摘要」。
    pub fn is_empty_for_last_preview(&self) -> bool {
        if self.k != keys::USER_TEXT {
            return false;
        }
        self.a
            .get("t")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
    }
}

/// 若已是合法 JSON 载荷则解析；否则视为用户可见原文（兼容历史或非结构化 `plain_text`）。
pub fn decode_or_user_text(s: &str) -> PreviewStoragePayload {
    let t = s.trim();
    if t.is_empty() {
        return PreviewStoragePayload {
            k: keys::USER_TEXT.to_string(),
            a: Map::new(),
        };
    }
    if let Ok(p) = serde_json::from_str::<PreviewStoragePayload>(t) {
        return p;
    }
    let mut a = Map::new();
    a.insert("t".into(), Value::String(s.to_string()));
    PreviewStoragePayload {
        k: keys::USER_TEXT.to_string(),
        a,
    }
}

/// 与 [`DecodedContent::Unknown`]、Elem 解析失败等对齐的预览 JSON。
pub fn unknown_preview_json() -> String {
    serde_json::to_string(&PreviewStoragePayload {
        k: keys::UNKNOWN.to_string(),
        a: Map::new(),
    })
    .unwrap_or_default()
}

/// 不应写入 `extra.contentText` 的预览（空串、未知类型、历史 `[未知]` 占位）。
pub fn is_redundant_content_text_extra(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return true;
    }
    if t == "[未知]" {
        return true;
    }
    if let Ok(p) = serde_json::from_str::<PreviewStoragePayload>(t) {
        if p.k == keys::UNKNOWN {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preview_payload_roundtrips_json() {
        let mut a = Map::new();
        a.insert("t".into(), json!("hi"));
        let p = PreviewStoragePayload {
            k: keys::USER_TEXT.to_string(),
            a,
        };
        let s = serde_json::to_string(&p).unwrap();
        let back: PreviewStoragePayload = serde_json::from_str(&s).unwrap();
        assert_eq!(back.k, keys::USER_TEXT);
        assert_eq!(back.a.get("t").and_then(|v| v.as_str()), Some("hi"));
    }

    #[test]
    fn decode_or_user_text_accepts_json_or_plain() {
        let mut ia = Map::new();
        ia.insert("m".into(), json!(true));
        let inner = PreviewStoragePayload {
            k: keys::IMAGE.to_string(),
            a: ia,
        };
        let s = serde_json::to_string(&inner).unwrap();
        let p = decode_or_user_text(&s);
        assert_eq!(p.k, keys::IMAGE);

        let q = decode_or_user_text("legacy");
        assert_eq!(q.k, keys::USER_TEXT);
        assert_eq!(q.a.get("t").and_then(|v| v.as_str()), Some("legacy"));
    }

    #[test]
    fn unknown_preview_json_shape() {
        let s = unknown_preview_json();
        let p: PreviewStoragePayload = serde_json::from_str(&s).unwrap();
        assert_eq!(p.k, keys::UNKNOWN);
    }

    #[test]
    fn is_redundant_content_text_extra_cases() {
        assert!(is_redundant_content_text_extra(""));
        assert!(is_redundant_content_text_extra("   "));
        assert!(is_redundant_content_text_extra("[未知]"));
        assert!(is_redundant_content_text_extra(&unknown_preview_json()));
        assert!(!is_redundant_content_text_extra("你好"));
        assert!(!is_redundant_content_text_extra(
            r#"{"k":"im.preview.image","a":{}}"#,
        ));
    }
}
