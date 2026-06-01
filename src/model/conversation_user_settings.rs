//! 用户级会话偏好 ext 键与辅助函数（多端 LWW 同步）。

use super::Conversation;

pub const EXT_USER_SETTINGS_VERSION: &str = "user_settings_version";
pub const EXT_SETTINGS_DIRTY: &str = "settings_dirty";

pub fn user_settings_version(conversation: &Conversation) -> u64 {
    conversation
        .ext
        .get(EXT_USER_SETTINGS_VERSION)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
}

pub fn is_settings_dirty(conversation: &Conversation) -> bool {
    conversation
        .ext
        .get(EXT_SETTINGS_DIRTY)
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

pub fn mark_settings_dirty(conversation: &mut Conversation) {
    conversation
        .ext
        .insert(EXT_SETTINGS_DIRTY.to_string(), "1".to_string());
}

pub fn apply_remote_settings_version(conversation: &mut Conversation, version: u64) {
    conversation
        .ext
        .insert(EXT_USER_SETTINGS_VERSION.to_string(), version.to_string());
    conversation
        .ext
        .insert(EXT_SETTINGS_DIRTY.to_string(), "0".to_string());
}
