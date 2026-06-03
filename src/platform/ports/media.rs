//! Media ports.
//!
//! Media is platform-specific at the edges: browsers provide File/Blob,
//! mobile platforms provide URI or asset identifiers, and desktop/native
//! platforms often provide filesystem paths. The core only depends on this
//! normalized contract.

use async_trait::async_trait;
use std::collections::HashMap;

use crate::application::callbacks::{UploadProgress, UserFileDownloadRequest};
use crate::domain::MediaCacheStatsVo;
use crate::model::{
    MediaAccessUrl, MediaCacheEntryVo, MediaResolvedAccess, UploadOptions, UploadedMedia,
};
use crate::shared::error::{ErrorCode, FlareError, Result};

#[derive(Debug, Clone, Default)]
pub struct MediaMetadata {
    pub file_name: String,
    pub mime_type: String,
    pub size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub extra: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub enum MediaSourceKind {
    Path,
    Uri,
    Blob,
    Bytes,
    Asset,
    Custom(String),
}

#[derive(Debug, Clone)]
pub struct MediaSourceDescriptor {
    pub kind: MediaSourceKind,
    pub locator: String,
    pub metadata: Option<MediaMetadata>,
}

impl MediaSourceDescriptor {
    pub fn path(locator: impl Into<String>) -> Self {
        Self {
            kind: MediaSourceKind::Path,
            locator: locator.into(),
            metadata: None,
        }
    }

    pub fn uri(locator: impl Into<String>) -> Self {
        Self {
            kind: MediaSourceKind::Uri,
            locator: locator.into(),
            metadata: None,
        }
    }

    pub fn blob(locator: impl Into<String>) -> Self {
        Self {
            kind: MediaSourceKind::Blob,
            locator: locator.into(),
            metadata: None,
        }
    }

    pub fn bytes(locator: impl Into<String>, metadata: MediaMetadata) -> Self {
        Self {
            kind: MediaSourceKind::Bytes,
            locator: locator.into(),
            metadata: Some(metadata),
        }
    }

    pub fn asset(locator: impl Into<String>) -> Self {
        Self {
            kind: MediaSourceKind::Asset,
            locator: locator.into(),
            metadata: None,
        }
    }

    pub fn custom(kind: impl Into<String>, locator: impl Into<String>) -> Self {
        Self {
            kind: MediaSourceKind::Custom(kind.into()),
            locator: locator.into(),
            metadata: None,
        }
    }

    pub fn with_metadata(mut self, metadata: MediaMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

#[derive(Debug, Clone)]
pub struct ProcessedMedia {
    pub source: MediaSourceDescriptor,
    pub metadata: MediaMetadata,
    pub payload: Option<Vec<u8>>,
}

#[async_trait]
pub trait MediaProcessorPort: Send + Sync {
    async fn inspect(&self, source: &MediaSourceDescriptor) -> Result<MediaMetadata>;

    async fn prepare_upload(
        &self,
        source: MediaSourceDescriptor,
        options: Option<UploadOptions>,
    ) -> Result<ProcessedMedia>;
}

pub type UploadProgressSink = Box<dyn Fn(UploadProgress) + Send + Sync>;

#[async_trait]
pub trait MediaUploaderPort: Send + Sync {
    async fn upload(
        &self,
        media: ProcessedMedia,
        options: Option<UploadOptions>,
        progress: Option<UploadProgressSink>,
    ) -> Result<UploadedMedia>;
}

/// High-level media service port consumed by client facades and message
/// use-cases.
///
/// Platform adapters may implement only upload and return the default
/// unsupported errors for cache/download management, but the core never needs
/// to know whether the implementation is native files, Web Blob, RN URI,
/// uni-app temp file, Android content URI, iOS asset, or Flutter plugin IO.
#[async_trait]
pub trait MediaServicePort: Send + Sync {
    async fn upload(
        &self,
        media: ProcessedMedia,
        options: Option<UploadOptions>,
        progress: Option<UploadProgressSink>,
    ) -> Result<UploadedMedia>;

    async fn delete_file(&self, _file_id: &str, _hard_delete: bool) -> Result<bool> {
        Err(unsupported_media_operation("delete_file"))
    }

    async fn get_file_url(&self, _file_id: &str, _expires_in: i32) -> Result<MediaAccessUrl> {
        Err(unsupported_media_operation("get_file_url"))
    }

    async fn get_temp_url_for_file_download(
        &self,
        _file_id: &str,
        _expires_in: i32,
    ) -> Result<MediaAccessUrl> {
        Err(unsupported_media_operation(
            "get_temp_url_for_file_download",
        ))
    }

    async fn resolve_media_access(
        &self,
        _file_id: &str,
        _expires_in: i32,
    ) -> Result<MediaResolvedAccess> {
        Err(unsupported_media_operation("resolve_media_access"))
    }

    async fn cache_remote_media(
        &self,
        _file_id: &str,
        _expires_in: i32,
    ) -> Result<MediaCacheEntryVo> {
        Err(unsupported_media_operation("cache_remote_media"))
    }

    async fn media_cache_stats(&self) -> Result<MediaCacheStatsVo> {
        Err(unsupported_media_operation("media_cache_stats"))
    }

    async fn set_media_cache_max_bytes(&self, _max_bytes: u64) -> Result<()> {
        Err(unsupported_media_operation("set_media_cache_max_bytes"))
    }

    async fn set_media_cache_root(&self, _absolute_path: Option<&str>) -> Result<()> {
        Err(unsupported_media_operation("set_media_cache_root"))
    }

    async fn clear_media_cache(&self) -> Result<()> {
        Err(unsupported_media_operation("clear_media_cache"))
    }

    fn cancel_user_file_download(&self, _download_key: &str) -> bool {
        false
    }

    async fn user_download_get_subfolder(&self) -> Result<String> {
        Err(unsupported_media_operation("user_download_get_subfolder"))
    }

    async fn user_download_set_subfolder(&self, _name: &str) -> Result<()> {
        Err(unsupported_media_operation("user_download_set_subfolder"))
    }

    async fn user_download_get_saved_path(&self, _download_key: &str) -> Result<Option<String>> {
        Err(unsupported_media_operation("user_download_get_saved_path"))
    }

    async fn user_download_delete_record(&self, _download_key: &str) -> Result<()> {
        Err(unsupported_media_operation("user_download_delete_record"))
    }

    async fn download_file_to_user_downloads_folder(
        &self,
        _request: UserFileDownloadRequest,
    ) -> Result<String> {
        Err(unsupported_media_operation(
            "download_file_to_user_downloads_folder",
        ))
    }
}

pub fn unsupported_media_operation(operation: &str) -> FlareError {
    FlareError::localized(
        ErrorCode::OperationNotSupported,
        format!("{operation} is not supported by the configured media adapter"),
    )
}
