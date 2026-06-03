//! Web/WASM media service stub.
//!
//! Browser media uses File/Blob and upload primitives supplied by the Web
//! runtime adapter. Native file-path upload/cache logic lives in
//! `media_service_native`.

use std::path::Path;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::application::callbacks::UploadProgressCallback;
use crate::domain::{
    MediaCacheAdmin, MediaCacheEntryVo, MediaCacheStatsVo, MediaCacheStore, UploadManifestStore,
    UserFileDownloadStore,
};
use crate::infrastructure::transport::HttpClient;
use crate::model::{MediaAccessUrl, MediaResolvedAccess, UploadOptions, UploadedMedia};
use crate::platform::ports::media::{MediaServicePort, ProcessedMedia, UploadProgressSink};
use crate::shared::error::{ErrorCode, FlareError, Result};

#[derive(Clone)]
pub struct MediaService {
    http: HttpClient,
}

impl MediaService {
    pub fn new(
        http: HttpClient,
        _current_user_id: Arc<RwLock<String>>,
        _upload_manifest_store: Option<Arc<dyn UploadManifestStore>>,
        _media_cache_store: Option<Arc<dyn MediaCacheStore>>,
        _media_cache_admin: Option<Arc<dyn MediaCacheAdmin>>,
        _user_file_download_store: Option<Arc<dyn UserFileDownloadStore>>,
    ) -> Self {
        Self { http }
    }

    pub fn http(&self) -> &HttpClient {
        &self.http
    }

    pub async fn upload_file_from_path_with_progress(
        &self,
        _path: impl AsRef<Path>,
        _options: Option<UploadOptions>,
        _on_progress: Option<UploadProgressCallback>,
    ) -> Result<UploadedMedia> {
        Err(wasm_media_unavailable("upload_file_from_path"))
    }

    pub async fn upload_image_from_path_with_progress(
        &self,
        _path: impl AsRef<Path>,
        _options: Option<UploadOptions>,
        _on_progress: Option<UploadProgressCallback>,
    ) -> Result<UploadedMedia> {
        Err(wasm_media_unavailable("upload_image_from_path"))
    }

    pub async fn upload_video_from_path_with_progress(
        &self,
        _path: impl AsRef<Path>,
        _options: Option<UploadOptions>,
        _on_progress: Option<UploadProgressCallback>,
    ) -> Result<UploadedMedia> {
        Err(wasm_media_unavailable("upload_video_from_path"))
    }

    pub async fn delete_file(&self, _file_id: &str, _hard_delete: bool) -> Result<bool> {
        Err(wasm_media_unavailable("delete_file"))
    }

    pub async fn get_file_url(&self, _file_id: &str, _expires_in: i32) -> Result<MediaAccessUrl> {
        Err(wasm_media_unavailable("get_file_url"))
    }

    pub async fn resolve_media_access(
        &self,
        _file_id: &str,
        _expires_in: i32,
    ) -> Result<MediaResolvedAccess> {
        Err(wasm_media_unavailable("resolve_media_access"))
    }

    pub async fn cache_remote_media(
        &self,
        _file_id: &str,
        _expires_in: i32,
    ) -> Result<MediaCacheEntryVo> {
        Err(wasm_media_unavailable("cache_remote_media"))
    }

    pub async fn media_cache_stats(&self) -> Result<MediaCacheStatsVo> {
        Err(wasm_media_unavailable("media_cache_stats"))
    }

    pub async fn set_media_cache_max_bytes(&self, _max_bytes: u64) -> Result<()> {
        Err(wasm_media_unavailable("set_media_cache_max_bytes"))
    }

    pub async fn set_media_cache_root(&self, _absolute_path: Option<&str>) -> Result<()> {
        Err(wasm_media_unavailable("set_media_cache_root"))
    }

    pub async fn clear_media_cache(&self) -> Result<()> {
        Err(wasm_media_unavailable("clear_media_cache"))
    }
}

#[async_trait::async_trait]
impl MediaServicePort for MediaService {
    async fn upload(
        &self,
        _media: ProcessedMedia,
        _options: Option<UploadOptions>,
        _progress: Option<UploadProgressSink>,
    ) -> Result<UploadedMedia> {
        Err(wasm_media_unavailable("upload"))
    }

    async fn delete_file(&self, file_id: &str, hard_delete: bool) -> Result<bool> {
        MediaService::delete_file(self, file_id, hard_delete).await
    }

    async fn get_file_url(&self, file_id: &str, expires_in: i32) -> Result<MediaAccessUrl> {
        MediaService::get_file_url(self, file_id, expires_in).await
    }

    async fn resolve_media_access(
        &self,
        file_id: &str,
        expires_in: i32,
    ) -> Result<MediaResolvedAccess> {
        MediaService::resolve_media_access(self, file_id, expires_in).await
    }

    async fn cache_remote_media(
        &self,
        file_id: &str,
        expires_in: i32,
    ) -> Result<MediaCacheEntryVo> {
        MediaService::cache_remote_media(self, file_id, expires_in).await
    }

    async fn media_cache_stats(&self) -> Result<MediaCacheStatsVo> {
        MediaService::media_cache_stats(self).await
    }

    async fn set_media_cache_max_bytes(&self, max_bytes: u64) -> Result<()> {
        MediaService::set_media_cache_max_bytes(self, max_bytes).await
    }

    async fn set_media_cache_root(&self, absolute_path: Option<&str>) -> Result<()> {
        MediaService::set_media_cache_root(self, absolute_path).await
    }

    async fn clear_media_cache(&self) -> Result<()> {
        MediaService::clear_media_cache(self).await
    }
}

fn wasm_media_unavailable(operation: &str) -> FlareError {
    FlareError::localized(
        ErrorCode::OperationNotSupported,
        format!("{operation} requires a Web media adapter"),
    )
}
