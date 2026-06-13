//! 媒体 Facade — 委托 [`crate::platform::ports::media::MediaServicePort`]（上传与下载）。

use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::session_guard::SessionGuard;
#[cfg(not(target_arch = "wasm32"))]
pub use crate::application::UserFileDownloadRequest;
pub use crate::application::{
    FileDownloadProgress, FileDownloadProgressCallback, UploadPhase, UploadProgress,
    UploadProgressCallback,
};
use crate::domain::{
    MediaCacheAdmin, MediaCacheStatsVo, MediaCacheStore, UploadManifestStore, UserFileDownloadStore,
};
use crate::infrastructure::transport::HttpClient;
use crate::model::{
    MediaAccessUrl, MediaCacheEntryVo, MediaResolvedAccess, UploadOptions, UploadedMedia,
};
use crate::platform::adapters::media::MediaService;
use crate::platform::ports::media::{
    MediaMetadata, MediaServicePort, MediaSourceDescriptor, MediaSourceKind, ProcessedMedia,
    UploadProgressSink,
};
use crate::shared::error::Result;

#[derive(Clone)]
pub struct MediaApi {
    handler: Arc<dyn MediaServicePort>,
    session_guard: SessionGuard,
}

impl MediaApi {
    pub fn from_handler(handler: Arc<dyn MediaServicePort>) -> Self {
        Self {
            handler,
            session_guard: SessionGuard::disabled("media"),
        }
    }

    pub fn from_session_handler(
        handler: Arc<dyn MediaServicePort>,
        current_user_id: Arc<RwLock<String>>,
    ) -> Self {
        Self {
            handler,
            session_guard: SessionGuard::new(current_user_id, "media"),
        }
    }

    pub fn new(
        http: HttpClient,
        current_user_id: Arc<RwLock<String>>,
        upload_manifest_store: Option<Arc<dyn UploadManifestStore>>,
        media_cache_store: Option<Arc<dyn MediaCacheStore>>,
        media_cache_admin: Option<Arc<dyn MediaCacheAdmin>>,
        user_file_download_store: Option<Arc<dyn UserFileDownloadStore>>,
    ) -> Self {
        let handler: Arc<dyn MediaServicePort> = Arc::new(MediaService::new(
            http,
            current_user_id.clone(),
            upload_manifest_store,
            media_cache_store,
            media_cache_admin,
            user_file_download_store,
        ));
        Self {
            handler,
            session_guard: SessionGuard::new(current_user_id, "media"),
        }
    }

    async fn ensure_session_active(&self) -> Result<()> {
        self.session_guard.ensure_active().await
    }

    async fn run_session_bound<T>(&self, operation: impl Future<Output = Result<T>>) -> Result<T> {
        self.session_guard.run(operation).await
    }

    pub async fn upload_file_from_path(
        &self,
        path: impl AsRef<Path>,
        options: Option<UploadOptions>,
    ) -> Result<UploadedMedia> {
        self.ensure_session_active().await?;
        self.upload_file_from_path_with_progress(path, options, None)
            .await
    }

    pub async fn upload_file_from_path_with_progress(
        &self,
        path: impl AsRef<Path>,
        options: Option<UploadOptions>,
        on_progress: Option<UploadProgressCallback>,
    ) -> Result<UploadedMedia> {
        self.ensure_session_active().await?;
        let source = MediaSourceDescriptor::path(path.as_ref().to_string_lossy().to_string());
        self.upload_source_with_progress(source, options, on_progress)
            .await
    }

    pub async fn upload_source(
        &self,
        source: MediaSourceDescriptor,
        options: Option<UploadOptions>,
    ) -> Result<UploadedMedia> {
        self.ensure_session_active().await?;
        self.upload_source_with_progress(source, options, None)
            .await
    }

    pub async fn upload_source_with_progress(
        &self,
        source: MediaSourceDescriptor,
        options: Option<UploadOptions>,
        on_progress: Option<UploadProgressCallback>,
    ) -> Result<UploadedMedia> {
        let metadata = source.metadata.clone().unwrap_or_default();
        let media = ProcessedMedia {
            source,
            metadata,
            payload: None,
        };
        let progress: Option<UploadProgressSink> =
            on_progress.map(|callback| Box::new(move |progress| callback(progress)) as _);
        let handler = self.handler.clone();
        self.run_session_bound(async move { handler.upload(media, options, progress).await })
            .await
    }

    pub async fn upload_bytes(
        &self,
        bytes: Vec<u8>,
        file_name: impl Into<String>,
        mime_type: impl Into<String>,
        options: Option<UploadOptions>,
    ) -> Result<UploadedMedia> {
        self.ensure_session_active().await?;
        self.upload_bytes_with_progress(bytes, file_name, mime_type, options, None)
            .await
    }

    pub async fn upload_bytes_with_progress(
        &self,
        bytes: Vec<u8>,
        file_name: impl Into<String>,
        mime_type: impl Into<String>,
        options: Option<UploadOptions>,
        on_progress: Option<UploadProgressCallback>,
    ) -> Result<UploadedMedia> {
        let metadata = MediaMetadata {
            file_name: file_name.into(),
            mime_type: mime_type.into(),
            size: bytes.len() as u64,
            ..Default::default()
        };
        let source = MediaSourceDescriptor {
            kind: MediaSourceKind::Bytes,
            locator: "memory".to_string(),
            metadata: Some(metadata.clone()),
        };
        let media = ProcessedMedia {
            source,
            metadata,
            payload: Some(bytes),
        };
        let progress: Option<UploadProgressSink> =
            on_progress.map(|callback| Box::new(move |progress| callback(progress)) as _);
        let handler = self.handler.clone();
        self.run_session_bound(async move { handler.upload(media, options, progress).await })
            .await
    }

    pub async fn upload_image_from_path_with_progress(
        &self,
        path: impl AsRef<Path>,
        options: Option<UploadOptions>,
        on_progress: Option<UploadProgressCallback>,
    ) -> Result<UploadedMedia> {
        self.ensure_session_active().await?;
        self.upload_file_from_path_with_progress(path, options, on_progress)
            .await
    }

    pub async fn upload_video_from_path_with_progress(
        &self,
        path: impl AsRef<Path>,
        options: Option<UploadOptions>,
        on_progress: Option<UploadProgressCallback>,
    ) -> Result<UploadedMedia> {
        self.ensure_session_active().await?;
        self.upload_file_from_path_with_progress(path, options, on_progress)
            .await
    }

    pub async fn delete_file(&self, file_id: &str, hard_delete: bool) -> Result<bool> {
        let handler = self.handler.clone();
        self.run_session_bound(async move { handler.delete_file(file_id, hard_delete).await })
            .await
    }

    pub async fn get_file_url(&self, file_id: &str, expires_in: i32) -> Result<MediaAccessUrl> {
        let handler = self.handler.clone();
        self.run_session_bound(async move { handler.get_file_url(file_id, expires_in).await })
            .await
    }

    /// 网关临时直链（`download: true`，适合附件另存为）。
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn get_temp_url_for_file_download(
        &self,
        file_id: &str,
        expires_in: i32,
    ) -> Result<MediaAccessUrl> {
        let handler = self.handler.clone();
        self.run_session_bound(async move {
            handler
                .get_temp_url_for_file_download(file_id, expires_in)
                .await
        })
        .await
    }

    /// 取消进行中的「下载到用户下载目录」任务。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn cancel_user_file_download(&self, download_key: &str) -> bool {
        self.handler.cancel_user_file_download(download_key)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn user_download_get_subfolder(&self) -> Result<String> {
        let handler = self.handler.clone();
        self.run_session_bound(async move { handler.user_download_get_subfolder().await })
            .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn user_download_set_subfolder(&self, name: &str) -> Result<()> {
        let handler = self.handler.clone();
        self.run_session_bound(async move { handler.user_download_set_subfolder(name).await })
            .await
    }

    /// SQLite 中已记录的本地绝对路径（文件是否仍存在需上层校验）。
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn user_download_get_saved_path(&self, download_key: &str) -> Result<Option<String>> {
        let handler = self.handler.clone();
        self.run_session_bound(
            async move { handler.user_download_get_saved_path(download_key).await },
        )
        .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn user_download_delete_record(&self, download_key: &str) -> Result<()> {
        let handler = self.handler.clone();
        self.run_session_bound(
            async move { handler.user_download_delete_record(download_key).await },
        )
        .await
    }

    /// 下载到「系统下载目录 / 子目录」并写入 SQLite；进度经 `on_progress` 回调。
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn download_file_to_user_downloads_folder(
        &self,
        request: UserFileDownloadRequest,
    ) -> Result<String> {
        let handler = self.handler.clone();
        self.run_session_bound(async move {
            handler
                .download_file_to_user_downloads_folder(request)
                .await
        })
        .await
    }

    /// 优先返回本地缓存路径，否则返回网关短时 URL。
    pub async fn resolve_media_access(
        &self,
        file_id: &str,
        expires_in: i32,
    ) -> Result<MediaResolvedAccess> {
        let handler = self.handler.clone();
        self.run_session_bound(
            async move { handler.resolve_media_access(file_id, expires_in).await },
        )
        .await
    }

    /// 下载远程媒体并写入本地缓存与 SQLite 对照表（点击预览等时机调用）。
    pub async fn cache_remote_media(
        &self,
        file_id: &str,
        expires_in: i32,
    ) -> Result<MediaCacheEntryVo> {
        let handler = self.handler.clone();
        self.run_session_bound(async move { handler.cache_remote_media(file_id, expires_in).await })
            .await
    }

    pub async fn media_cache_stats(&self) -> Result<MediaCacheStatsVo> {
        let handler = self.handler.clone();
        self.run_session_bound(async move { handler.media_cache_stats().await })
            .await
    }

    pub async fn set_media_cache_max_bytes(&self, max_bytes: u64) -> Result<()> {
        let handler = self.handler.clone();
        self.run_session_bound(async move { handler.set_media_cache_max_bytes(max_bytes).await })
            .await
    }

    pub async fn set_media_cache_root(&self, absolute_path: Option<&str>) -> Result<()> {
        let handler = self.handler.clone();
        self.run_session_bound(async move { handler.set_media_cache_root(absolute_path).await })
            .await
    }

    pub async fn clear_media_cache(&self) -> Result<()> {
        let handler = self.handler.clone();
        self.run_session_bound(async move { handler.clear_media_cache().await })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::error::ErrorCode;
    use std::time::Duration;
    use tokio::sync::Notify;
    use tokio::time::timeout;

    struct BlockingUploadService {
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[async_trait::async_trait]
    impl MediaServicePort for BlockingUploadService {
        async fn upload(
            &self,
            _media: ProcessedMedia,
            _options: Option<UploadOptions>,
            _progress: Option<UploadProgressSink>,
        ) -> Result<UploadedMedia> {
            self.started.notify_one();
            self.release.notified().await;
            Ok(UploadedMedia {
                file_id: "file_after_logout".to_string(),
                file_name: "after_logout.txt".to_string(),
                mime_type: "text/plain".to_string(),
                size: 5,
                url: None,
                cdn_url: None,
            })
        }
    }

    #[tokio::test]
    async fn in_flight_upload_fails_when_session_changes_before_completion() {
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let current_user_id = Arc::new(RwLock::new("alice".to_string()));
        let api = MediaApi::from_session_handler(
            Arc::new(BlockingUploadService {
                started: started.clone(),
                release: release.clone(),
            }),
            current_user_id.clone(),
        );

        let upload = tokio::spawn(async move {
            api.upload_bytes(b"hello".to_vec(), "hello.txt", "text/plain", None)
                .await
        });

        started.notified().await;
        *current_user_id.write().await = String::new();

        let result = timeout(Duration::from_secs(1), upload)
            .await
            .expect("session change should abort in-flight media operation");
        let err = result
            .expect("upload task should not panic")
            .expect_err("session change must cancel successful media result");
        assert_eq!(err.code(), Some(ErrorCode::NotConnected));
    }
}
