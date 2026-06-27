//! Upload-only media service adapter.

use std::sync::Arc;

use async_trait::async_trait;

use crate::model::{UploadOptions, UploadedMedia};
use crate::platform::ports::media::{
    MediaProcessorPort, MediaServicePort, MediaUploaderPort, ProcessedMedia, UploadProgressSink,
};
use crate::shared::error::Result;

/// Adapter used when a host can upload media but does not implement media
/// cache/download administration yet.
pub struct UploadOnlyMediaService {
    uploader: Arc<dyn MediaUploaderPort>,
    processor: Option<Arc<dyn MediaProcessorPort>>,
}

impl UploadOnlyMediaService {
    pub fn new(uploader: Arc<dyn MediaUploaderPort>) -> Self {
        Self {
            uploader,
            processor: None,
        }
    }

    pub fn with_processor(
        uploader: Arc<dyn MediaUploaderPort>,
        processor: Arc<dyn MediaProcessorPort>,
    ) -> Self {
        Self {
            uploader,
            processor: Some(processor),
        }
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
        let media = match &self.processor {
            Some(processor) => {
                processor
                    .prepare_upload(media.source, options.clone())
                    .await?
            }
            None => media,
        };
        self.uploader.upload(media, options, progress).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, Ordering};

    use crate::platform::ports::media::{MediaMetadata, MediaSourceDescriptor};

    struct RecordingProcessor {
        called: Arc<AtomicBool>,
    }

    #[async_trait]
    impl MediaProcessorPort for RecordingProcessor {
        async fn inspect(&self, _source: &MediaSourceDescriptor) -> Result<MediaMetadata> {
            Ok(MediaMetadata {
                file_name: "processed.jpg".to_string(),
                mime_type: "image/jpeg".to_string(),
                size: 7,
                ..Default::default()
            })
        }

        async fn prepare_upload(
            &self,
            source: MediaSourceDescriptor,
            _options: Option<UploadOptions>,
        ) -> Result<ProcessedMedia> {
            self.called.store(true, Ordering::SeqCst);
            Ok(ProcessedMedia {
                source,
                metadata: self
                    .inspect(&MediaSourceDescriptor::path("ignored"))
                    .await?,
                payload: Some(vec![1, 2, 3]),
            })
        }
    }

    struct RecordingUploader {
        saw_payload: Arc<AtomicBool>,
    }

    #[async_trait]
    impl MediaUploaderPort for RecordingUploader {
        async fn upload(
            &self,
            media: ProcessedMedia,
            _options: Option<UploadOptions>,
            _progress: Option<UploadProgressSink>,
        ) -> Result<UploadedMedia> {
            self.saw_payload.store(
                media.payload.as_deref() == Some(&[1, 2, 3]),
                Ordering::SeqCst,
            );
            Ok(UploadedMedia {
                file_id: "file-1".to_string(),
                file_name: media.metadata.file_name,
                mime_type: media.metadata.mime_type,
                size: media.metadata.size as i64,
                url: None,
                cdn_url: None,
            })
        }
    }

    #[tokio::test]
    async fn upload_only_service_runs_processor_before_uploader() {
        let processor_called = Arc::new(AtomicBool::new(false));
        let uploader_saw_payload = Arc::new(AtomicBool::new(false));
        let service = UploadOnlyMediaService::with_processor(
            Arc::new(RecordingUploader {
                saw_payload: uploader_saw_payload.clone(),
            }),
            Arc::new(RecordingProcessor {
                called: processor_called.clone(),
            }),
        );

        let uploaded = service
            .upload(
                ProcessedMedia {
                    source: MediaSourceDescriptor::path("/tmp/a.jpg"),
                    metadata: MediaMetadata::default(),
                    payload: None,
                },
                None,
                None,
            )
            .await
            .expect("upload");

        assert_eq!(uploaded.file_name, "processed.jpg");
        assert!(processor_called.load(Ordering::SeqCst));
        assert!(uploader_saw_payload.load(Ordering::SeqCst));
    }
}
