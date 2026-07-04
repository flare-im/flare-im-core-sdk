#[derive(Debug, Clone)]
pub(crate) struct AppliedSingleConversationPage {
    pub has_decoded_items: bool,
    pub max_seq: u64,
    pub remote_max_seq: u64,
    pub has_more: bool,
    pub next_cursor: String,
    pub has_seq_gap: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct AppliedConversationIncremental {
    pub has_more: bool,
    pub message_sync_conversation_ids: Vec<String>,
    pub synced_conversation_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ReplayMode {
    SingleConversation,
    CriticalEvents,
}

#[derive(Debug, Clone)]
pub(crate) struct DecodedSingleConversationItems {
    pub messages: Vec<crate::model::IMMessage>,
    pub events: Vec<flare_proto::common::Event>,
    /// 与 `events` 一一对齐的 item seq；事件是否计入游标由 apply 结果决定，不在解码期预判。
    pub event_item_seqs: Vec<u64>,
    /// 落库可保证持久的 item seq（消息/skip/tombstone）：save_batch 失败会让整页出错、游标不动。
    pub covered_item_seqs: Vec<u64>,
    pub has_decoded_items: bool,
}
