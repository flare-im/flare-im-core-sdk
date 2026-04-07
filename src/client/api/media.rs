//! 媒体 Facade — 委托 [`crate::application::MediaUploadService`]。

use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::application::MediaUploadService;
use crate::domain::{MediaCacheAdmin, MediaCacheStatsVo, MediaCacheStore, UploadManifestStore};
use crate::error::Result;
use crate::model::{MediaAccessUrl, MediaCacheEntryVo, MediaResolvedAccess, UploadOptions, UploadedMedia};
use crate::transport::HttpClient;
pub use crate::application::{UploadPhase, UploadProgress, UploadProgressCallback};

#[derive(Clone)]
pub struct MediaApi {
    handler: Arc<MediaUploadService>,
}

impl MediaApi {
    pub fn from_handler(handler: Arc<MediaUploadService>) -> Self {
        Self { handler }
    }

    pub fn new(
        http: HttpClient,
        current_user_id: Arc<RwLock<String>>,
        upload_manifest_store: Option<Arc<dyn UploadManifestStore>>,
        media_cache_store: Option<Arc<dyn MediaCacheStore>>,
        media_cache_admin: Option<Arc<dyn MediaCacheAdmin>>,
    ) -> Self {
        Self {
            handler: Arc::new(MediaUploadService::new(
                http,
                current_user_id,
                upload_manifest_store,
                media_cache_store,
                media_cache_admin,
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
