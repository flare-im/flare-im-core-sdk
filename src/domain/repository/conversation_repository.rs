use async_trait::async_trait;

use crate::error::Result;
use crate::model::Conversation;

/// 会话查询（只读）
/// 列表顺序：先置顶（is_pinned DESC），再按 last_message_at 倒序
#[async_trait]
pub trait ConversationReader: Send + Sync {
    async fn get(&self, conversation_id: &str) -> Result<Option<Conversation>>;
    /// 列表：置顶优先，再按 last_message_at 倒序
    async fn list(&self) -> Result<Vec<Conversation>>;
}

/// 会话写操作（内部统一使用 Conversation）
#[async_trait]
pub trait ConversationWriter: Send + Sync {
    async fn save_batch(&self, conversations: &[Conversation]) -> Result<()>;
    async fn save_one(&self, conversation: &Conversation) -> Result<()>;
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

    /// 更新会话最后一条消息与 max_seq（消息同步/接收后写本地视图）
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
