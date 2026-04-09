//! 用户「下载到下载目录」记录与设置（与 SQLite `user_file_download` / `file_download_settings` 对应）。

use async_trait::async_trait;

use crate::Result;

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
}
