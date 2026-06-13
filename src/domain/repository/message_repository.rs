use async_trait::async_trait;
use flare_proto::common::{MessageRetentionPolicy, MessageRetentionState};
use std::collections::HashMap;

use crate::model::message::ReactionEntry;
use crate::model::{IMMessage, MessageSearchQuery};
use crate::shared::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditApplyResult {
    Applied,
    IgnoredStale,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationApplyResult {
    Applied,
    IgnoredStale,
    NotFound,
}

/// 消息查询（只读）
#[async_trait]
pub trait MessageReader: Send + Sync {
    async fn get(&self, message_id: &str) -> Result<Option<IMMessage>>;
    /// 按 client_msg_id 查询（发送中/待 ACK 时可能仅有 client_msg_id）
    async fn get_by_client_msg_id(&self, client_msg_id: &str) -> Result<Option<IMMessage>>;
    /// 批量按 client_msg_id 查询。默认逐条兜底；SQLite 实现覆盖为 `IN (...)` 单次查询，
    /// 降低可靠队列对账每 tick 的 O(n) DB 往返与连接池争用。
    async fn get_by_client_msg_ids(&self, client_msg_ids: &[String]) -> Result<Vec<IMMessage>> {
        let mut out = Vec::with_capacity(client_msg_ids.len());
        for id in client_msg_ids {
            if let Some(message) = self.get_by_client_msg_id(id).await? {
                out.push(message);
            }
        }
        Ok(out)
    }
    /// `before_seq == 0`：首屏，返回该会话最新 `limit` 条（`seq` 降序）。
    /// `before_seq > 0`：返回满足 `seq < before_seq` 的更早消息。
    async fn get_by_conversation(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<IMMessage>>;
    async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<IMMessage>>;
    async fn search_by_query(&self, query: &MessageSearchQuery) -> Result<Vec<IMMessage>> {
        if let Some(conversation_id) = query
            .conversation_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return self
                .search_in_conversation(
                    conversation_id,
                    query.keyword.as_deref().unwrap_or_default(),
                    query.normalized_limit(),
                )
                .await;
        }
        self.search(
            query.keyword.as_deref().unwrap_or_default(),
            query.normalized_limit(),
        )
        .await
    }
    /// 在指定会话内按正文关键字搜索（本地 SQLite）。
    async fn search_in_conversation(
        &self,
        conversation_id: &str,
        keyword: &str,
        limit: u32,
    ) -> Result<Vec<IMMessage>>;
}

/// 消息写操作
#[async_trait]
pub trait MessageWriter: Send + Sync {
    async fn save_batch(&self, messages: &[IMMessage]) -> Result<()>;
    async fn save_one(&self, message: &IMMessage) -> Result<()>;
    async fn update_status(&self, message_id: &str, status: i32) -> Result<()>;
    /// 更新消息正文；`Ok(true)` 表示至少更新了一行（`server_id` 或 `client_msg_id` 命中）。
    async fn update_content(&self, message_id: &str, new_content: Vec<u8>) -> Result<bool>;
    async fn delete(&self, message_id: &str) -> Result<()>;

    /// 发送 ACK 后更新：删除以 client_msg_id 为 server_id 的乐观写入行，再写入带 server_msg_id/seq 的终态消息（原子化，保证主键从 client_msg_id 迁移到 server_msg_id）
    async fn update_after_ack(&self, client_msg_id: &str, message: &IMMessage) -> Result<()>;
}

/// 消息统一端口（读写聚合）
#[async_trait]
pub trait MessageStore: MessageReader + MessageWriter {
    /// 应用编辑事件到存储层；实现可基于 `edit_version` 做版本收敛，避免旧编辑覆盖新内容。
    async fn apply_edit_event(
        &self,
        message_id: &str,
        new_content: Vec<u8>,
        _edit_version: i32,
    ) -> Result<EditApplyResult> {
        let updated = self.update_content(message_id, new_content).await?;
        Ok(if updated {
            EditApplyResult::Applied
        } else {
            EditApplyResult::NotFound
        })
    }

    /// 应用删除事件到存储层；实现可基于 `event_seq/seq` 做顺序收敛，避免旧删除覆盖更新状态。
    async fn apply_delete_event(
        &self,
        message_id: &str,
        _event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        self.delete(message_id).await?;
        Ok(OperationApplyResult::Applied)
    }

    /// 应用置顶/取消置顶事件到存储层；实现可基于 `event_seq/seq` 防止旧状态覆盖新状态。
    async fn apply_pin_event(
        &self,
        message_id: &str,
        enabled: bool,
        _event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        self.set_message_flag(message_id, "pinned", enabled).await?;
        Ok(OperationApplyResult::Applied)
    }

    /// 应用标记/取消标记事件到存储层；实现可基于 `event_seq/seq` 防止旧状态覆盖新状态。
    async fn apply_mark_event(
        &self,
        message_id: &str,
        mark_type: i32,
        color: Option<&str>,
        set_mark: bool,
        _event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        if set_mark {
            self.set_message_mark(message_id, mark_type, color).await?;
        } else {
            self.clear_message_mark(message_id, mark_type).await?;
        }
        Ok(OperationApplyResult::Applied)
    }

    /// 应用 retention「已安排」事件到存储层；实现可基于 `conversation_seq` 防旧事件回放覆盖新状态。
    async fn apply_retention_scheduled_event(
        &self,
        _message_id: &str,
        _policy: &MessageRetentionPolicy,
        _state: &MessageRetentionState,
        _scheduled_at: i64,
        _event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        Ok(OperationApplyResult::NotFound)
    }

    /// 应用 retention「已过期」事件到存储层；实现可基于 `conversation_seq` 防旧事件回放覆盖新状态。
    async fn apply_retention_expired_event(
        &self,
        _message_id: &str,
        _state: &MessageRetentionState,
        _expired_at: i64,
        _event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        Ok(OperationApplyResult::NotFound)
    }

    /// 应用 retention「已清理」事件到存储层；实现可基于 `conversation_seq` 防旧事件回放覆盖新状态。
    async fn apply_retention_purged_event(
        &self,
        _message_id: &str,
        _state: &MessageRetentionState,
        _purged_at: i64,
        _event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        Ok(OperationApplyResult::NotFound)
    }

    /// 对方已读回执落地：将当前用户在该会话中 `seq <= read_seq` 的已发送消息标记为已读。
    async fn mark_outgoing_read_upto_seq(
        &self,
        _conversation_id: &str,
        _sender_user_id: &str,
        _read_seq: u64,
    ) -> Result<()> {
        Ok(())
    }

    /// 以服务端会话摘要中的对端已读位点为权威，修正当前用户发出消息的已读投影。
    ///
    /// `peer_read_seq` 之前的消息可被标记为已读；其后的消息如果本地曾被 ACK/回显污染成已读，
    /// 需要回退到已发送，避免“对方离线但立刻双对号”。
    async fn reconcile_outgoing_read_by_peer_seq(
        &self,
        conversation_id: &str,
        sender_user_id: &str,
        peer_read_seq: u64,
    ) -> Result<()> {
        if peer_read_seq > 0 {
            self.mark_outgoing_read_upto_seq(conversation_id, sender_user_id, peer_read_seq)
                .await?;
        }
        Ok(())
    }

    /// 应用一条 reaction 事件到存储层（独立 reaction 表）。
    async fn apply_reaction(
        &self,
        _conversation_id: &str,
        _message_server_id: &str,
        _user_id: &str,
        _emoji: &str,
        _action: i32,
    ) -> Result<()> {
        Ok(())
    }

    /// 应用 reaction 事件到存储层；实现可基于 `(message,user,emoji)` 与 `event_seq/seq`
    /// 做顺序收敛，避免旧反应事件覆盖新状态。
    async fn apply_reaction_event(
        &self,
        conversation_id: &str,
        message_server_id: &str,
        user_id: &str,
        emoji: &str,
        action: i32,
        _event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        self.apply_reaction(conversation_id, message_server_id, user_id, emoji, action)
            .await?;
        Ok(OperationApplyResult::Applied)
    }

    /// 批量查询消息 reactions，key=message_server_id。
    async fn list_reactions(
        &self,
        _message_server_ids: &[String],
    ) -> Result<HashMap<String, Vec<ReactionEntry>>> {
        Ok(HashMap::new())
    }

    /// 设置消息 extra 中的布尔标记（如 pinned=true/false）。
    async fn set_message_flag(
        &self,
        _message_id: &str,
        _flag_key: &str,
        _enabled: bool,
    ) -> Result<()> {
        Ok(())
    }

    /// 设置消息标记（extra.markType / extra.markColor）。
    async fn set_message_mark(
        &self,
        _message_id: &str,
        _mark_type: i32,
        _color: Option<&str>,
    ) -> Result<()> {
        Ok(())
    }

    /// 取消消息标记（清理 extra.markType / extra.markColor）。
    async fn clear_message_mark(&self, _message_id: &str, _mark_type: i32) -> Result<()> {
        Ok(())
    }

    /// 会话恢复自愈：将本地 `sending=1` 但不在 pending 队列中的孤儿消息改为 failed。
    async fn heal_orphan_sending_messages(
        &self,
        _sender_user_id: &str,
        _pending_client_msg_ids: &[String],
    ) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// 会话恢复自检：将 pending 队列里不属于当前账号的历史条目标记失败并移除。
    async fn heal_cross_account_pending_messages(
        &self,
        _sender_user_id: &str,
        _pending_client_msg_ids: &[String],
    ) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
}
