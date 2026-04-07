use async_trait::async_trait;
use std::collections::HashMap;

use crate::error::Result;
use crate::model::message::ReactionEntry;
use crate::model::IMMessage;

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
    async fn get_by_conversation(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<IMMessage>>;
    async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<IMMessage>>;
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

    /// 对方已读回执落地：将当前用户在该会话中 `seq <= read_seq` 的已发送消息标记为已读。
    async fn mark_outgoing_read_upto_seq(
        &self,
        _conversation_id: &str,
        _sender_user_id: &str,
        _read_seq: u64,
    ) -> Result<()> {
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

    /// 设置消息标记（extra.mark_type / extra.mark_color）。
    async fn set_message_mark(
        &self,
        _message_id: &str,
        _mark_type: i32,
        _color: Option<&str>,
    ) -> Result<()> {
        Ok(())
    }

    /// 取消消息标记（清理 extra.mark_type / extra.mark_color）。
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

