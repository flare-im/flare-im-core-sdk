//! 用户「下载到下载目录」记录与设置（与 SQLite `user_file_download` / `file_download_settings` 对应）。

use async_trait::async_trait;

use crate::shared::error::Result;

#[async_trait]
pub trait UserFileDownloadStore: Send + Sync {
    /// 已保存到本机下载目录的绝对路径（文件仍存在与否由上层用 `std::fs` 校验）。
    async fn get_saved_path(&self, download_key: &str) -> Result<Option<String>>;

    async fn save_download_record(
        &self,
        download_key: &str,
        local_path: &str,
        display_name: &str,
    ) -> Result<()>;

    /// 相对系统「下载」目录的子文件夹名，默认 `flare`。
    async fn get_download_subfolder(&self) -> Result<String>;

    async fn set_download_subfolder(&self, name: &str) -> Result<()>;

    /// 删除 `download_key` 对应行（本地文件已删或需重新下载时由上层调用）。
    async fn delete_download_record(&self, download_key: &str) -> Result<()>;

    /// 用户自选的下载目录（绝对路径）；未选时为 `None`，用平台默认目录。
    async fn get_download_directory(&self) -> Result<Option<String>>;

    /// 设置（`Some`）或清除（`None`，回到平台默认）用户自选的下载目录。
    /// 只负责持久化；路径是否可写由上层校验。
    async fn set_download_directory(&self, directory: Option<&str>) -> Result<()>;

    /// 平台下载目录都不可用时的兜底根目录（与本地库同级的 `downloads`）。
    fn fallback_download_root(&self) -> Option<std::path::PathBuf> {
        None
    }
}
