//! 会话仓储接口
//!
//! 定义在领域层，实现在基础设施层

use crate::domain::message::model::SessionId;
use crate::domain::session::model::Session;
use anyhow::Result;
use async_trait::async_trait;

/// 会话仓储接口
///
/// 定义在领域层，实现在基础设施层
#[async_trait]
pub trait SessionRepository: Send + Sync {
    /// 保存会话
    async fn save(&self, session: &Session) -> Result<()>;

    /// 根据 ID 查找会话
    async fn find_by_id(&self, id: &SessionId) -> Result<Option<Session>>;

    /// 查找所有会话
    async fn find_all(&self, limit: Option<usize>) -> Result<Vec<Session>>;

    /// 删除会话
    async fn delete(&self, id: &SessionId) -> Result<()>;
}
