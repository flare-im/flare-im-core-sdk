//! 媒体上传服务
//!
//! 负责媒体文件的上传、进度回调和元数据管理

use super::StorageBackend;
use crate::application::handlers::MessageCommandHandler;
use crate::domain::MessageBuilder;
use anyhow::{Context, Result};
use flare_proto::{AudioInfo, ImageInfo, VideoInfo};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// 媒体类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Image,
    Video,
    Audio,
    File,
}

/// 上传进度回调
pub struct UploadProgress {
    pub bytes_uploaded: u64,
    pub total_bytes: u64,
    pub percentage: f64,
}

impl UploadProgress {
    pub fn new(bytes_uploaded: u64, total_bytes: u64) -> Self {
        Self {
            bytes_uploaded,
            total_bytes,
            percentage: if total_bytes > 0 {
                (bytes_uploaded as f64 / total_bytes as f64) * 100.0
            } else {
                0.0
            },
        }
    }
}

/// 媒体上传选项
#[derive(Clone)]
pub struct MediaUploadOptions {
    /// 上传进度回调
    pub on_progress: Option<Arc<dyn Fn(UploadProgress) + Send + Sync>>,
    /// 是否压缩（图片/视频）
    pub compress: bool,
    /// 压缩质量（0-100，仅对图片有效）
    pub compress_quality: Option<u8>,
    /// 是否生成缩略图（图片/视频）
    pub generate_thumbnail: bool,
    /// 是否提取封面图（视频）
    pub extract_cover: bool,
    /// 重试配置（TODO: 迁移到新的重试机制）
    pub retry_config: Option<()>, // TODO: 定义新的 RetryConfig
}

impl Default for MediaUploadOptions {
    fn default() -> Self {
        Self {
            on_progress: None,
            compress: false,
            compress_quality: Some(80),
            generate_thumbnail: true,
            extract_cover: true,
            retry_config: None,
        }
    }
}

/// 媒体上传结果
pub struct MediaUploadResult {
    /// 媒体ID（服务端返回的唯一标识）
    pub media_id: String,
    /// 媒体URL（CDN地址）
    pub url: String,
    /// 媒体信息（图片/视频/音频的元数据）
    pub info: MediaInfo,
    /// 缩略图URL（如果有）
    pub thumbnail_url: Option<String>,
    /// 封面图URL（视频，如果有）
    pub cover_url: Option<String>,
}

/// 媒体信息（根据类型不同）
pub enum MediaInfo {
    Image(ImageInfo),
    Video(VideoInfo),
    Audio(AudioInfo),
    File {
        file_name: String,
        file_size: i64,
        mime_type: String,
    },
}

/// 媒体上传服务
///
/// 职责：
/// - 媒体文件上传（图片、视频、音频、文件）
/// - 上传进度回调
/// - 上传失败重试
/// - 媒体元数据管理
pub struct MediaUploadService {
    /// 消息命令处理器（用于发送已上传的媒体消息）
    message_command_handler: Arc<MessageCommandHandler>,

    /// 存储后端（用于保存媒体元数据）
    storage: Arc<dyn StorageBackend>,

    /// 当前用户ID
    user_id: Arc<RwLock<String>>,
}

impl MediaUploadService {
    /// 创建新的媒体上传服务实例
    pub fn new(
        message_command_handler: Arc<MessageCommandHandler>,
        storage: Arc<dyn StorageBackend>,
        user_id: Arc<RwLock<String>>,
    ) -> Self {
        Self {
            message_command_handler,
            storage,
            user_id,
        }
    }

    /// 上传媒体并发送消息（SDK 自动上传，推荐方式）
    ///
    /// # 参数
    /// - `builder`: MessageBuilder（已设置 session_id, receiver_id 等）
    /// - `media_path`: 媒体文件路径
    /// - `options`: 上传选项
    ///
    /// # 返回
    /// - `Result<String>`: 消息ID
    ///
    /// # 示例
    /// ```rust,no_run
    /// let message_id = media_upload_service
    ///     .send_media_with_upload(
    ///         MessageBuilder::new(session_id, &user_id)
    ///             .session_type(SessionType::Single)
    ///             .receiver_id(receiver_id),
    ///         Path::new("/path/to/image.jpg"),
    ///         MediaUploadOptions {
    ///             on_progress: Some(Arc::new(|progress| {
    ///                 println!("Upload progress: {}%", progress.percentage);
    ///             })),
    ///             compress: true,
    ///             ..Default::default()
    ///         },
    ///     )
    ///     .await?;
    /// ```
    pub async fn send_media_with_upload(
        &self,
        builder: MessageBuilder,
        media_path: &Path,
        options: MediaUploadOptions,
    ) -> Result<String> {
        // 1. 检测媒体类型
        let media_type = Self::detect_media_type(media_path)?;

        // 2. 上传媒体文件
        let upload_result = self
            .upload_media(media_path, media_type, options.clone())
            .await?;

        // 3. 使用 MessageBuilder 构建消息并发送
        let user_id = {
            let guard = self.user_id.read().await;
            guard.clone()
        };

        let message = match upload_result.info {
            MediaInfo::Image(image_info) => {
                // 构建 ImageContent
                let image_content = flare_proto::ImageContent {
                    image_id: upload_result.media_id.clone(),
                    source: Some(image_info),
                    thumbnail: upload_result
                        .thumbnail_url
                        .map(|url| flare_proto::ImageInfo {
                            uuid: String::new(),
                            url,
                            mime_type: String::new(),
                            size: 0,
                            width: 0,
                            height: 0,
                        }),
                    description: String::new(),
                };
                builder.image(image_content).build()
            }
            MediaInfo::Video(video_info) => {
                // 构建 VideoContent
                let video_content = flare_proto::VideoContent {
                    video_id: upload_result.media_id.clone(),
                    source: Some(video_info),
                    cover: upload_result.cover_url.map(|url| flare_proto::ImageInfo {
                        uuid: String::new(),
                        url,
                        mime_type: String::new(),
                        size: 0,
                        width: 0,
                        height: 0,
                    }),
                    description: String::new(),
                };
                builder.video(video_content).build()
            }
            MediaInfo::Audio(audio_info) => {
                // 构建 AudioContent
                let audio_content = flare_proto::AudioContent {
                    audio_id: upload_result.media_id.clone(),
                    source: Some(audio_info),
                    description: String::new(),
                };
                builder.audio(audio_content).build()
            }
            MediaInfo::File {
                file_name,
                file_size,
                mime_type,
            } => {
                // 构建 FileContent
                let file_content = flare_proto::FileContent {
                    file_id: upload_result.media_id.clone(),
                    file_name,
                    mime_type,
                    file_size,
                    url: media_path.to_string_lossy().to_string(),
                    description: String::new(),
                };
                builder.file(file_content).build()
            }
        };

        // 4. 发送消息（通过 MessageCommandHandler）
        use crate::application::commands::message::SendMessageCommand;
        use crate::domain::{MessageId, MessageType, SessionId, UserId};

        let user_id = {
            let guard = self.user_id.read().await;
            UserId::new(guard.clone())
        };

        let session_id = SessionId::new(message.session_id.clone());
        let content = message
            .content
            .ok_or_else(|| anyhow::anyhow!("Message content is required"))?;
        let message_type = MessageType::try_from(message.message_type)
            .map_err(|_| anyhow::anyhow!("Invalid message type"))?;

        let cmd = SendMessageCommand {
            session_id,
            sender_id: user_id,
            receiver_id: None,
            channel_id: None,
            content,
            message_type,
            seq: None, // 序列号由服务端分配
        };

        self.message_command_handler
            .handle_send_message(cmd)
            .await
            .map(|id| id.to_string())
    }

    /// 发送已上传的媒体消息（使用者已完成上传）
    ///
    /// # 参数
    /// - `message`: 完整的 Message 对象（MessageBuilder 已设置 media_url）
    ///
    /// # 返回
    /// - `Result<String>`: 消息ID
    ///
    /// # 示例
    /// ```rust,no_run
    /// let message = MessageBuilder::new(session_id, &user_id)
    ///     .image(
    ///         "image_id_123",
    ///         ImageInfo {
    ///             url: "https://cdn.example.com/image.jpg".to_string(),
    ///             width: 1920,
    ///             height: 1080,
    ///             ..Default::default()
    ///         },
    ///         None, // thumbnail
    ///         None, // description
    ///     )
    ///     .session_type(SessionType::Single)
    ///     .receiver_id(receiver_id)
    ///     .build_message();
    ///
    /// let message_id = media_upload_service.send_media(message).await?;
    /// ```
    pub async fn send_media(&self, message: flare_proto::Message) -> Result<String> {
        // 通过 MessageCommandHandler 发送消息
        use crate::application::commands::message::SendMessageCommand;
        use crate::domain::{MessageId, MessageType, SessionId, UserId};

        let user_id = {
            let guard = self.user_id.read().await;
            UserId::new(guard.clone())
        };

        let session_id = SessionId::new(message.session_id.clone());
        let content = message
            .content
            .ok_or_else(|| anyhow::anyhow!("Message content is required"))?;
        let message_type = MessageType::try_from(message.message_type)
            .map_err(|_| anyhow::anyhow!("Invalid message type"))?;

        let cmd = SendMessageCommand {
            session_id,
            sender_id: user_id,
            receiver_id: None,
            channel_id: None,
            content,
            message_type,
            seq: None, // 序列号由服务端分配
        };

        self.message_command_handler
            .handle_send_message(cmd)
            .await
            .map(|id| id.to_string())
    }

    /// 上传媒体文件（独立上传，不发送消息）
    ///
    /// # 参数
    /// - `media_path`: 媒体文件路径
    /// - `media_type`: 媒体类型
    /// - `options`: 上传选项
    ///
    /// # 返回
    /// - `Result<MediaUploadResult>`: 上传结果
    pub async fn upload_media(
        &self,
        media_path: &Path,
        media_type: MediaType,
        options: MediaUploadOptions,
    ) -> Result<MediaUploadResult> {
        // TODO: 实现实际上传逻辑
        // 这里需要：
        // 1. 读取文件
        // 2. 可选：压缩/处理
        // 3. 上传到 CDN/对象存储
        // 4. 获取 media_id 和 url
        // 5. 可选：生成缩略图/封面图
        // 6. 调用进度回调

        info!(
            media_path = %media_path.display(),
            media_type = ?media_type,
            "Uploading media file"
        );

        // 临时实现：返回模拟结果
        // 实际实现需要调用上传服务（如 MinIO/S3）
        warn!("Media upload not yet implemented, returning mock result");

        let file_name = media_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let media_id = format!("media_{}", uuid::Uuid::new_v4());
        let url = format!("https://cdn.example.com/{}", file_name);

        let info = match media_type {
            MediaType::Image => MediaInfo::Image(ImageInfo {
                uuid: String::new(),
                url: url.clone(),
                mime_type: "image/jpeg".to_string(),
                size: 0, // TODO: 获取实际文件大小
                width: 1920,
                height: 1080,
            }),
            MediaType::Video => MediaInfo::Video(VideoInfo {
                uuid: String::new(),
                url: url.clone(),
                mime_type: "video/mp4".to_string(),
                size: 0,
                duration_ms: 0,
                width: 1920,
                height: 1080,
            }),
            MediaType::Audio => MediaInfo::Audio(AudioInfo {
                uuid: String::new(),
                url: url.clone(),
                mime_type: "audio/mpeg".to_string(),
                size: 0,
                duration_ms: 0,
            }),
            MediaType::File => MediaInfo::File {
                file_name: file_name.clone(),
                file_size: 0,
                mime_type: "application/octet-stream".to_string(),
            },
        };

        Ok(MediaUploadResult {
            media_id,
            url,
            info,
            thumbnail_url: if options.generate_thumbnail {
                Some(format!("https://cdn.example.com/thumb_{}", file_name))
            } else {
                None
            },
            cover_url: if options.extract_cover && media_type == MediaType::Video {
                Some(format!("https://cdn.example.com/cover_{}", file_name))
            } else {
                None
            },
        })
    }

    /// 检测媒体类型（根据文件扩展名）
    fn detect_media_type(path: &Path) -> Result<MediaType> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" => Ok(MediaType::Image),
            "mp4" | "avi" | "mov" | "wmv" | "flv" | "webm" | "mkv" => Ok(MediaType::Video),
            "mp3" | "wav" | "aac" | "ogg" | "flac" | "m4a" => Ok(MediaType::Audio),
            _ => Ok(MediaType::File),
        }
    }
}
