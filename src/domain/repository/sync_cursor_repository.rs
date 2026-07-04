use crate::domain::SyncCursorVo;
use crate::shared::error::Result;
use async_trait::async_trait;

/// 同步游标查询（只读）
#[async_trait]
pub trait SyncCursorReader: Send + Sync {
    async fn get_raw(&self, key: &str) -> Result<Option<String>>;

    /// 批量 raw 读取（批量优于 N+1）：默认逐条回退；SQL 后端覆盖为单条 IN 查询。
    /// 缺失的 key 不在返回表中。
    async fn get_raws(&self, keys: &[String]) -> Result<std::collections::HashMap<String, String>> {
        let mut out = std::collections::HashMap::with_capacity(keys.len());
        for key in keys {
            if let Some(value) = self.get_raw(key).await? {
                out.insert(key.clone(), value);
            }
        }
        Ok(out)
    }
    async fn get_conversation_cursor(
        &self,
        user_id: &str,
        conversation_id: &str,
    ) -> Result<Option<SyncCursorVo>>;

    /// 批量会话游标（I7 批量优于 N+1）：默认逐条回退；SQL 后端覆盖为单条 IN 查询。
    /// 无游标的会话不在返回表中。
    async fn get_conversation_cursors(
        &self,
        user_id: &str,
        conversation_ids: &[String],
    ) -> Result<std::collections::HashMap<String, SyncCursorVo>> {
        let mut out = std::collections::HashMap::with_capacity(conversation_ids.len());
        for conversation_id in conversation_ids {
            if let Some(cursor) = self
                .get_conversation_cursor(user_id, conversation_id)
                .await?
            {
                out.insert(conversation_id.clone(), cursor);
            }
        }
        Ok(out)
    }
}

/// 同步游标写操作
#[async_trait]
pub trait SyncCursorWriter: Send + Sync {
    async fn save_raw(&self, key: &str, cursor: &str) -> Result<()>;
    async fn save_conversation_cursor(&self, cursor: &SyncCursorVo) -> Result<()>;

    /// 批量保存。默认逐条（语义与今日完全一致——本地存储无隐藏放大，
    /// 区别于服务端批量 RPC 那类性能契约）；SQLite 以单事务覆盖，
    /// 冷启 bundle 每页 ~100 次串行 upsert → 1 个事务。
    async fn save_conversation_cursors(&self, cursors: &[SyncCursorVo]) -> Result<()> {
        for cursor in cursors {
            self.save_conversation_cursor(cursor).await?;
        }
        Ok(())
    }
}

/// 同步游标统一端口（读写聚合）
pub trait SyncCursorStore: SyncCursorReader + SyncCursorWriter {}

impl<T> SyncCursorStore for T where T: SyncCursorReader + SyncCursorWriter + Send + Sync {}
