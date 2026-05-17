mod models;
mod policy;

pub use models::{ConversationCursorSelection, CriticalEventQueryPlan, DeleteVisibilityDecision};
pub use policy::SyncPolicy;

pub const DEFAULT_SYNC_LIMIT: i32 = 100;
pub const CONVERSATION_CURSOR_KEY: &str = "__conversations__";
pub const CRITICAL_EVENT_CURSOR_KEY: &str = "__critical_events__";
pub const QUERY_EVENTS_TIMEOUT_SECS: u64 = 30;
pub const UPDATE_CURSOR_TIMEOUT_SECS: u64 = 5;
/// 冷启动会话列表同步可能经多服务读快照，30s 在本地/弱环境易触发误超时。
pub const CONVERSATIONS_SYNC_TIMEOUT_SECS: u64 = 90;
