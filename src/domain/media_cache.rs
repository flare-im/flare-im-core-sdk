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
    /// 实际生效的容量上限（字节）。未设置时为默认上限 [`DEFAULT_MEDIA_CACHE_MAX_BYTES`]。
    pub max_bytes: u64,
    /// 容量上限是否为默认值（用户没有设置过）。
    #[serde(default)]
    pub max_bytes_is_default: bool,
    pub total_bytes: i64,
    pub entry_count: i64,
}

/// 媒体缓存未设置容量上限时的默认上限（1 GiB）：显示过的图片会自动进缓存，
/// 没有上限缓存会一直长。
pub const DEFAULT_MEDIA_CACHE_MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// 「保存到本机」的目标目录（设置页「下载位置」）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDownloadDirectoryVo {
    /// 实际生效的目录（绝对路径）。
    pub directory: String,
    /// 平台默认目录（未自选时生效）：桌面为系统「下载」下的子目录，Android 为
    /// 共享存储 `Download/` 下的子目录，iOS 为「文件」App 可见的 `Documents/` 下的子目录。
    pub default_directory: String,
    /// 用户自选的目录；未自选为 `None`。
    #[serde(default)]
    pub custom_directory: Option<String>,
    pub is_custom: bool,
    /// 默认目录里的子文件夹名。
    pub subfolder: String,
}

/// 一次「保存到本机」的结果。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserFileDownloadResultVo {
    /// 保存后的文件（绝对路径）。
    pub path: String,
    /// 文件所在目录。
    pub directory: String,
    /// 实际文件名（同名时带 ` (n)` 后缀）。
    pub file_name: String,
    pub size_bytes: u64,
    /// 取自本地媒体缓存（没有走网络）。
    pub from_cache: bool,
    pub download_key: String,
}
