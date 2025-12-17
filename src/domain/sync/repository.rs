//! 同步仓储接口
//!
//! 定义在领域层，实现在基础设施层

use crate::domain::message::model::SessionId;
use crate::domain::sync::model::Sync;
use anyhow::Result;
use async_trait::async_trait;

/// 同步仓储接口
///
/// 定义在领域层，实现在基础设施层
#[async_trait]
pub trait SyncRepository: Send + std::marker::Sync {
    /// 保存同步状态
    async fn save(&self, sync: &Sync) -> Result<()>;

    /// 根据会话 ID 查找同步状态
    async fn find_by_session(&self, session_id: &SessionId) -> Result<Option<Sync>>;

    /// 查找所有同步状态
    async fn find_all(&self) -> Result<Vec<Sync>>;
}
