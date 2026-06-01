//! 会话摘要与本地投影字段合并（remark / channel 等非读位字段）。

use crate::model::conversation::ConversationType;

/// 摘要同步时保留本地 remark（服务端摘要不含通讯录备注）。
#[must_use]
pub fn preserve_local_remark(incoming: Option<&str>, local: Option<&str>) -> Option<String> {
    if incoming.map(str::trim).is_some_and(|v| !v.is_empty()) {
        return incoming.map(str::to_string);
    }
    local
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// 单聊 channel_id 以本地已修复的 peer 为准。
#[must_use]
pub fn preserve_local_single_chat_channel(
    conversation_type: ConversationType,
    incoming_channel: &str,
    local_channel: &str,
) -> String {
    if !conversation_type.is_single_chat_conversation() {
        return incoming_channel.to_string();
    }
    let local = local_channel.trim();
    let incoming = incoming_channel.trim();
    if !local.is_empty() && (incoming.is_empty() || incoming != local) {
        local.to_string()
    } else {
        incoming.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{preserve_local_remark, preserve_local_single_chat_channel};
    use crate::model::conversation::ConversationType;

    #[test]
    fn preserve_remark_when_incoming_empty() {
        assert_eq!(
            preserve_local_remark(None, Some(" 备注 ")),
            Some("备注".to_string())
        );
    }

    #[test]
    fn preserve_incoming_remark_when_present() {
        assert_eq!(
            preserve_local_remark(Some("服务端"), Some("本地")),
            Some("服务端".to_string())
        );
    }

    #[test]
    fn preserve_local_single_chat_channel_when_incoming_is_display_name() {
        let channel =
            preserve_local_single_chat_channel(ConversationType::Single, "张三", "user_123");
        assert_eq!(channel, "user_123");
    }
}
