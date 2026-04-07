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

    /// 基于本地消息表重算单会话未读：
    /// `seq > last_read_seq` 且 `sender_id != current_user_id` 且未撤回。
    async fn recompute_unread_for_user(
        &self,
        conversation_id: &str,
        current_user_id: &str,
    ) -> Result<()>;

    /// 查询本地消息表中的最大 seq（用于 read_seq=0 的“全部已读”本地对齐）。
    async fn get_local_max_seq(&self, _conversation_id: &str) -> Result<u64> {
        Ok(0)
    }
}

/// 会话统一端口（读写聚合）
pub trait ConversationStore: ConversationReader + ConversationWriter {}

impl<T> ConversationStore for T where T: ConversationReader + ConversationWriter + Send + Sync {}
