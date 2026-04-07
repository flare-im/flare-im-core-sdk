#[derive(Debug, Clone)]
pub struct CriticalEventQueryPlan {
    pub event_types: Vec<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeleteVisibilityDecision {
    pub apply_to_current_user: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversationCursorSelection {
    pub selected_cursor_ms: Option<u64>,
    pub drop_local_incremental_cursor: bool,
    pub force_full_sync: bool,
}
