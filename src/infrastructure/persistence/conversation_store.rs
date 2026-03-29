use async_trait::async_trait;

use crate::error::Result;
use crate::model::Conversation;

/// 会话存储 trait — 可插拔后端（SQLite / Memory）；内部统一使用 Conversation
/// 列表顺序：先置顶，再按 last_message_at 倒序
#[async_trait]
pub trait ConversationStore: Send + Sync {
    async fn save_batch(&self, conversations: &[Conversation]) -> Result<()>;
    async fn save_one(&self, conversation: &Conversation) -> Result<()>;
    async fn get(&self, conversation_id: &str) -> Result<Option<Conversation>>;
    async fn list(&self) -> Result<Vec<Conversation>>;
    async fn update_unread(
        &self,
        conversation_id: &str,
        unread_count: u32,
        last_read_seq: u64,
    ) -> Result<()>;
    async fn set_pinned(&self, conversation_id: &str, pinned: bool) -> Result<()>;
    async fn set_muted(&self, conversation_id: &str, muted: bool) -> Result<()>;
    async fn set_archived(&self, conversation_id: &str, archived: bool) -> Result<()>;
    async fn update_draft(&self, conversation_id: &str, draft: Option<&str>) -> Result<()>;
    async fn delete(&self, conversation_id: &str) -> Result<()>;

    /// 收到新消息或同步拉取后更新会话最后一条消息与 max_seq（本地视图与列表排序）
    async fn update_last_message(
        &self,
        conversation_id: &str,
        last_message_id: &str,
        last_sender_id: &str,
        last_message_at: u64,
        last_message_preview: Option<&str>,
        max_seq: u64,
    ) -> Result<()>;
}
