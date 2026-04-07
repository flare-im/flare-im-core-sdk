use async_trait::async_trait;

use crate::domain::MediaCacheStatsVo;
use crate::error::Result;

/// 媒体缓存管理：容量、目录、清空（与 [`crate::domain::MediaCacheStore`] 配套，通常仅 SQLite 实现）。
#[async_trait]
pub trait MediaCacheAdmin: Send + Sync {
    async fn media_cache_stats(&self) -> Result<MediaCacheStatsVo>;

    /// `0` 表示不限制；写入后对新写入生效并在必要时按 LRU 淘汰旧条目。
    async fn set_media_cache_max_bytes(&self, max_bytes: u64) -> Result<()>;

    /// `None` 或空字符串恢复为默认目录（与数据库文件同级的 `media_cache`）。  
    /// 变更前会**清空**现有缓存条目与文件，避免路径不一致。
    async fn set_media_cache_root(&self, absolute_path: Option<&str>) -> Result<()>;

    /// 删除全部缓存文件与 `media_local_cache` 记录。
    async fn clear_media_cache(&self) -> Result<()>;
}
