use std::sync::Arc;

use crate::application::commands::SendMessageCommand;
use crate::application::{UploadPhase, UploadProgress, UploadProgressCallback};
use crate::content::message_elem::{AudioInfoElem, Elem, ImageInfoElem, VideoInfoElem};
use crate::domain::{
    ConversationIdentityService, ConversationStore, MessageActor, MessageDraftService, MessageStore,
};
use crate::extension::middleware::MiddlewareChain;
use crate::infrastructure::protocol::PacketSender;
use crate::kernel::event::{EventBus, MessageEvent, SdkEvent};
use crate::kernel::{CurrentUserIdStore, ReliableSendQueuePort};
use crate::model::UploadedMedia;
use crate::model::conversation::ConversationType;
use crate::model::message::{IMMessage, MessageLocalState, MessageStatus, SendAck};
use crate::platform::ports::media::{
    MediaMetadata, MediaServicePort, MediaSourceDescriptor, MediaSourceKind, ProcessedMedia,
    UploadProgressSink,
};
use crate::shared::error::{ErrorCode, FlareError, Result};
use crate::shared::util::spawn_background;
use base64::Engine as _;
use flare_proto::common::ImageFormat;

pub struct MessageSendUseCase {
    sender: Arc<PacketSender>,
    store: Arc<dyn MessageStore>,
    conversation_store: Arc<dyn ConversationStore>,
    chain: Arc<MiddlewareChain>,
    current_user_id: CurrentUserIdStore,
    reliable_queue: Option<Arc<dyn ReliableSendQueuePort>>,
    media_service: Arc<dyn MediaServicePort>,
    bus: EventBus,
    draft_service: MessageDraftService,
    conversation_identity: ConversationIdentityService,
}

impl MessageSendUseCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sender: Arc<PacketSender>,
        store: Arc<dyn MessageStore>,
        conversation_store: Arc<dyn ConversationStore>,
        chain: Arc<MiddlewareChain>,
        current_user_id: CurrentUserIdStore,
        reliable_queue: Option<Arc<dyn ReliableSendQueuePort>>,
        media_service: Arc<dyn MediaServicePort>,
        bus: EventBus,
    ) -> Self {
        Self {
            sender,
            store,
            conversation_store,
            chain,
            current_user_id,
            reliable_queue,
            media_service,
            bus,
            draft_service: MessageDraftService::default(),
            conversation_identity: ConversationIdentityService,
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
        let (_actor, message) = self.prepare_outbound_identity(message).await?;
        self.draft_service.validate_outbound_message(&message)?;
        self.dispatch_prepared_message(message).await
    }

    async fn prepare_outbound_identity(
        &self,
        message: IMMessage,
    ) -> Result<(MessageActor, IMMessage)> {
        let actor = self.actor().await?;
        let mut message = message;
        if let Some(rewrite) = ConversationIdentityService::canonicalize_single_chat_message(
            &mut message,
            &actor.user_id,
        ) {
            self.store
                .rewrite_conversation_id(&rewrite.from, &rewrite.to)
                .await?;
            self.conversation_store
                .merge_conversation_identity(&rewrite.from, &rewrite.to)
                .await?;
        }
        let message = self
            .draft_service
            .prepare_outbound_identity(&actor, message)?;
        Ok((actor, message))
    }

    async fn dispatch_prepared_message(&self, message: IMMessage) -> Result<SendAck> {
        self.ensure_optimistic_conversation(&message).await?;
        if let Some(queue) = &self.reliable_queue {
            SendMessageCommand::new(message.clone())
                .execute_via_queue(queue.as_ref(), &self.chain)
                .await?;
            Ok(SendAck {
                client_msg_id: message.client_msg_id,
                conversation_id: message.conversation_id,
                ack_id: None,
                result: None,
            })
        } else {
            SendMessageCommand::new(message)
                .execute(&self.sender, self.store.as_ref(), &self.chain)
                .await
        }
    }

    async fn ensure_optimistic_conversation(&self, message: &IMMessage) -> Result<()> {
        let conversation_id = message.conversation_id.trim();
        if conversation_id.is_empty() {
            return Ok(());
        }
        if self
            .conversation_store
            .get(conversation_id)
            .await?
            .is_some()
        {
            return Ok(());
        }

        let conversation_type = ConversationType::from_proto_int(message.conversation_type);
        if matches!(conversation_type, ConversationType::Unspecified) {
            return Ok(());
        }

        let channel_id = message.channel_id.trim();
        if channel_id.is_empty() {
            return Ok(());
        }

        let business_type = message
            .attributes
            .get("business_type")
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| conversation_type.as_str());

        let mut conversation = self.conversation_identity.build_local_conversation(
            conversation_id,
            Some(channel_id),
            conversation_type,
            business_type,
            channel_id.to_string(),
        );
        conversation.updated_at = message.created_at;
        conversation.created_at = conversation.updated_at;
        self.conversation_store.save_batch(&[conversation]).await
    }

    pub async fn send_with_media(
        &self,
        message: IMMessage,
        on_progress: Option<UploadProgressCallback>,
    ) -> Result<SendAck> {
        let (_actor, mut message) = self.prepare_outbound_identity(message).await?;
        let sort_ts = message.client_created_at.max(message.created_at);
        message.status = MessageStatus::Created as i32;
        message.local_state = MessageLocalState {
            sending: true,
            failed: false,
            is_local: true,
            uploading: true,
            upload_progress: 0,
            sort_ts,
        };
        attach_local_media_preview(&mut message);
        message.materialize_encoded_content_from_elem();
        self.ensure_optimistic_conversation(&message).await?;
        persist_local_message_update(&self.store, &self.conversation_store, &self.bus, &message)
            .await?;

        let client_msg_id = message.client_msg_id.clone();
        let progress = self.upload_progress_callback(client_msg_id.clone(), on_progress);
        let normalized = match self
            .normalize_message_media_with_progress(message.clone(), Some(progress))
            .await
        {
            Ok(mut normalized) => {
                normalized.status = MessageStatus::Created as i32;
                normalized.local_state = MessageLocalState {
                    sending: true,
                    failed: false,
                    is_local: true,
                    uploading: false,
                    upload_progress: 100,
                    sort_ts,
                };
                normalized.materialize_encoded_content_from_elem();
                persist_local_message_update(
                    &self.store,
                    &self.conversation_store,
                    &self.bus,
                    &normalized,
                )
                .await?;
                normalized
            }
            Err(error) => {
                let mut failed = self
                    .store
                    .get_by_client_msg_id(&client_msg_id)
                    .await?
                    .unwrap_or(message);
                failed.status = MessageStatus::Failed as i32;
                failed.local_state = MessageLocalState {
                    sending: false,
                    failed: true,
                    is_local: true,
                    uploading: false,
                    upload_progress: failed.local_state.upload_progress,
                    sort_ts,
                };
                persist_local_message_update(
                    &self.store,
                    &self.conversation_store,
                    &self.bus,
                    &failed,
                )
                .await?;
                self.bus
                    .publish(SdkEvent::Message(MessageEvent::SendFailed {
                        client_msg_id,
                        reason: error.to_string(),
                    }));
                return Err(error);
            }
        };

        self.draft_service.validate_outbound_message(&normalized)?;
        match self.dispatch_prepared_message(normalized.clone()).await {
            Ok(ack) => Ok(ack),
            Err(error) => {
                let mut failed = normalized;
                failed.status = MessageStatus::Failed as i32;
                failed.local_state = MessageLocalState {
                    sending: false,
                    failed: true,
                    is_local: true,
                    uploading: false,
                    upload_progress: 100,
                    sort_ts,
                };
                persist_local_message_update(
                    &self.store,
                    &self.conversation_store,
                    &self.bus,
                    &failed,
                )
                .await?;
                self.bus
                    .publish(SdkEvent::Message(MessageEvent::SendFailed {
                        client_msg_id: failed.client_msg_id.clone(),
                        reason: error.to_string(),
                    }));
                Err(error)
            }
        }
    }

    fn upload_progress_callback(
        &self,
        client_msg_id: String,
        external: Option<UploadProgressCallback>,
    ) -> UploadProgressCallback {
        let store = self.store.clone();
        let conversation_store = self.conversation_store.clone();
        let bus = self.bus.clone();
        Arc::new(move |progress: UploadProgress| {
            if let Some(callback) = external.as_ref() {
                callback(progress.clone());
            }
            let next_progress = upload_progress_percent(&progress);
            let client_msg_id = client_msg_id.clone();
            let store = store.clone();
            let conversation_store = conversation_store.clone();
            let bus = bus.clone();
            spawn_background(async move {
                let Ok(Some(mut current)) = store.get_by_client_msg_id(&client_msg_id).await else {
                    return;
                };
                if current.local_state.failed || current.status == MessageStatus::Failed as i32 {
                    return;
                }
                if !current.local_state.uploading && current.local_state.upload_progress >= 100 {
                    return;
                }
                let next_progress = if matches!(progress.phase, UploadPhase::Finished) {
                    100
                } else {
                    next_progress.min(99)
                };
                if next_progress < current.local_state.upload_progress {
                    return;
                }
                current.local_state.sending = true;
                current.local_state.failed = false;
                current.local_state.is_local = true;
                current.local_state.uploading = true;
                current.local_state.upload_progress = next_progress;
                let _ =
                    persist_local_message_update(&store, &conversation_store, &bus, &current).await;
            });
        })
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
                        let display_url = uploaded_media_display_url(&uploaded);
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
                        file.url = display_url;
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
                Elem::ImageGroup(group) => {
                    for image in &mut group.images {
                        if let Some(source) = image_info_media_source(image) {
                            let uploaded = self
                                .upload_media_source(source, on_progress.clone())
                                .await?;
                            if let Some(desc) = uploaded_media_to_image_descriptor(&uploaded) {
                                *image = desc;
                                touched = true;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if touched {
            message.encoded_content.clear();
        }
        Ok(message)
    }

    async fn upload_media_source(
        &self,
        source: MediaSourceDescriptor,
        on_progress: Option<UploadProgressCallback>,
    ) -> Result<UploadedMedia> {
        let media = processed_media_from_source(source)?;
        let progress: Option<UploadProgressSink> =
            on_progress.map(|callback| Box::new(move |progress| callback(progress)) as _);
        self.media_service.upload(media, None, progress).await
    }
}

async fn persist_local_message_update(
    store: &Arc<dyn MessageStore>,
    conversation_store: &Arc<dyn ConversationStore>,
    bus: &EventBus,
    message: &IMMessage,
) -> Result<()> {
    store.save_one(message).await?;
    update_local_conversation_projection(conversation_store, message).await?;
    bus.publish(SdkEvent::Message(MessageEvent::ReceivedBatch {
        messages: vec![message.clone()],
    }));
    Ok(())
}

async fn update_local_conversation_projection(
    conversation_store: &Arc<dyn ConversationStore>,
    message: &IMMessage,
) -> Result<()> {
    let conversation_id = message.conversation_id.trim();
    if conversation_id.is_empty() {
        return Ok(());
    }
    let message_id = if message.server_id.trim().is_empty() {
        message.client_msg_id.trim()
    } else {
        message.server_id.trim()
    };
    if message_id.is_empty() {
        return Ok(());
    }
    let preview = message.text_for_storage();
    conversation_store
        .update_last_message(
            conversation_id,
            message_id,
            message.sender_id(),
            message.timeline_sort_ts(),
            preview.as_deref(),
            message.conversation_seq,
        )
        .await
}

fn upload_progress_percent(progress: &UploadProgress) -> u32 {
    if matches!(progress.phase, UploadPhase::Finished) {
        return 100;
    }
    if progress.total_bytes > 0 {
        return ((progress.uploaded_bytes.saturating_mul(100)) / progress.total_bytes).min(100)
            as u32;
    }
    match progress.phase {
        UploadPhase::Preparing => 0,
        UploadPhase::Uploading => 1,
        UploadPhase::Completing => 99,
        UploadPhase::Finished => 100,
    }
}

fn attach_local_media_preview(message: &mut IMMessage) {
    let Some(ref mut content) = message.content else {
        return;
    };
    match content {
        Elem::File(file) => {
            if file.url.trim().is_empty()
                && let Some(locator) = local_preview_locator(&file.file_id)
            {
                file.url = locator;
            }
        }
        Elem::Image(image) => {
            if let Some(source) = image.source.as_mut() {
                attach_image_preview(source);
            }
            if let Some(thumbnail) = image.thumbnail.as_mut() {
                attach_image_preview(thumbnail);
            }
            if image.thumbnail.is_none() {
                image.thumbnail = image.source.clone();
            }
        }
        Elem::ImageGroup(group) => {
            for image in &mut group.images {
                attach_image_preview(image);
            }
        }
        Elem::Video(video) => {
            if video.source.is_none()
                && let Some(locator) = local_preview_locator(&video.video_id)
            {
                video.source = Some(VideoInfoElem {
                    uuid: video.video_id.clone(),
                    url: locator,
                    mime_type: String::new(),
                    size: 0,
                    duration_ms: 0,
                    width: 0,
                    height: 0,
                });
            }
            if let Some(source) = video.source.as_mut() {
                attach_video_preview(source, &video.video_id);
            }
        }
        Elem::Audio(audio) => {
            if audio.source.is_none()
                && let Some(locator) = local_preview_locator(&audio.audio_id)
            {
                audio.source = Some(AudioInfoElem {
                    uuid: audio.audio_id.clone(),
                    url: locator,
                    mime_type: String::new(),
                    size: 0,
                    duration_ms: 0,
                });
            }
            if let Some(source) = audio.source.as_mut() {
                attach_audio_preview(source, &audio.audio_id);
            }
        }
        _ => {}
    }
    message.encoded_content.clear();
}

fn attach_image_preview(info: &mut ImageInfoElem) {
    if !info.url.trim().is_empty() {
        return;
    }
    let locator =
        local_preview_locator(&info.image_id).or_else(|| local_preview_locator(&info.uuid));
    if let Some(locator) = locator {
        info.url = locator;
    }
}

fn attach_video_preview(info: &mut VideoInfoElem, fallback: &str) {
    if !info.url.trim().is_empty() {
        return;
    }
    let locator = local_preview_locator(&info.uuid).or_else(|| local_preview_locator(fallback));
    if let Some(locator) = locator {
        info.url = locator;
    }
}

fn attach_audio_preview(info: &mut AudioInfoElem, fallback: &str) {
    if !info.url.trim().is_empty() {
        return;
    }
    let locator = local_preview_locator(&info.uuid).or_else(|| local_preview_locator(fallback));
    if let Some(locator) = locator {
        info.url = locator;
    }
}

fn local_preview_locator(value: &str) -> Option<String> {
    extract_media_source(value).map(|source| match source.kind {
        MediaSourceKind::Path => source.locator,
        MediaSourceKind::Uri
        | MediaSourceKind::Asset
        | MediaSourceKind::Blob
        | MediaSourceKind::Bytes
        | MediaSourceKind::Custom(_) => source.locator,
    })
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
        url: uploaded_media_display_url(uploaded),
        mime_type: uploaded.mime_type.clone(),
        size: uploaded.size,
        width: 0,
        height: 0,
        format,
        animated,
        blurhash: String::new(),
    })
}

fn uploaded_media_to_video_descriptor(uploaded: &UploadedMedia) -> Option<VideoInfoElem> {
    if uploaded.file_id.trim().is_empty() {
        return None;
    }
    Some(VideoInfoElem {
        uuid: uploaded.file_id.clone(),
        url: uploaded_media_display_url(uploaded),
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
        url: uploaded_media_display_url(uploaded),
        mime_type: uploaded.mime_type.clone(),
        size: uploaded.size,
        duration_ms: 0,
    })
}

fn uploaded_media_display_url(uploaded: &UploadedMedia) -> String {
    uploaded
        .cdn_url
        .as_deref()
        .or(uploaded.url.as_deref())
        .unwrap_or_default()
        .to_string()
}

fn processed_media_from_source(mut source: MediaSourceDescriptor) -> Result<ProcessedMedia> {
    if matches!(source.kind, MediaSourceKind::Bytes) && source.locator.starts_with("data:") {
        let (payload, metadata) =
            decode_data_media_source(&source.locator, source.metadata.clone().unwrap_or_default())?;
        source.metadata = Some(metadata.clone());
        return Ok(ProcessedMedia {
            source,
            metadata,
            payload: Some(payload),
        });
    }

    let metadata = source.metadata.clone().unwrap_or_default();
    Ok(ProcessedMedia {
        source,
        metadata,
        payload: None,
    })
}

fn decode_data_media_source(
    locator: &str,
    fallback: MediaMetadata,
) -> Result<(Vec<u8>, MediaMetadata)> {
    let Some((header, body)) = locator.split_once(',') else {
        return Err(FlareError::localized(
            ErrorCode::InvalidParameter,
            "invalid data media source",
        ));
    };
    let header = header.strip_prefix("data:").unwrap_or(header);
    let mut mime_type = fallback.mime_type;
    let mut file_name = fallback.file_name;
    let mut declared_size = fallback.size;
    let mut base64_encoded = false;

    for part in header.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part.eq_ignore_ascii_case("base64") {
            base64_encoded = true;
            continue;
        }
        if let Some((key, value)) = part.split_once('=') {
            match key {
                "name" => file_name = percent_decode(value),
                "size" => declared_size = value.parse::<u64>().unwrap_or(declared_size),
                _ => {}
            }
            continue;
        }
        if part.contains('/') {
            mime_type = part.to_string();
        }
    }

    if !base64_encoded {
        return Err(FlareError::localized(
            ErrorCode::InvalidParameter,
            "data media source must be base64 encoded",
        ));
    }
    let payload = base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|error| {
            FlareError::localized(
                ErrorCode::InvalidParameter,
                format!("invalid data media payload: {error}"),
            )
        })?;
    if mime_type.trim().is_empty() {
        mime_type = "application/octet-stream".to_string();
    }
    if file_name.trim().is_empty() {
        file_name = default_file_name_for_mime(&mime_type).to_string();
    }
    if declared_size == 0 {
        declared_size = payload.len() as u64;
    }

    Ok((
        payload,
        MediaMetadata {
            file_name,
            mime_type,
            size: declared_size,
            width: fallback.width,
            height: fallback.height,
            duration_ms: fallback.duration_ms,
            extra: fallback.extra,
        },
    ))
}

fn percent_decode(value: &str) -> String {
    let mut out = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = hex_value(bytes[i + 1]);
            let lo = hex_value(bytes[i + 2]);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| value.to_string())
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn default_file_name_for_mime(mime_type: &str) -> &'static str {
    if mime_type.starts_with("image/") {
        "image-upload"
    } else if mime_type.starts_with("video/") {
        "video-upload"
    } else if mime_type.starts_with("audio/") {
        "audio-upload"
    } else {
        "file-upload"
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ImageFormat, MessageSendUseCase, attach_local_media_preview, extract_media_source,
        processed_media_from_source, uploaded_media_to_image_descriptor,
    };
    use async_trait::async_trait;
    use flare_proto::common::ImageInfo;
    use std::sync::Arc;
    use tokio::sync::{Mutex, RwLock, oneshot, watch};

    use crate::content::{ContentBuilder, Elem, MessageBuilder};
    use crate::domain::{ConversationStore, MessageStore};
    use crate::extension::middleware::MiddlewareChain;
    use crate::infrastructure::protocol::{PacketSender, ProtobufCodec};
    use crate::kernel::event::{EventBus, MessageEvent, SdkEvent};
    use crate::kernel::{CurrentUserIdStore, ReliableSendQueuePort};
    use crate::model::message::{IMMessage, MessageStatus, SendAck};
    use crate::model::{
        MediaAccessUrl, MediaCacheEntryVo, MediaResolvedAccess, UploadOptions, UploadedMedia,
    };
    use crate::platform::ports::media::{
        MediaServicePort, MediaSourceKind, ProcessedMedia, UploadProgressSink,
    };
    use crate::shared::error::{ErrorCode, FlareError, Result};
    use crate::storage::{MemoryConversationStore, MemoryMessageStore};

    struct TestHarness {
        usecase: MessageSendUseCase,
        messages: Arc<dyn MessageStore>,
        _conversations: Arc<dyn ConversationStore>,
        queue: Arc<CapturingReliableQueue>,
        media: Arc<ControlledMediaService>,
    }

    impl TestHarness {
        fn new(media_result: MediaResult) -> Self {
            let messages: Arc<dyn MessageStore> = Arc::new(MemoryMessageStore::new());
            let conversations: Arc<dyn ConversationStore> =
                Arc::new(MemoryConversationStore::new());
            let queue = Arc::new(CapturingReliableQueue::default());
            let media = Arc::new(ControlledMediaService::new(media_result));
            let sender = Arc::new(PacketSender::new(
                Arc::new(Mutex::new(None)),
                Arc::new(ProtobufCodec),
            ));
            let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("hugo".to_string()));
            let usecase = MessageSendUseCase::new(
                sender,
                messages.clone(),
                conversations.clone(),
                Arc::new(MiddlewareChain::new()),
                current_user_id,
                Some(queue.clone()),
                media.clone(),
                EventBus::new(),
            );
            Self {
                usecase,
                messages,
                _conversations: conversations,
                queue,
                media,
            }
        }
    }

    #[derive(Clone)]
    enum MediaResult {
        Uploaded(UploadedMedia),
        Failed,
    }

    struct ControlledMediaService {
        result: MediaResult,
        started_tx: watch::Sender<bool>,
        started_rx: watch::Receiver<bool>,
        release_tx: Mutex<Option<oneshot::Sender<()>>>,
        release_rx: Mutex<Option<oneshot::Receiver<()>>>,
    }

    impl ControlledMediaService {
        fn new(result: MediaResult) -> Self {
            let (release_tx, release_rx) = oneshot::channel();
            let (started_tx, started_rx) = watch::channel(false);
            Self {
                result,
                started_tx,
                started_rx,
                release_tx: Mutex::new(Some(release_tx)),
                release_rx: Mutex::new(Some(release_rx)),
            }
        }

        async fn wait_until_upload_started(&self) {
            let mut started_rx = self.started_rx.clone();
            if *started_rx.borrow() {
                return;
            }
            let _ = started_rx.changed().await;
        }

        async fn finish_upload(&self) {
            if let Some(tx) = self.release_tx.lock().await.take() {
                let _ = tx.send(());
            }
        }
    }

    #[async_trait]
    impl MediaServicePort for ControlledMediaService {
        async fn upload(
            &self,
            media: ProcessedMedia,
            _options: Option<UploadOptions>,
            progress: Option<UploadProgressSink>,
        ) -> Result<UploadedMedia> {
            if let Some(progress) = &progress {
                progress(crate::application::UploadProgress {
                    file_name: media.source.locator.clone(),
                    upload_id: "upload-test".to_string(),
                    phase: crate::application::UploadPhase::Uploading,
                    uploaded_bytes: 50,
                    total_bytes: 100,
                    chunk_index: Some(1),
                    total_chunks: Some(2),
                });
            }
            let _ = self.started_tx.send(true);
            if let Some(rx) = self.release_rx.lock().await.take() {
                let _ = rx.await;
            }
            match &self.result {
                MediaResult::Uploaded(uploaded) => {
                    if let Some(progress) = &progress {
                        progress(crate::application::UploadProgress {
                            file_name: uploaded.file_name.clone(),
                            upload_id: "upload-test".to_string(),
                            phase: crate::application::UploadPhase::Finished,
                            uploaded_bytes: 100,
                            total_bytes: 100,
                            chunk_index: Some(2),
                            total_chunks: Some(2),
                        });
                    }
                    Ok(uploaded.clone())
                }
                MediaResult::Failed => Err(FlareError::localized(
                    ErrorCode::GeneralError,
                    "controlled upload failed",
                )),
            }
        }

        async fn delete_file(&self, _file_id: &str, _hard_delete: bool) -> Result<bool> {
            Ok(true)
        }

        async fn get_file_url(&self, file_id: &str, _expires_in: i32) -> Result<MediaAccessUrl> {
            Ok(MediaAccessUrl {
                url: format!("https://media.example/{file_id}"),
                cdn_url: None,
            })
        }

        async fn get_temp_url_for_file_download(
            &self,
            file_id: &str,
            expires_in: i32,
        ) -> Result<MediaAccessUrl> {
            self.get_file_url(file_id, expires_in).await
        }

        async fn resolve_media_access(
            &self,
            file_id: &str,
            expires_in: i32,
        ) -> Result<MediaResolvedAccess> {
            Ok(MediaResolvedAccess {
                source: "remote".to_string(),
                local_path: None,
                remote: Some(self.get_file_url(file_id, expires_in).await?),
            })
        }

        async fn cache_remote_media(
            &self,
            file_id: &str,
            expires_in: i32,
        ) -> Result<MediaCacheEntryVo> {
            Ok(MediaCacheEntryVo {
                file_id: file_id.to_string(),
                local_path: String::new(),
                mime_type: String::new(),
                size_bytes: 0,
                updated_at_ms: expires_in as i64,
            })
        }
    }

    #[derive(Default)]
    struct CapturingReliableQueue {
        enqueued: Mutex<Vec<IMMessage>>,
    }

    impl CapturingReliableQueue {
        async fn enqueued(&self) -> Vec<IMMessage> {
            self.enqueued.lock().await.clone()
        }
    }

    #[async_trait]
    impl ReliableSendQueuePort for CapturingReliableQueue {
        async fn enqueue(&self, message: IMMessage) -> Result<()> {
            self.enqueued.lock().await.push(message);
            Ok(())
        }

        async fn on_ack(&self, _ack: SendAck) -> Result<()> {
            Ok(())
        }

        async fn reset_pending_on_login(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }

        async fn recover_pending_for_current_user(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    fn local_file_message(path: &str) -> IMMessage {
        let message = MessageBuilder::new("conv-media", "hugo")
            .single_chat()
            .channel("peer-1")
            .content(
                ContentBuilder::file(path)
                    .file_name("demo.png")
                    .mime_type("image/png")
                    .file_size(100)
                    .build(),
            )
            .build()
            .expect("message");
        IMMessage::new(message)
    }

    fn local_image_group_message(paths: &[&str]) -> IMMessage {
        let images = paths
            .iter()
            .map(|path| ImageInfo {
                uuid: (*path).to_string(),
                image_id: (*path).to_string(),
                url: String::new(),
                mime_type: "image/png".to_string(),
                size: 100,
                width: 0,
                height: 0,
                format: ImageFormat::Png as i32,
                animated: false,
                blurhash: String::new(),
            })
            .collect();
        let message = MessageBuilder::new("conv-media", "hugo")
            .single_chat()
            .channel("peer-1")
            .content(
                ContentBuilder::image_group_with_details(images, "album", Default::default())
                    .build(),
            )
            .build()
            .expect("message");
        IMMessage::new(message)
    }

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
    fn data_media_source_builds_payload_and_metadata_for_web_upload() {
        let source =
            extract_media_source("data:image/png;name=real%20image.png;size=3;base64,QUJD")
                .expect("data source");

        let media = processed_media_from_source(source).expect("processed media");

        assert_eq!(media.payload.as_deref(), Some(&b"ABC"[..]));
        assert_eq!(media.metadata.file_name, "real image.png");
        assert_eq!(media.metadata.mime_type, "image/png");
        assert_eq!(media.metadata.size, 3);
    }

    #[test]
    fn uploaded_image_descriptor_keeps_stable_metadata_and_display_url() {
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
        assert_eq!(source.url, "https://cdn.example.com/demo.png");
        assert_eq!(source.mime_type, "image/png");
        assert_eq!(source.size, 123);
        assert_eq!(source.format, ImageFormat::Png as i32);
        assert!(!source.animated);
    }

    #[test]
    fn uploaded_media_display_url_falls_back_to_origin_url() {
        let uploaded = UploadedMedia {
            file_id: "img-1".to_string(),
            file_name: "demo.png".to_string(),
            mime_type: "image/png".to_string(),
            size: 123,
            url: Some("https://origin.example.com/demo.png".to_string()),
            cdn_url: None,
        };

        let source = uploaded_media_to_image_descriptor(&uploaded).expect("image descriptor");

        assert_eq!(source.url, "https://origin.example.com/demo.png");
    }

    #[test]
    fn attach_local_media_preview_keeps_optimistic_sources_renderable() {
        let mut file = local_file_message("/tmp/demo.png");
        attach_local_media_preview(&mut file);
        let Elem::File(file_content) = file.content.as_ref().expect("file content") else {
            panic!("expected file content")
        };
        assert_eq!(file_content.file_id, "/tmp/demo.png");
        assert_eq!(file_content.url, "/tmp/demo.png");

        let mut group = local_image_group_message(&["/tmp/one.png", "/tmp/two.png"]);
        attach_local_media_preview(&mut group);
        let Elem::ImageGroup(group_content) = group.content.as_ref().expect("image group") else {
            panic!("expected image group")
        };
        assert_eq!(group_content.images[0].image_id, "/tmp/one.png");
        assert_eq!(group_content.images[0].url, "/tmp/one.png");
        assert_eq!(group_content.images[1].url, "/tmp/two.png");
    }

    #[tokio::test]
    async fn send_with_media_persists_uploading_message_before_upload_finishes() {
        let uploaded = UploadedMedia {
            file_id: "remote-file-1".to_string(),
            file_name: "remote-demo.png".to_string(),
            mime_type: "image/png".to_string(),
            size: 100,
            url: Some("https://origin.example/demo.png".to_string()),
            cdn_url: Some("https://cdn.example/demo.png".to_string()),
        };
        let harness = TestHarness::new(MediaResult::Uploaded(uploaded));
        let message = local_file_message("/tmp/demo.png");
        let client_msg_id = message.client_msg_id.clone();
        let mut events = harness.usecase.bus.subscribe();

        let send_task = harness.usecase.send_with_media(message, None);
        tokio::pin!(send_task);
        tokio::select! {
            _ = harness.media.wait_until_upload_started() => {}
            result = &mut send_task => panic!("send finished before upload could be inspected: {result:?}"),
        }

        let stored = harness
            .messages
            .get_by_client_msg_id(&client_msg_id)
            .await
            .expect("store read")
            .expect("optimistic uploading message");
        assert_eq!(stored.client_msg_id, client_msg_id);
        assert_eq!(stored.status, MessageStatus::Created as i32);
        assert!(stored.local_state.is_local);
        assert!(stored.local_state.sending);
        assert!(stored.local_state.uploading);
        assert!(stored.local_state.upload_progress <= 99);
        let Elem::File(file) = stored.content.as_ref().expect("file content") else {
            panic!("expected file content")
        };
        assert_eq!(file.file_id, "/tmp/demo.png");
        assert_eq!(file.url, "/tmp/demo.png");

        harness.media.finish_upload().await;
        let ack = send_task.await.expect("send");
        assert_eq!(ack.client_msg_id, client_msg_id);

        let queued = harness.queue.enqueued().await;
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].client_msg_id, client_msg_id);
        let Elem::File(file) = queued[0].content.as_ref().expect("file content") else {
            panic!("expected queued file content")
        };
        assert_eq!(file.file_id, "remote-file-1");
        assert_eq!(file.file_name, "remote-demo.png");
        assert_eq!(file.url, "https://cdn.example/demo.png");

        let final_local = harness
            .messages
            .get_by_client_msg_id(&client_msg_id)
            .await
            .expect("store read")
            .expect("final local message");
        assert!(!final_local.local_state.failed);
        assert!(!final_local.local_state.uploading);
        assert_eq!(final_local.local_state.upload_progress, 100);

        let mut received_updates = 0;
        while let Ok(event) = events.try_recv() {
            if let SdkEvent::Message(MessageEvent::ReceivedBatch { messages }) = event
                && messages
                    .iter()
                    .any(|message| message.client_msg_id == client_msg_id)
            {
                received_updates += 1;
            }
        }
        assert!(
            received_updates >= 2,
            "uploading and uploaded states should be emitted as timeline deltas"
        );
    }

    #[tokio::test]
    async fn send_with_media_marks_same_optimistic_message_failed_when_upload_fails() {
        let harness = TestHarness::new(MediaResult::Failed);
        let message = local_file_message("/tmp/fail.png");
        let client_msg_id = message.client_msg_id.clone();

        let send_task = harness.usecase.send_with_media(message, None);
        tokio::pin!(send_task);
        tokio::select! {
            _ = harness.media.wait_until_upload_started() => {}
            result = &mut send_task => panic!("send finished before upload could be inspected: {result:?}"),
        }
        harness.media.finish_upload().await;

        let err = send_task.await.expect_err("upload should fail");
        assert!(err.to_string().contains("controlled upload failed"));
        assert!(harness.queue.enqueued().await.is_empty());

        let stored = harness
            .messages
            .get_by_client_msg_id(&client_msg_id)
            .await
            .expect("store read")
            .expect("failed optimistic message");
        assert_eq!(stored.client_msg_id, client_msg_id);
        assert_eq!(stored.status, MessageStatus::Failed as i32);
        assert!(stored.local_state.failed);
        assert!(!stored.local_state.sending);
        assert!(!stored.local_state.uploading);
        let Elem::File(file) = stored.content.as_ref().expect("file content") else {
            panic!("expected file content")
        };
        assert_eq!(file.file_id, "/tmp/fail.png");
        assert_eq!(file.url, "/tmp/fail.png");
    }
}
