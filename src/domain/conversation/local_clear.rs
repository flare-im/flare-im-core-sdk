//! 本地清空聊天记录水位：用户清空后，重启/再登录不同步已隐藏的历史消息。

use std::collections::HashMap;

use crate::model::IMMessage;

/// 持久化在 `conversations.ext` 中。
pub const EXT_LOCAL_CLEARED_THROUGH_SEQ: &str = "local_cleared_through_seq";

pub fn local_cleared_through_seq(ext: &HashMap<String, String>) -> u64 {
    ext.get(EXT_LOCAL_CLEARED_THROUGH_SEQ)
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

pub fn set_local_cleared_through_seq(ext: &mut HashMap<String, String>, seq: u64) {
    if seq == 0 {
        ext.remove(EXT_LOCAL_CLEARED_THROUGH_SEQ);
        return;
    }
    ext.insert(EXT_LOCAL_CLEARED_THROUGH_SEQ.to_string(), seq.to_string());
}

/// `seq == 0` 保留待发/未分配 seq 的本地消息；已分配 seq 须严格大于水位。
pub fn message_visible_after_clear(message: &IMMessage, cleared_through_seq: u64) -> bool {
    if cleared_through_seq == 0 {
        return true;
    }
    if message.seq == 0 {
        return true;
    }
    message.seq > cleared_through_seq
}

pub fn filter_messages_after_clear(
    messages: Vec<IMMessage>,
    cleared_through_seq: u64,
) -> Vec<IMMessage> {
    if cleared_through_seq == 0 {
        return messages;
    }
    messages
        .into_iter()
        .filter(|m| message_visible_after_clear(m, cleared_through_seq))
        .collect()
}
