//! 媒体 Facade — 委托 [`crate::application::MediaService`]（上传与下载）。

use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::application::MediaService;
use crate::domain::{
    MediaCacheAdmin, MediaCacheStatsVo, MediaCacheStore, UploadManifestStore, UserFileDownloadStore,
};
use crate::error::Result;
use crate::model::{MediaAccessUrl, MediaCacheEntryVo, MediaResolvedAccess, UploadOptions, UploadedMedia};
use crate::transport::HttpClient;
pub use crate::application::{
    FileDownloadProgress, FileDownloadProgressCallback, UploadPhase, UploadProgress,
    UploadProgressCallback,
};

#[derive(Clone)]
pub struct MediaApi {
    handler: Arc<MediaService>,
}

impl MediaApi {
    pub fn from_handler(handler: Arc<MediaService>) -> Self {
        Self { handler }
    }

    pub fn new(
        http: HttpClient,
        current_user_id: Arc<RwLock<String>>,
        upload_manifest_store: Option<Arc<dyn UploadManifestStore>>,
        media_cache_store: Option<Arc<dyn MediaCacheStore>>,
        media_cache_admin: Option<Arc<dyn MediaCacheAdmin>>,
        user_file_download_store: Option<Arc<dyn UserFileDownloadStore>>,
    ) -> Self {
        Self {
            handler: Arc::new(MediaService::new(
                http,
                current_user_id,
                upload_manifest_store,
                media_cache_store,
                media_cache_admin,
                user_file_download_store,
            )),
        }
    }

    pub fn http(&self) -> &HttpClient {
        self.handler.http()
    }

    pub async fn upload_file_from_path(
        &self,
        path: impl AsRef<Path>,
        options: Option<UploadOptions>,
    ) -> Result<UploadedMedia> {
        self.handler
            .upload_file_from_path_with_progress(path, options, None)
            .await
    }

    pub async fn upload_file_from_path_with_progress(
        &self,
        path: impl AsRef<Path>,
        options: Option<UploadOptions>,
        on_progress: Option<UploadProgressCallback>,
    ) -> Result<UploadedMedia> {
        self.handler
            .upload_file_from_path_with_progress(path, options, on_progress)
            .await
    }

    pub async fn delete_file(&self, file_id: &str, hard_delete: bool) -> Result<bool> {
        self.handler.delete_file(file_id, hard_delete).await
    }

    pub async fn get_file_url(&self, file_id: &str, expires_in: i32) -> Result<MediaAccessUrl> {
        self.handler.get_file_url(file_id, expires_in).await
    }

    /// 网关临时直链（`download: true`，适合附件另存为）。
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn get_temp_url_for_file_download(
        &self,
        file_id: &str,
        expires_in: i32,
    ) -> Result<MediaAccessUrl> {
        self.handler
            .get_temp_url_for_file_download(file_id, expires_in)
            .await
    }

    /// 取消进行中的「下载到用户下载目录」任务。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn cancel_user_file_download(&self, download_key: &str) -> bool {
        self.handler.cancel_user_file_download(download_key)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn user_download_get_subfolder(&self) -> Result<String> {
        self.handler.user_download_get_subfolder().await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn user_download_set_subfolder(&self, name: &str) -> Result<()> {
        self.handler.user_download_set_subfolder(name).await
    }

    /// SQLite 中已记录的本地绝对路径（文件是否仍存在需上层校验）。
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn user_download_get_saved_path(&self, download_key: &str) -> Result<Option<String>> {
        self.handler.user_download_get_saved_path(download_key).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn user_download_delete_record(&self, download_key: &str) -> Result<()> {
        self.handler.user_download_delete_record(download_key).await
    }

    /// 下载到「系统下载目录 / 子目录」并写入 SQLite；进度经 `on_progress` 回调。
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn download_file_to_user_downloads_folder(
        &self,
        download_key: impl AsRef<str>,
        display_file_name: impl AsRef<str>,
        source_path: Option<&str>,
        source_http_url: Option<&str>,
        remote_file_id: Option<&str>,
        expires_in: i32,
        on_progress: Option<FileDownloadProgressCallback>,
    ) -> Result<String> {
        self.handler
            .download_file_to_user_downloads_folder(
                download_key,
                display_file_name,
                source_path,
                source_http_url,
                remote_file_id,
                expires_in,
                on_progress,
            )
            .await
    }

    /// 优先返回本地缓存路径，否则返回网关短时 URL。
    pub async fn resolve_media_access(
        &self,
        file_id: &str,
        expires_in: i32,
    ) -> Result<MediaResolvedAccess> {
        self.handler.resolve_media_access(file_id, expires_in).await
    }

    /// 下载远程媒体并写入本地缓存与 SQLite 对照表（点击预览等时机调用）。
    pub async fn cache_remote_media(
        &self,
        file_id: &str,
        expires_in: i32,
    ) -> Result<MediaCacheEntryVo> {
        self.handler.cache_remote_media(file_id, expires_in).await
    }

    pub async fn media_cache_stats(&self) -> Result<MediaCacheStatsVo> {
        self.handler.media_cache_stats().await
    }

    pub async fn set_media_cache_max_bytes(&self, max_bytes: u64) -> Result<()> {
        self.handler.set_media_cache_max_bytes(max_bytes).await
    }

    pub async fn set_media_cache_root(&self, absolute_path: Option<&str>) -> Result<()> {
        self.handler.set_media_cache_root(absolute_path).await
    }

    pub async fn clear_media_cache(&self) -> Result<()> {
        self.handler.clear_media_cache().await
    }
}
