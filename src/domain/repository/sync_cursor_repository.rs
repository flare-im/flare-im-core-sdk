use crate::domain::SyncCursorVo;
use crate::error::Result;
use async_trait::async_trait;

/// 同步游标查询（只读）
#[async_trait]
pub trait SyncCursorReader: Send + Sync {
    async fn get_raw(&self, key: &str) -> Result<Option<String>>;
    async fn get_conversation_cursor(
        &self,
        user_id: &str,
        conversation_id: &str,
    ) -> Result<Option<SyncCursorVo>>;
}

/// 同步游标写操作
#[async_trait]
pub trait SyncCursorWriter: Send + Sync {
    async fn save_raw(&self, key: &str, cursor: &str) -> Result<()>;
    async fn save_conversation_cursor(&self, cursor: &SyncCursorVo) -> Result<()>;
}
