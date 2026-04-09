//! 媒体本地缓存（file_id ↔ 磁盘路径），与 SQLite `media_local_cache` 表对应。

/// 单条缓存记录（展示层可用 `local_path` 拼 `file://`）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MediaCacheEntryVo {
    pub file_id: String,
    pub local_path: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub size_bytes: i64,
    pub updated_at_ms: i64,
}

/// 本地媒体缓存空间概览（供设置页 / 清理 UI）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MediaCacheStatsVo {
    /// 当前实际使用的根目录（绝对路径）
    pub effective_root: String,
    /// 与 SQLite 库文件同级的默认目录（未自定义 `cache_root` 时与 effective 相同）
    pub default_root: String,
    /// `max_bytes == 0` 表示不限制
    pub max_bytes: u64,
    pub total_bytes: i64,
    pub entry_count: i64,
}
