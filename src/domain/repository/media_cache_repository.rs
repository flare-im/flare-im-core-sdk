use async_trait::async_trait;

use crate::domain::MediaCacheEntryVo;
use crate::shared::error::Result;

/// 媒体文件本地缓存端口：持久化 file_id 与落盘路径对照（通常由 SQLite + 文件系统实现）。
#[async_trait]
pub trait MediaCacheStore: Send + Sync {
    /// 返回仍存在于磁盘上的缓存；文件缺失时清理脏行并返回 `None`。
    async fn get_cached(&self, file_id: &str) -> Result<Option<MediaCacheEntryVo>>;

    /// 将字节写入缓存目录并 upsert 对照表。
    async fn put_bytes(
        &self,
        file_id: &str,
        data: &[u8],
        mime_type: &str,
    ) -> Result<MediaCacheEntryVo>;

    /// 删除记录；若本地文件存在则一并删除（忽略文件删除错误）。
    async fn remove(&self, file_id: &str) -> Result<()>;
}
