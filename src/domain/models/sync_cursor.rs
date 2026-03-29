#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncCursorVo {
    pub user_id: String,
    pub conversation_id: String,
    pub last_seq: u64,
    pub synced_at: u64,
}
