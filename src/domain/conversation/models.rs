#[derive(Debug, Clone)]
pub struct ConversationReadDecision {
    pub unread_count: u32,
    pub next_read_seq: u64,
    pub should_recompute_local_unread: bool,
}
