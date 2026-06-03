use std::sync::Arc;

use crate::application::UploadProgressCallback;
use crate::application::commands::SendMessageCommand;
use crate::core::CurrentUserIdStore;
use crate::core::ReliableSendQueue;
use crate::domain::{MessageActor, MessageDraftService, MessageStore};
use crate::extension::middleware::MiddlewareChain;
use crate::infrastructure::protocol::PacketSender;
use crate::model::UploadedMedia;
use crate::model::message::{IMMessage, SendAck};
use crate::model::message_elem::{AudioInfoElem, Elem, ImageInfoElem, VideoInfoElem};
use crate::platform::ports::media::{
    MediaServicePort, MediaSourceDescriptor, ProcessedMedia, UploadProgressSink,
};
use crate::shared::error::{ErrorCode, FlareError, Result};
use flare_proto::common::ImageFormat;

pub struct MessageSendUseCase {
    sender: Arc<PacketSender>,
    store: Arc<dyn MessageStore>,
    chain: Arc<MiddlewareChain>,
    current_user_id: CurrentUserIdStore,
    reliable_queue: Option<Arc<ReliableSendQueue>>,
    media_service: Arc<dyn MediaServicePort>,
    draft_service: MessageDraftService,
}

impl MessageSendUseCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sender: Arc<PacketSender>,
        store: Arc<dyn MessageStore>,
        chain: Arc<MiddlewareChain>,
        current_user_id: CurrentUserIdStore,
        reliable_queue: Option<Arc<ReliableSendQueue>>,
        media_service: Arc<dyn MediaServicePort>,
    ) -> Self {
        Self {
            sender,
            store,
            chain,
            current_user_id,
            reliable_queue,
            media_service,
            draft_service: MessageDraftService::default(),
        }
    }

    pub async fn current_user_id(&self) -> Result<String> {
        let uid = self.current_user_id.read().await.clone();
        if uid.is_empty() {
            return Err(FlareError::localized(ErrorCode::NotConnected, "未连接"));
        }
        Ok(uid)
    }

    async fn actor(&self) -> Result<MessageActor> {
        MessageActor::require(self.current_user_id().await?)
    }

    pub async fn send(&self, message: IMMessage) -> Result<SendAck> {
        let actor = self.actor().await?;
        let message = self
            .draft_service
            .prepare_outbound_message(&actor, message)?;
        if let Some(queue) = &self.reliable_queue {
            SendMessageCommand::new(message.clone())
                .execute_via_queue(queue.as_ref(), &self.chain)
                .await?;
            Ok(SendAck {
                client_msg_id: message.client_msg_id,
                server_msg_id: String::new(),
                seq: 0,
                conversation_id: message.conversation_id,
                success: true,
                ..Default::default()
            })
        } else {
            SendMessageCommand::new(message)
                .execute(&self.sender, self.store.as_ref(), &self.chain)
                .await
        }
    }

    pub async fn send_with_media(
        &self,
        message: IMMessage,
        on_progress: Option<UploadProgressCallback>,
    ) -> Result<SendAck> {
        let normalized = self
            .normalize_message_media_with_progress(message, on_progress)
            .await?;
        self.send(normalized).await
    }

    async fn normalize_message_media_with_progress(
        &self,
        mut message: IMMessage,
        on_progress: Option<UploadProgressCallback>,
    ) -> Result<IMMessage> {
        let mut touched = false;
        if let Some(ref mut content) = message.content {
            match content {
                Elem::File(file) => {
                    if let Some(source) = extract_media_source(&file.file_id)
                        .or_else(|| extract_media_source(&file.url))
                    {
                        let uploaded = self
                            .upload_media_source(source, on_progress.clone())
                            .await?;
                        file.file_id = uploaded.file_id;
                        if !uploaded.file_name.is_empty() {
                            file.file_name = uploaded.file_name;
                        }
                        if !uploaded.mime_type.is_empty() {
                            file.mime_type = uploaded.mime_type;
                        }
                        if uploaded.size > 0 {
                            file.file_size = uploaded.size;
                        }
                        file.url.clear();
                        touched = true;
                    }
                }
                Elem::Image(image) => {
                    let source_media: Option<MediaSourceDescriptor> =
                        image.source.as_ref().and_then(image_info_media_source);
                    let thumb_media: Option<MediaSourceDescriptor> =
                        image.thumbnail.as_ref().and_then(image_info_media_source);
                    if let Some(source) = source_media {
                        let uploaded = self
                            .upload_media_source(source.clone(), on_progress.clone())
                            .await?;
                        let desc = uploaded_media_to_image_descriptor(&uploaded);
                        image.source = desc.clone();
                        match thumb_media {
                            Some(thumb) if !same_media_source(&thumb, &source) => {
                                let uploaded_thumb =
                                    self.upload_media_source(thumb, on_progress.clone()).await?;
                                image.thumbnail =
                                    uploaded_media_to_image_descriptor(&uploaded_thumb);
                            }
                            _ => {
                                image.thumbnail = image.source.clone();
                            }
                        }
                        touched = true;
                    } else if let Some(thumb) = thumb_media {
                        let uploaded = self.upload_media_source(thumb, on_progress.clone()).await?;
                        let desc = uploaded_media_to_image_descriptor(&uploaded);
                        image.source = desc.clone();
                        image.thumbnail = desc;
                        touched = true;
                    }
                }
                Elem::Video(video) => {
                    if let Some(source) = extract_media_source(&video.video_id).or_else(|| {
                        video
                            .source
                            .as_ref()
                            .and_then(|info| extract_media_source(&info.url))
                    }) {
                        let uploaded = self
                            .upload_media_source(source, on_progress.clone())
                            .await?;
                        video.video_id = uploaded.file_id.clone();
                        video.source = uploaded_media_to_video_descriptor(&uploaded);
                        touched = true;
                    }
                }
                Elem::Audio(audio) => {
                    if let Some(source) = extract_media_source(&audio.audio_id).or_else(|| {
                        audio
                            .source
                            .as_ref()
                            .and_then(|info| extract_media_source(&info.url))
                    }) {
                        let uploaded = self
                            .upload_media_source(source, on_progress.clone())
                            .await?;
                        audio.audio_id = uploaded.file_id.clone();
                        audio.source = uploaded_media_to_audio_descriptor(&uploaded);
                        touched = true;
                    }
                }
                _ => {}
            }
        }
        if touched {
            message.content_bytes.clear();
        }
        Ok(message)
    }

    async fn upload_media_source(
        &self,
        source: MediaSourceDescriptor,
        on_progress: Option<UploadProgressCallback>,
    ) -> Result<UploadedMedia> {
        let media = ProcessedMedia {
            source,
            metadata: Default::default(),
            payload: None,
        };
        let progress: Option<UploadProgressSink> =
            on_progress.map(|callback| Box::new(move |progress| callback(progress)) as _);
        self.media_service.upload(media, None, progress).await
    }
}

/// 识别「可上传媒体源」：Unix/Windows path、`file://`、Android `content://`、
/// iOS `ph://`/`assets-library://`、RN/uni-app 临时 URI、Web `blob:`/`data:`。
fn extract_media_source(input: &str) -> Option<MediaSourceDescriptor> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if input.starts_with('/') || input.starts_with("./") || input.starts_with("../") {
        return Some(MediaSourceDescriptor::path(input));
    }
    // Windows: C:\path 或 C:/path
    let b = input.as_bytes();
    if input.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return Some(MediaSourceDescriptor::path(input));
    }
    // UNC \\server\share
    if input.starts_with("\\\\") {
        return Some(MediaSourceDescriptor::path(input));
    }
    if let Some(rest) = input.strip_prefix("file://") {
        let path_str = file_url_to_path_string(rest)?;
        return Some(MediaSourceDescriptor::path(path_str));
    }
    if input.starts_with("content://")
        || input.starts_with("rnfs://")
        || input.starts_with("wxfile://")
        || input.starts_with("uni://")
    {
        return Some(MediaSourceDescriptor::uri(input));
    }
    if input.starts_with("ph://")
        || input.starts_with("assets-library://")
        || input.starts_with("asset://")
        || input.starts_with("assets://")
    {
        return Some(MediaSourceDescriptor::asset(input));
    }
    if input.starts_with("blob:") {
        return Some(MediaSourceDescriptor::blob(input));
    }
    if input.starts_with("data:") {
        return Some(MediaSourceDescriptor::bytes(input, Default::default()));
    }
    None
}

/// `file://` 之后的路径段：`/Users/...` 保留；`/C:/...` 去掉前导 `/` 得到 `C:/...`。
fn file_url_to_path_string(after_scheme: &str) -> Option<String> {
    if after_scheme.is_empty() {
        return None;
    }
    if after_scheme.starts_with('/')
        && after_scheme.len() >= 3
        && after_scheme.as_bytes()[2] == b':'
        && after_scheme.as_bytes()[1].is_ascii_alphabetic()
    {
        return Some(after_scheme[1..].to_string());
    }
    Some(after_scheme.to_string())
}

fn image_info_media_source(info: &ImageInfoElem) -> Option<MediaSourceDescriptor> {
    let id = info.image_id.trim();
    if !id.is_empty()
        && let Some(source) = extract_media_source(id)
    {
        return Some(source);
    }
    let u = info.uuid.trim();
    if !u.is_empty()
        && let Some(source) = extract_media_source(u)
    {
        return Some(source);
    }
    extract_media_source(&info.url)
}

fn same_media_source(a: &MediaSourceDescriptor, b: &MediaSourceDescriptor) -> bool {
    std::mem::discriminant(&a.kind) == std::mem::discriminant(&b.kind) && a.locator == b.locator
}

fn uploaded_media_to_image_descriptor(uploaded: &UploadedMedia) -> Option<ImageInfoElem> {
    if uploaded.file_id.trim().is_empty() {
        return None;
    }
    let mime_lower = uploaded.mime_type.to_lowercase();
    let (format, animated) = if mime_lower.contains("gif") {
        (ImageFormat::Gif as i32, true)
    } else if mime_lower.contains("png") && mime_lower.contains("apng") {
        (ImageFormat::Apng as i32, true)
    } else if mime_lower.contains("png") {
        (ImageFormat::Png as i32, false)
    } else if mime_lower.contains("jpeg") || mime_lower.contains("jpg") {
        (ImageFormat::Jpeg as i32, false)
    } else if mime_lower.contains("webp") {
        (ImageFormat::Webp as i32, false)
    } else if mime_lower.contains("bmp") {
        (ImageFormat::Bmp as i32, false)
    } else if mime_lower.contains("heic") || mime_lower.contains("heif") {
        (ImageFormat::Heic as i32, false)
    } else if mime_lower.contains("svg") {
        (ImageFormat::Svg as i32, false)
    } else {
        (ImageFormat::Unspecified as i32, false)
    };
    Some(ImageInfoElem {
        uuid: uploaded.file_id.clone(),
        image_id: uploaded.file_id.clone(),
        url: String::new(),
        mime_type: uploaded.mime_type.clone(),
        size: uploaded.size,
        width: 0,
        height: 0,
        format,
        animated,
    })
}

fn uploaded_media_to_video_descriptor(uploaded: &UploadedMedia) -> Option<VideoInfoElem> {
    if uploaded.file_id.trim().is_empty() {
        return None;
    }
    Some(VideoInfoElem {
        uuid: uploaded.file_id.clone(),
        url: String::new(),
        mime_type: uploaded.mime_type.clone(),
        size: uploaded.size,
        duration_ms: 0,
        width: 0,
        height: 0,
    })
}

fn uploaded_media_to_audio_descriptor(uploaded: &UploadedMedia) -> Option<AudioInfoElem> {
    if uploaded.file_id.trim().is_empty() {
        return None;
    }
    Some(AudioInfoElem {
        uuid: uploaded.file_id.clone(),
        url: String::new(),
        mime_type: uploaded.mime_type.clone(),
        size: uploaded.size,
        duration_ms: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::{ImageFormat, extract_media_source, uploaded_media_to_image_descriptor};
    use crate::model::UploadedMedia;
    use crate::platform::ports::media::MediaSourceKind;

    #[test]
    fn extract_media_source_accepts_filesystem_inputs() {
        assert_eq!(
            extract_media_source("/tmp/demo.png").map(|source| source.locator),
            Some("/tmp/demo.png".to_string())
        );
        assert_eq!(
            extract_media_source("./demo.png").map(|source| source.locator),
            Some("./demo.png".to_string())
        );
        assert_eq!(
            extract_media_source("file:///tmp/demo.png").map(|source| source.locator),
            Some("/tmp/demo.png".to_string())
        );
        assert_eq!(
            extract_media_source(r"C:\data\demo.png").map(|source| source.locator),
            Some(r"C:\data\demo.png".to_string())
        );
        assert_eq!(
            extract_media_source("file:///C:/data/demo.png").map(|source| source.locator),
            Some("C:/data/demo.png".to_string())
        );
    }

    #[test]
    fn extract_media_source_accepts_host_platform_inputs() {
        assert!(matches!(
            extract_media_source("content://media/external/images/1").map(|source| source.kind),
            Some(MediaSourceKind::Uri)
        ));
        assert!(matches!(
            extract_media_source("ph://asset-id").map(|source| source.kind),
            Some(MediaSourceKind::Asset)
        ));
        assert!(matches!(
            extract_media_source("blob:https://example.com/id").map(|source| source.kind),
            Some(MediaSourceKind::Blob)
        ));
        assert!(matches!(
            extract_media_source("data:image/png;base64,AAA").map(|source| source.kind),
            Some(MediaSourceKind::Bytes)
        ));
    }

    #[test]
    fn extract_media_source_rejects_remote_and_empty_inputs() {
        assert!(extract_media_source("").is_none());
        assert!(extract_media_source("https://example.com/demo.png").is_none());
        assert!(extract_media_source("media-file-id").is_none());
    }

    #[test]
    fn uploaded_image_descriptor_keeps_stable_metadata_without_short_url() {
        let uploaded = UploadedMedia {
            file_id: "img-1".to_string(),
            file_name: "demo.png".to_string(),
            mime_type: "image/png".to_string(),
            size: 123,
            url: Some("https://origin.example.com/demo.png".to_string()),
            cdn_url: Some("https://cdn.example.com/demo.png".to_string()),
        };

        let source = uploaded_media_to_image_descriptor(&uploaded).expect("image descriptor");

        assert_eq!(source.uuid, "img-1");
        assert_eq!(source.image_id, "img-1");
        assert_eq!(source.url, "");
        assert_eq!(source.mime_type, "image/png");
        assert_eq!(source.size, 123);
        assert_eq!(source.format, ImageFormat::Png as i32);
        assert!(!source.animated);
    }
}
