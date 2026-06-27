use async_trait::async_trait;

use crate::model::{Conversation, ConversationListQuery};
use crate::shared::error::Result;

/// 会话查询（只读）
/// 列表顺序：先置顶（is_pinned DESC），再按 last_message_at 倒序
#[async_trait]
pub trait ConversationReader: Send + Sync {
    async fn get(&self, conversation_id: &str) -> Result<Option<Conversation>>;
    /// 列表：置顶优先，再按 last_message_at 倒序
    async fn list(&self) -> Result<Vec<Conversation>>;
    async fn list_by_query(&self, query: &ConversationListQuery) -> Result<Vec<Conversation>> {
        let keyword = query.normalized_keyword();
        let mut out = self.list().await?;
        out.retain(|conversation| {
            if !query.include_archived && conversation.is_archived {
                return false;
            }
            if query.unread_only && conversation.unread_count == 0 {
                return false;
            }
            if query.mention_me_only && !conversation.mention_me && conversation.mention_count == 0
            {
                return false;
            }
            if query.pinned_only && !conversation.is_pinned {
                return false;
            }
            if let Some(muted) = query.muted_only
                && conversation.is_muted != muted
            {
                return false;
            }
            if query.has_draft_only
                && conversation
                    .draft
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default()
                    .is_empty()
            {
                return false;
            }
            if !query.conversation_types.is_empty()
                && !query
                    .conversation_types
                    .contains(&conversation.conversation_type)
            {
                return false;
            }
            if query.has_marked_messages {
                return false;
            }
            if let Some(keyword) = keyword.as_deref() {
                let haystacks = [
                    conversation.conversation_id.as_str(),
                    conversation.channel_id.as_str(),
                    conversation.display_name.as_str(),
                    conversation.remark.as_deref().unwrap_or_default(),
                    conversation.description.as_deref().unwrap_or_default(),
                    conversation
                        .last_message_preview
                        .as_deref()
                        .unwrap_or_default(),
                ];
                return haystacks
                    .iter()
                    .any(|value| value.to_lowercase().contains(keyword));
            }
            true
        });
        Ok(out)
    }
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
    /// 标为未读：将读位回退 1 并显示未读角标（飞书「标为未读」语义）。
    async fn mark_unread(&self, conversation_id: &str) -> Result<u32>;
    async fn update_draft(&self, conversation_id: &str, draft: Option<&str>) -> Result<()>;
    async fn delete(&self, conversation_id: &str) -> Result<()>;

    /// 将错误会话 ID 的本地会话状态合并到规范会话 ID，并移除错误行。
    async fn merge_conversation_identity(
        &self,
        from_conversation_id: &str,
        to_conversation_id: &str,
    ) -> Result<()>;

    /// 清空本地聊天记录（保留会话行；不删服务端历史）。
    /// `cleared_through_seq`：该 seq 及更早消息在本地视为已清空，后续同步不再落库。
    async fn clear_local_chat_history(
        &self,
        conversation_id: &str,
        cleared_through_seq: u64,
    ) -> Result<()>;

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

    /// 查询本地消息表中的最大 seq（用于将显式 read_seq clamp 到本地已知上界）。
    async fn get_local_max_seq(&self, _conversation_id: &str) -> Result<u64> {
        Ok(0)
    }

    /// 用户资料变更后，刷新单聊展示名/头像及最后发送者展示字段；返回受影响的会话 ID。
    async fn apply_user_profile_snapshot(
        &self,
        user_id: &str,
        nickname: &str,
        avatar_url: &str,
    ) -> Result<Vec<String>> {
        let _ = (user_id, nickname, avatar_url);
        Ok(Vec::new())
    }
}

/// 会话统一端口（读写聚合）
pub trait ConversationStore: ConversationReader + ConversationWriter {}

impl<T> ConversationStore for T where T: ConversationReader + ConversationWriter + Send + Sync {}
