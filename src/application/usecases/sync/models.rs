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
    pub server_cursor_ms: u64,
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
    pub applied_item_seqs: Vec<u64>,
    pub has_decoded_items: bool,
}
