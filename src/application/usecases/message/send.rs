use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::application::commands::SendMessageCommand;
use crate::application::UploadProgressCallback;
use crate::application::MediaService;
use crate::core::CurrentUserIdStore;
use crate::domain::{MessageActor, MessageDraftService, MessageStore};
use crate::error::{ErrorCode, FlareError, Result};
use crate::middleware::MiddlewareChain;
use crate::model::UploadedMedia;
use crate::model::message::{IMMessage, SendAck};
use crate::model::message_elem::{AudioInfoElem, Elem, ImageInfoElem, VideoInfoElem};
use crate::protocol::PacketSender;
use crate::reliable_queue::ReliableSendQueue;
use flare_proto::common::ImageFormat;

pub struct MessageSendUseCase {
    sender: Arc<PacketSender>,
    store: Arc<dyn MessageStore>,
    chain: Arc<MiddlewareChain>,
    current_user_id: CurrentUserIdStore,
    reliable_queue: Option<Arc<ReliableSendQueue>>,
    media_service: Arc<MediaService>,
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
        media_service: Arc<MediaService>,
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
                .execute_via_queue(queue.as_ref())
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
                    if let Some(path) =
                        extract_local_path(&file.file_id).or_else(|| extract_local_path(&file.url))
                    {
                        let uploaded = self
                            .media_service
                            .upload_file_from_path_with_progress(path, None, on_progress.clone())
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
                    let source_path: Option<PathBuf> = image
                        .source
                        .as_ref()
                        .and_then(image_info_local_path)
                        .map(|p| p.to_path_buf());
                    let thumb_path: Option<PathBuf> = image
                        .thumbnail
                        .as_ref()
                        .and_then(image_info_local_path)
                        .map(|p| p.to_path_buf());
                    if let Some(ref path) = source_path {
                        let uploaded = self
                            .media_service
                            .upload_file_from_path_with_progress(path.as_path(), None, on_progress.clone())
                            .await?;
                        let desc = uploaded_media_to_image_descriptor(&uploaded);
                        image.source = desc.clone();
                        match thumb_path.as_ref() {
                            Some(tp) if tp != path => {
                                let uploaded_thumb = self
                                    .media_service
                                    .upload_file_from_path_with_progress(
                                        tp.as_path(),
                                        None,
                                        on_progress.clone(),
                                    )
                                    .await?;
                                image.thumbnail =
                                    uploaded_media_to_image_descriptor(&uploaded_thumb);
                            }
                            _ => {
                                image.thumbnail = image.source.clone();
                            }
                        }
                        touched = true;
                    } else if let Some(ref path) = thumb_path {
                        let uploaded = self
                            .media_service
                            .upload_file_from_path_with_progress(path.as_path(), None, on_progress.clone())
                            .await?;
                        let desc = uploaded_media_to_image_descriptor(&uploaded);
                        image.source = desc.clone();
                        image.thumbnail = desc;
                        touched = true;
                    }
                }
                Elem::Video(video) => {
                    if let Some(path) = extract_local_path(&video.video_id) {
                        let uploaded = self
                            .media_service
                            .upload_file_from_path_with_progress(path, None, on_progress.clone())
                            .await?;
                        video.video_id = uploaded.file_id.clone();
                        video.source = uploaded_media_to_video_descriptor(&uploaded);
                        touched = true;
                    }
                }
                Elem::Audio(audio) => {
                    if let Some(path) = extract_local_path(&audio.audio_id) {
                        let uploaded = self
                            .media_service
                            .upload_file_from_path_with_progress(path, None, on_progress.clone())
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
}

/// 识别「可上传的本地路径」：Unix、`./`、`file://`、`file:///C:/...`（Windows）、`C:\`、UNC `\\`。
fn extract_local_path(input: &str) -> Option<&Path> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if input.starts_with('/') || input.starts_with("./") || input.starts_with("../") {
        return Some(Path::new(input));
    }
    // Windows: C:\path 或 C:/path
    let b = input.as_bytes();
    if input.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return Some(Path::new(input));
    }
    // UNC \\server\share
    if input.starts_with("\\\\") {
        return Some(Path::new(input));
    }
    if let Some(rest) = input.strip_prefix("file://") {
        let path_str = file_url_to_path_str(rest)?;
        return Some(Path::new(path_str));
    }
    None
}

/// `file://` 之后的路径段：`/Users/...` 保留；`/C:/...` 去掉前导 `/` 得到 `C:/...`。
fn file_url_to_path_str(after_scheme: &str) -> Option<&str> {
    if after_scheme.is_empty() {
        return None;
    }
    if after_scheme.starts_with('/')
        && after_scheme.len() >= 3
        && after_scheme.as_bytes()[2] == b':'
        && after_scheme.as_bytes()[1].is_ascii_alphabetic()
    {
        return Some(&after_scheme[1..]);
    }
    Some(after_scheme)
}

fn image_info_local_path(info: &ImageInfoElem) -> Option<&Path> {
    let id = info.image_id.trim();
    if !id.is_empty() {
        if let Some(p) = extract_local_path(id) {
            return Some(p);
        }
    }
    let u = info.uuid.trim();
    if u.is_empty() {
        return None;
    }
    extract_local_path(u)
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
    use super::{extract_local_path, uploaded_media_to_image_descriptor, ImageFormat};
    use crate::model::UploadedMedia;

    #[test]
    fn extract_local_path_accepts_filesystem_inputs() {
        assert_eq!(
            extract_local_path("/tmp/demo.png").map(|path| path.to_string_lossy().to_string()),
            Some("/tmp/demo.png".to_string())
        );
        assert_eq!(
            extract_local_path("./demo.png").map(|path| path.to_string_lossy().to_string()),
            Some("./demo.png".to_string())
        );
        assert_eq!(
            extract_local_path("file:///tmp/demo.png")
                .map(|path| path.to_string_lossy().to_string()),
            Some("/tmp/demo.png".to_string())
        );
        assert_eq!(
            extract_local_path(r"C:\data\demo.png").map(|path| path.to_string_lossy().to_string()),
            Some(r"C:\data\demo.png".to_string())
        );
        assert_eq!(
            extract_local_path("file:///C:/data/demo.png")
                .map(|path| path.to_string_lossy().to_string()),
            Some("C:/data/demo.png".to_string())
        );
    }

    #[test]
    fn extract_local_path_rejects_remote_and_empty_inputs() {
        assert!(extract_local_path("").is_none());
        assert!(extract_local_path("https://example.com/demo.png").is_none());
        assert!(extract_local_path("media-file-id").is_none());
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
