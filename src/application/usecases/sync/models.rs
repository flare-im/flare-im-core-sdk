#[derive(Debug, Clone)]
pub(crate) struct AppliedSingleConversationPage {
    pub has_decoded_items: bool,
    pub max_seq: u64,
    pub has_more: bool,
    pub next_cursor: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AppliedConversationIncremental {
    pub has_more: bool,
    pub server_cursor_ms: u64,
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
    pub has_decoded_items: bool,
}
