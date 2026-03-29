use crate::domain::models::PendingSendVo;
use crate::error::Result;
use async_trait::async_trait;

/// 待发送队列查询（只读）
#[async_trait]
pub trait PendingSendReader: Send + Sync {
    async fn get(&self, client_msg_id: &str) -> Result<Option<PendingSendVo>>;
    async fn list(&self) -> Result<Vec<PendingSendVo>>;

    /// 取队列中 enqueued_at_ms 最小的一条（用于可靠队列按序发送，避免全表扫描）
    async fn take_oldest(&self) -> Result<Option<PendingSendVo>> {
        let list = self.list().await?;
        Ok(list.into_iter().min_by_key(|e| e.enqueued_at_ms))
    }
}

/// 待发送队列写操作
#[async_trait]
pub trait PendingSendWriter: Send + Sync {
    async fn push(&self, entry: PendingSendVo) -> Result<()>;
    async fn pop(&self, client_msg_id: &str) -> Result<Option<PendingSendVo>>;
}
