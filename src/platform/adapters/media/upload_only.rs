//! Upload-only media service adapter.

use std::sync::Arc;

use async_trait::async_trait;

use crate::model::{UploadOptions, UploadedMedia};
use crate::platform::ports::media::{
    MediaServicePort, MediaUploaderPort, ProcessedMedia, UploadProgressSink,
};
use crate::shared::error::Result;

/// Adapter used when a host can upload media but does not implement media
/// cache/download administration yet.
pub struct UploadOnlyMediaService {
    uploader: Arc<dyn MediaUploaderPort>,
}

impl UploadOnlyMediaService {
    pub fn new(uploader: Arc<dyn MediaUploaderPort>) -> Self {
        Self { uploader }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl MediaServicePort for UploadOnlyMediaService {
    async fn upload(
        &self,
        media: ProcessedMedia,
        options: Option<UploadOptions>,
        progress: Option<UploadProgressSink>,
    ) -> Result<UploadedMedia> {
        self.uploader.upload(media, options, progress).await
    }
}
