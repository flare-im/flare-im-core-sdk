//! 消息仓储接口
//!
//! 定义在领域层，实现在基础设施层

use crate::domain::message::model::{Message, MessageId, SessionId};
use anyhow::Result;
use async_trait::async_trait;

/// 消息仓储接口
///
/// 定义在领域层，实现在基础设施层（infrastructure/persistence/storage）
#[async_trait]
pub trait MessageRepository: Send + Sync {
    /// 保存消息
    async fn save(&self, message: &Message) -> Result<()>;

    /// 根据 ID 查找消息
    async fn find_by_id(&self, id: &MessageId) -> Result<Option<Message>>;

    /// 根据会话 ID 查找消息列表
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `limit`: 返回数量限制
    /// - `before`: 在此消息 ID 之前查询（用于分页）
    async fn find_by_session(
        &self,
        session_id: &SessionId,
        limit: usize,
        before: Option<&MessageId>,
    ) -> Result<Vec<Message>>;

    /// 删除消息
    async fn delete(&self, id: &MessageId) -> Result<()>;

    /// 批量删除消息
    async fn delete_batch(&self, ids: Vec<MessageId>) -> Result<()>;

    /// 更新消息状态
    async fn update_status(&self, id: &MessageId, status: flare_proto::MessageStatus)
    -> Result<()>;
}
