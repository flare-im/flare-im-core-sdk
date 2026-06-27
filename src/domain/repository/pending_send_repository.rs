use crate::domain::PendingSendVo;
use crate::shared::error::Result;
use async_trait::async_trait;

/// 待发送队列查询（只读）
#[async_trait]
pub trait PendingSendReader: Send + Sync {
    async fn get(&self, client_msg_id: &str) -> Result<Option<PendingSendVo>>;
    async fn list(&self) -> Result<Vec<PendingSendVo>>;

    /// 按入队时间取最老的候选项，同时排除已在途 client_msg_id。
    ///
    /// 可靠队列使用它做有界流水线填充，避免每次都全量拉取/解码 pending 表。
    async fn list_oldest_excluding(
        &self,
        excluded_client_msg_ids: &[String],
        limit: usize,
    ) -> Result<Vec<PendingSendVo>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let excluded = excluded_client_msg_ids
            .iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>();
        let mut list = self.list().await?;
        list.retain(|entry| !excluded.contains(entry.client_msg_id.as_str()));
        list.sort_by(|a, b| {
            a.enqueued_at_ms
                .cmp(&b.enqueued_at_ms)
                .then_with(|| a.client_msg_id.cmp(&b.client_msg_id))
        });
        list.truncate(limit);
        Ok(list)
    }

    /// 取队列中 enqueued_at_ms 最小的一条（用于可靠队列按序发送，避免全表扫描）
    async fn take_oldest(&self) -> Result<Option<PendingSendVo>> {
        Ok(self.list_oldest_excluding(&[], 1).await?.into_iter().next())
    }
}

/// 待发送队列写操作
#[async_trait]
pub trait PendingSendWriter: Send + Sync {
    async fn push(&self, entry: PendingSendVo) -> Result<()>;
    async fn pop(&self, client_msg_id: &str) -> Result<Option<PendingSendVo>>;
}
