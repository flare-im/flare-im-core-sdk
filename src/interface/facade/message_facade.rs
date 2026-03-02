//! Message Facade
//!
//! Provides high-level APIs for message-related operations including:
//!
//! - Creating messages (text, image, video, audio, file, etc.)
//! - Sending messages
//! - Message operations (edit, delete, recall, react, pin, mark)
//! - Querying messages
//!
//! ## Example
//!
//! ```no_run
//! use flare_im_core_sdk::interface::facade::MessageFacade;
//! // 创建消息不再需要租户上下文
//!
//! # async fn example(facade: &MessageFacade) -> anyhow::Result<()> {
//! // Create a text message
//! let message = facade.create_text_message(
//!     "user1".to_string(),
//!     "Hello".to_string(),
//!     Some("user2".to_string()),
//! )?;
//!
//! // Send the message
//! facade.send_message(message).await?;
//! # Ok(())
//! # }
//! ```

use std::sync::Arc;
use crate::application::handlers::{CommandHandler, QueryHandler};
use crate::domain::message::*;
use crate::domain::service::{
    MessageDomainService, 
    MediaDomainService,
    MentionInfo, 
};
use crate::infrastructure::storage::media_cache::MediaCacheManager;
use crate::infrastructure::converter::ConverterRegistry;
use anyhow::Result;

/// Message facade providing high-level message APIs
///
/// Interface 层返回领域模型（Message），提供类型安全和良好的 Rust 原生体验
/// FFI 层负责将领域模型转换为 JSON 字符串
pub struct MessageFacade {
    fsm: Arc<crate::application::fsm::FsmManager>,
    command_handler: Arc<CommandHandler>,
    query_handler: Arc<QueryHandler>,
    message_repository: Arc<dyn crate::domain::repository::MessageRepository>,
    conversation_repository: Arc<dyn crate::domain::repository::ConversationRepository>,
    domain_service: MessageDomainService,
    media_service: MediaDomainService,
    media_cache: Arc<MediaCacheManager>,
    #[allow(dead_code)]
    converter: Arc<ConverterRegistry>,
}

impl MessageFacade {
    /// Creates a new message facade
    ///
    /// # Arguments
    ///
    /// * `fsm` - FSM manager for getting current user ID
    /// * `command_handler` - Command handler for message operations
    /// * `query_handler` - Query handler for message queries
    /// * `message_repository` - Message repository for querying messages
    /// * `conversation_repository` - Conversation repository for querying conversations
    /// * `media_cache` - Media cache manager for media files
    /// * `converter` - Converter registry for JSON ↔ Domain Model conversion
    pub fn new(
        fsm: Arc<crate::application::fsm::FsmManager>,
        command_handler: Arc<CommandHandler>,
        query_handler: Arc<QueryHandler>,
        message_repository: Arc<dyn crate::domain::repository::MessageRepository>,
        conversation_repository: Arc<dyn crate::domain::repository::ConversationRepository>,
        media_cache: Arc<MediaCacheManager>,
        converter: Arc<ConverterRegistry>,
    ) -> Self {
        Self {
            fsm,
            command_handler,
            query_handler,
            message_repository,
            conversation_repository,
            domain_service: MessageDomainService::new(),
            media_service: MediaDomainService::new(),
            media_cache,
            converter,
        }
    }
    
    
    /// Creates a text message
    ///
    /// # Arguments
    ///
    /// * `text` - The message text content
    /// * `receiver_id` - The receiver ID (required for single chat, optional for group chat)
    ///
    /// # Returns
    ///
    /// Returns a [`Message`] instance on success.
    ///
    /// # Errors
    ///
    /// Returns an error if message creation fails or user is not logged in.
    ///
    /// # Note
    ///
    /// - The `sender_id` is automatically obtained from FSM (current logged-in user)
    /// - The `conversation_id` will be set when sending the message
    /// - For single chat messages, `receiver_id` is required and will be validated
    ///   by the Message Orchestrator when sending
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::MessageFacade;
    /// # async fn example(facade: &MessageFacade) -> anyhow::Result<()> {
    /// let message = facade.create_text_message(
    ///     "Hello, World!".to_string(),
    ///     Some("user2".to_string()),
    /// ).await?;
    /// 
    /// // Send to a conversation
    /// facade.send_message(message, Some("conv1".to_string())).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create_text_message(
        &self,
        text: String,
        receiver_id: Option<String>,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let user_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 创建领域模型并直接返回（conversation_id 为 None）
        use crate::domain::message::build_text_message;
        build_text_message(None, user_id, text, receiver_id)
    }
    
    /// Creates a text message with mentions
    ///
    /// # Arguments
    ///
    /// * `text` - The message text content
    /// * `mentions` - List of mention information
    ///
    /// # Returns
    ///
    /// Returns a [`Message`] instance with mentions on success.
    ///
    /// # Errors
    ///
    /// Returns an error if message creation fails or user is not logged in.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::MessageFacade;
    /// # use flare_im_core_sdk::domain::service::MentionInfo;
    /// # async fn example(facade: &MessageFacade) -> anyhow::Result<()> {
    /// let mentions = vec![MentionInfo {
    ///     user_id: "user2".to_string(),
    ///     start: 0,
    ///     length: 5,
    /// }];
    /// let message = facade.create_text_at_message(
    ///     "@user2 Hello".to_string(),
    ///     mentions,
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create_text_at_message(
        &self,
        text: String,
        mentions: Vec<MentionInfo>,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let user_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 创建领域模型并直接返回
        self.domain_service.create_text_at_message(None, user_id, text, mentions)
    }
    
    /// Creates an image message from a file path
    ///
    /// Reads the image file, validates it, caches it locally, and creates
    /// a message with the image attachment.
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - The conversation ID
    /// * `sender_id` - The sender ID
    /// * `file_path` - The absolute path to the image file
    ///
    /// # Returns
    ///
    /// Returns a [`Message`] instance with image attachment on success.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - File reading fails
    /// - File validation fails
    /// - Media caching fails
    /// - Message creation fails
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::MessageFacade;
    /// # async fn example(facade: &MessageFacade) -> anyhow::Result<()> {
    /// // 不再需要租户上下文
    /// let message = facade.create_image_message_from_full_path(
    ///     "/path/to/image.jpg".to_string(),
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create_image_message_from_full_path(
        &self,
        file_path: String,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 读取文件数据
        let file_data = tokio::fs::read(&file_path).await?;
        let file_size = file_data.len() as u64;
        
        // 获取文件 MIME 类型（通过媒体领域服务）
        let mime_type = self.media_service.detect_mime_type(&file_path)?;
        
        // 使用媒体领域服务准备上传上下文
        let upload_context = self.media_service.prepare_media_upload_context(
            &file_path,
            file_size,
            &mime_type,
            &sender_id,
        )?;
        
        // 验证媒体文件
        self.media_service.validate_media_file(
            file_size,
            &mime_type,
            upload_context.file_type,
        )?;
        
        // 保存到本地缓存
        use crate::domain::message::MediaAttachment;
        let mut attachment = MediaAttachment {
            attachment_id: upload_context.file_id.clone(),
            attachment_type: "image".to_string(),
            url: String::new(), // 暂时为空，上传后更新
            size: file_size,
            mime_type: mime_type.clone(),
            metadata: upload_context.metadata.clone(),
        };
        
        // 保存到本地缓存
        let local_path = self.media_cache.save_media(&attachment, file_data).await?;
        
        // 更新附件元数据（添加本地路径）
        attachment.metadata.insert("local_path".to_string(), local_path.to_string_lossy().to_string());
        
        // 生成上传URL（占位实现，基础层实现暂时留出来）
        let image_url = self.media_service.generate_upload_url(&upload_context)?;
        
        // 调用领域服务构建图片消息
        use crate::domain::message::build_image_message;
        let mut message = build_image_message(
            None,  // conversation_id 在发送时设置
            sender_id,
            image_url,
            Some(local_path.to_string_lossy().to_string()),
        )?;
        
        // 添加附件信息
        message.attachments.push(attachment);
        
        Ok(message)
    }
    
    /// Creates an image message from a URL
    ///
    /// Use this method when you have already uploaded the image and have the URL.
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - The conversation ID
    /// * `sender_id` - The sender ID
    /// * `image_url` - The URL of the uploaded image
    ///
    /// # Returns
    ///
    /// Returns a [`Message`] instance with image URL on success.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::MessageFacade;
    /// # fn example(facade: &MessageFacade) -> anyhow::Result<()> {
    /// // 不再需要租户上下文
    /// let message = facade.create_image_message_by_url(
    ///     "https://example.com/image.jpg".to_string(),
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create_image_message_by_url(
        &self,
        image_url: String,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        use crate::domain::message::build_image_message;
        build_image_message(
            None,  // conversation_id 在发送时设置
            sender_id,
            image_url,
            None, // 没有本地路径
        )
    }
    
    /// 根据文件对象创建图片消息（Web）
    #[cfg(target_arch = "wasm32")]
    pub async fn create_image_message_by_file(
        &self,
        file: web_sys::File,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 薄层：Web 平台文件处理，然后调用领域服务
        use wasm_bindgen_futures::JsFuture;
        use web_sys::FileReader;
        use uuid::Uuid;
        use crate::domain::message::MediaAttachment;
        
        let reader = FileReader::new()?;
        let promise = reader.read_as_array_buffer(&file)?;
        let array_buffer = JsFuture::from(promise).await?;
        let uint8_array = js_sys::Uint8Array::new(&array_buffer);
        let data: Vec<u8> = uint8_array.to_vec();
        
        // 获取文件名和 MIME 类型
        let file_name = file.name();
        let mime_type = file.type_();
        
        // 保存到本地缓存
        let attachment = MediaAttachment {
            attachment_id: Uuid::new_v4().to_string(),
            attachment_type: "image".to_string(),
            url: String::new(),
            size: data.len() as u64,
            mime_type: mime_type.clone(),
            metadata: {
                let mut m = std::collections::HashMap::new();
                m.insert("file_name".to_string(), file_name);
                m
            },
        };
        
        let local_path = self.media_cache.save_media(&attachment, data).await?;
        
        // 生成 URL（占位）
        let image_url = format!("https://example.com/images/{}", attachment.attachment_id);
        
        // 调用领域服务构建图片消息
        use crate::domain::message::build_image_message;
        build_image_message(
            None,  // conversation_id 在发送时设置
            sender_id,
            image_url,
            Some(local_path.to_string_lossy().to_string()),
        )
    }
    
    /// 根据文件绝对路径创建语音消息
    pub async fn create_sound_message_from_full_path(
        &self,
        file_path: String,
        duration_ms: u64,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 读取文件数据
        let file_data = tokio::fs::read(&file_path).await?;
        let file_size = file_data.len() as u64;
        
        // 获取文件 MIME 类型（通过媒体领域服务）
        let mime_type = self.media_service.detect_mime_type(&file_path)?;
        
        // 使用媒体领域服务准备上传上下文
        let upload_context = self.media_service.prepare_media_upload_context(
            &file_path,
            file_size,
            &mime_type,
            &sender_id,
        )?;
        
        // 验证媒体文件
        self.media_service.validate_media_file(
            file_size,
            &mime_type,
            upload_context.file_type,
        )?;
        
        // 保存到本地缓存
        use crate::domain::message::MediaAttachment;
        let mut attachment = MediaAttachment {
            attachment_id: upload_context.file_id.clone(),
            attachment_type: "audio".to_string(),
            url: String::new(), // 暂时为空，上传后更新
            size: file_size,
            mime_type: mime_type.clone(),
            metadata: upload_context.metadata.clone(),
        };
        attachment.metadata.insert("duration_ms".to_string(), duration_ms.to_string());
        
        // 保存到本地缓存
        let local_path = self.media_cache.save_media(&attachment, file_data).await?;
        
        // 更新附件元数据（添加本地路径）
        attachment.metadata.insert("local_path".to_string(), local_path.to_string_lossy().to_string());
        
        // 生成上传URL（占位实现，基础层实现暂时留出来）
        let audio_url = self.media_service.generate_upload_url(&upload_context)?;
        
        // 更新附件URL
        attachment.url = audio_url.clone();
        
        // 调用领域服务构建语音消息
        use crate::domain::message::build_audio_message;
        let mut message = build_audio_message(
            None,  // conversation_id 在发送时设置
            sender_id,
            audio_url,
            Some(local_path.to_string_lossy().to_string()),
            duration_ms,
        )?;
        
        // 添加附件信息
        message.attachments.push(attachment);
        
        Ok(message)
    }
    
    /// 自行上传文件并创建语音消息
    pub async fn create_sound_message_by_url(
        &self,
        audio_url: String,
        duration_ms: u64,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        use crate::domain::message::build_audio_message;
        build_audio_message(
            None,  // conversation_id 在发送时设置
            sender_id,
            audio_url,
            None,
            duration_ms,
        )
    }
    
    /// 根据文件对象创建语音消息（Web）
    #[cfg(target_arch = "wasm32")]
    pub async fn create_sound_message_by_file(
        &self,
        file: web_sys::File,
        duration_ms: u64,
    ) -> Result<Message> {
        // 类似 create_image_message_by_file 的实现
        // TODO: 实现 Web 平台的文件读取
        Err(anyhow::anyhow!("Not implemented for Web platform"))
    }
    
    /// 根据文件绝对路径创建视频消息
    pub async fn create_video_message_from_full_path(
        &self,
        file_path: String,
        duration_ms: u64,
        width: i32,
        height: i32,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 读取文件数据
        let file_data = tokio::fs::read(&file_path).await?;
        let file_size = file_data.len() as u64;
        
        // 获取文件 MIME 类型（通过媒体领域服务）
        let mime_type = self.media_service.detect_mime_type(&file_path)?;
        
        // 使用媒体领域服务准备上传上下文
        let upload_context = self.media_service.prepare_media_upload_context(
            &file_path,
            file_size,
            &mime_type,
            &sender_id,
        )?;
        
        // 验证媒体文件
        self.media_service.validate_media_file(
            file_size,
            &mime_type,
            upload_context.file_type,
        )?;
        
        // 保存到本地缓存
        use crate::domain::message::MediaAttachment;
        let mut attachment = MediaAttachment {
            attachment_id: upload_context.file_id.clone(),
            attachment_type: "video".to_string(),
            url: String::new(), // 暂时为空，上传后更新
            size: file_size,
            mime_type: mime_type.clone(),
            metadata: upload_context.metadata.clone(),
        };
        attachment.metadata.insert("duration_ms".to_string(), duration_ms.to_string());
        attachment.metadata.insert("width".to_string(), width.to_string());
        attachment.metadata.insert("height".to_string(), height.to_string());
        
        // 保存到本地缓存
        let local_path = self.media_cache.save_media(&attachment, file_data).await?;
        
        // 更新附件元数据（添加本地路径）
        attachment.metadata.insert("local_path".to_string(), local_path.to_string_lossy().to_string());
        
        // 生成上传URL（占位实现，基础层实现暂时留出来）
        let video_url = self.media_service.generate_upload_url(&upload_context)?;
        
        // 更新附件URL
        attachment.url = video_url.clone();
        
        // 调用领域服务构建视频消息
        use crate::domain::message::build_video_message;
        let mut message = build_video_message(
            None,  // conversation_id 在发送时设置
            sender_id,
            video_url,
            Some(local_path.to_string_lossy().to_string()),
            duration_ms,
            width,
            height,
        )?;
        
        // 添加附件信息
        message.attachments.push(attachment);
        
        Ok(message)
    }
    
    /// 自行上传文件并创建视频消息
    pub async fn create_video_message_by_url(
        &self,
        video_url: String,
        duration_ms: u64,
        width: i32,
        height: i32,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        use crate::domain::message::build_video_message;
        build_video_message(
            None,  // conversation_id 在发送时设置
            sender_id,
            video_url,
            None,
            duration_ms,
            width,
            height,
        )
    }
    
    /// 根据文件对象创建视频消息（Web）
    #[cfg(target_arch = "wasm32")]
    pub async fn create_video_message_by_file(
        &self,
        file: web_sys::File,
        duration_ms: u64,
        width: i32,
        height: i32,
    ) -> Result<Message> {
        // TODO: 实现 Web 平台的文件读取
        Err(anyhow::anyhow!("Not implemented for Web platform"))
    }
    
    /// 根据文件绝对路径创建文件消息
    pub async fn create_file_message_from_full_path(
        &self,
        file_path: String,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 读取文件数据
        let file_data = tokio::fs::read(&file_path).await?;
        let file_size = file_data.len() as u64;
        
        // 获取文件 MIME 类型（通过媒体领域服务）
        let mime_type = self.media_service.detect_mime_type(&file_path)?;
        
        // 使用媒体领域服务准备上传上下文
        let upload_context = self.media_service.prepare_media_upload_context(
            &file_path,
            file_size,
            &mime_type,
            &sender_id,
        )?;
        
        // 验证媒体文件
        self.media_service.validate_media_file(
            file_size,
            &mime_type,
            upload_context.file_type,
        )?;
        
        // 保存到本地缓存
        use crate::domain::message::MediaAttachment;
        let mut attachment = MediaAttachment {
            attachment_id: upload_context.file_id.clone(),
            attachment_type: "file".to_string(),
            url: String::new(), // 暂时为空，上传后更新
            size: file_size,
            mime_type: mime_type.clone(),
            metadata: upload_context.metadata.clone(),
        };
        
        // 保存到本地缓存
        let local_path = self.media_cache.save_media(&attachment, file_data).await?;
        
        // 更新附件元数据（添加本地路径）
        attachment.metadata.insert("local_path".to_string(), local_path.to_string_lossy().to_string());
        
        // 获取文件名
        let file_name = std::path::Path::new(&file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        
        // 生成上传URL（占位实现，基础层实现暂时留出来）
        let file_url = self.media_service.generate_upload_url(&upload_context)?;
        
        // 更新附件URL
        attachment.url = file_url.clone();
        
        // 调用领域服务构建文件消息
        use crate::domain::message::build_file_message;
        let mut message = build_file_message(
            None,  // conversation_id 在发送时设置
            sender_id,
            file_url,
            file_name,
            file_size,
            mime_type,
            Some(local_path.to_string_lossy().to_string()),
        )?;
        
        // 添加附件信息
        message.attachments.push(attachment);
        
        Ok(message)
    }
    
    /// 自行上传文件并创建文件消息
    pub async fn create_file_message_by_url(
        &self,
        file_url: String,
        file_name: String,
        file_size: u64,
        mime_type: String,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        use crate::domain::message::build_file_message;
        build_file_message(
            None,  // conversation_id 在发送时设置
            sender_id,
            file_url,
            file_name,
            file_size,
            mime_type,
            None,
        )
    }
    
    /// 根据文件对象创建文件消息（Web）
    #[cfg(target_arch = "wasm32")]
    pub async fn create_file_message_by_file(
        &self,
        file: web_sys::File,
    ) -> Result<Message> {
        // TODO: 实现 Web 平台的文件读取
        Err(anyhow::anyhow!("Not implemented for Web platform"))
    }
    
    /// 创建合并消息
    pub async fn create_merge_message(
        &self,
        message_ids: Vec<String>,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        // 薄层：直接调用领域服务
        self.domain_service.create_merge_message(None, sender_id, message_ids)
    }
    
    /// 创建转发消息
    pub async fn create_forward_message(
        &self,
        message_ids: Vec<String>,
        forward_reason: Option<String>,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        // 薄层：直接调用领域服务
        self.domain_service.create_forward_message(None, sender_id, message_ids, forward_reason)
    }
    
    /// 创建定位消息
    pub async fn create_location_message(
        &self,
        longitude: f64,
        latitude: f64,
        address: String,
        description: Option<String>,
        poi_id: Option<String>,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        // 薄层：直接调用领域服务
        self.domain_service.create_location_message(
            None, sender_id, longitude, latitude, address, description, poi_id
        )
    }
    
    /// 创建引用消息（使用 quote 字段）
    pub async fn create_quote_message(
        &self,
        quoted_message_id: String,
        quoted_sender_id: Option<String>,
        quoted_text_preview: Option<String>,
        reply_content: Vec<u8>,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        // 薄层：直接调用领域服务
        self.domain_service.create_quote_message(
            None, sender_id, quoted_message_id, quoted_sender_id, quoted_text_preview, reply_content
        )
    }
    
    /// 创建引用消息（文本）
    pub async fn create_quote_text_message(
        &self,
        quoted_message_id: String,
        quoted_sender_id: Option<String>,
        quoted_text_preview: Option<String>,
        text: String,
    ) -> Result<Message> {
        // 构建文本内容
        use flare_proto::flare::common::v1::{MessageContent, message_content::Content, TextContent};
        use flare_proto::MessageContentExt;
        
        let text_content = TextContent {
            text,
            mentions: Vec::new(),
        };
        let mut content = MessageContent::default();
        content.content = Some(Content::Text(text_content));
        
        let reply_content = content.encode_to_bytes()
            .map_err(|e| anyhow::anyhow!("Failed to encode MessageContent: {}", e))?;
            
        // 从 FSM 获取 user_id
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
            
        self.domain_service.create_quote_message(
            None, 
            sender_id, 
            quoted_message_id, 
            quoted_sender_id, 
            quoted_text_preview, 
            reply_content
        )
    }

    /// 创建名片消息
    pub async fn create_card_message(
        &self,
        user_id: String,
        nickname: String,
        avatar_url: String,
        description: Option<String>,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        // 薄层：直接调用领域服务
        self.domain_service.create_card_message(
            None, sender_id, user_id, nickname, avatar_url, description
        )
    }
    
    /// 创建自定义消息
    pub async fn create_custom_message(
        &self,
        custom_type: String,
        payload: Vec<u8>,
        description: Option<String>,
        metadata: Option<std::collections::HashMap<String, String>>,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        // 薄层：直接调用领域服务
        self.domain_service.create_custom_message(
            None, sender_id, custom_type, payload, description, metadata
        )
    }
    
    /// 创建表情消息
    pub async fn create_face_message(
        &self,
        emoji: String,
    ) -> Result<Message> {
        // 从 FSM 获取 user_id
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        // 薄层：直接调用领域服务
        self.domain_service.create_face_message(None, sender_id, emoji)
    }
    
    
    /// 发送消息
    ///
    /// 薄层：直接调用 Application 层
    /// Sends a message
    ///
    /// Sends the message to the server and waits for acknowledgment.
    ///
    /// # Arguments
    ///
    /// * `message` - The message to send
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The message cannot be sent
    /// - The server rejects the message
    /// - Network error occurs
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::MessageFacade;
    /// # use flare_im_core_sdk::domain::message::Message;
    /// # async fn example(facade: &MessageFacade) -> anyhow::Result<()> {
    /// // 不再需要租户上下文
    /// let message = facade.create_text_message(
    ///     "Hello".to_string(),
    ///     Some("user2".to_string()),
    /// )?;
    /// facade.send_message(message, None).await?;
    /// # Ok(())
    /// # }
    /// ```
    /// Sends a message
    ///
    /// # Arguments
    ///
    /// * `message` - The message to send (Message domain model)
    /// * `conversation_id` - The conversation ID to send the message to
    ///
    /// # Note
    ///
    /// - Interface 层直接接受领域模型，提供类型安全和良好的 Rust 原生体验
    /// - The `conversation_id` will be set on the message before sending
    /// - If the message already has a `conversation_id`, it will be overwritten
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::MessageFacade;
    /// # async fn example(facade: &MessageFacade) -> anyhow::Result<()> {
    /// let message = facade.create_text_message(
    ///     "Hello".to_string(),
    ///     Some("user2".to_string()),
    /// ).await?;
    /// 
    /// // Send to a conversation
    /// facade.send_message(message, "conv1".to_string()).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn send_message(&self, mut message: Message, conversation_id: String) -> Result<()> {
        // 设置 conversation_id
        message.conversation_id = Some(conversation_id.clone());
        
        // 尝试获取会话信息以补充 receiver_id 和 correct conversation_type
        if let Ok(Some(conversation)) = self.conversation_repository.find_by_id(&conversation_id).await {
            // 1. 更新 conversation_type
            match conversation.conversation_type.as_str() {
                "single" | "Single" | "private" | "Private" => message.conversation_type = ConversationType::Single,
                "group" | "Group" => message.conversation_type = ConversationType::Group,
                "channel" | "Channel" => message.conversation_type = ConversationType::Channel,
                _ => {} 
            }
            
            // 2. 如果是单聊且没有 receiver_id，尝试自动填充
            if message.conversation_type == ConversationType::Single && message.receiver_id.is_none() {
                 let current_user_id = self.fsm.current_user_id().await.unwrap_or_default();
                 // Find peer
                 if let Some(peer) = conversation.participants.iter().find(|p| p.user_id != current_user_id) {
                     message.receiver_id = Some(peer.user_id.clone());
                 } else if !conversation.participants.is_empty() && conversation.participants.len() == 2 {
                     // 如果找不到"不是我"的人（例如 user_id 获取失败），且只有2人，取另一个
                     // 这种逻辑有点危险，但作为 fallback
                 }
            }
        }
        
        // 直接调用 Application 层
        self.command_handler.send_message_direct(message).await
    }
    
    /// 发送消息不通过 SDK 内置 OSS 上传多媒体文件
    ///
    /// # Arguments
    ///
    /// * `message` - The message to send (Message domain model)
    /// * `conversation_id` - The conversation ID to send the message to
    ///
    /// # Note
    ///
    /// - Interface 层直接接受领域模型，提供类型安全和良好的 Rust 原生体验
    /// - The `conversation_id` will be set on the message before sending
    pub async fn send_message_not_oss(&self, mut message: Message, conversation_id: String) -> Result<()> {
        // 设置 conversation_id
        message.conversation_id = Some(conversation_id.clone());
        
        // 尝试获取会话信息以补充 receiver_id 和 correct conversation_type
        if let Ok(Some(conversation)) = self.conversation_repository.find_by_id(&conversation_id).await {
            // 1. 更新 conversation_type
            match conversation.conversation_type.as_str() {
                "single" | "Single" | "private" | "Private" => message.conversation_type = ConversationType::Single,
                "group" | "Group" => message.conversation_type = ConversationType::Group,
                "channel" | "Channel" => message.conversation_type = ConversationType::Channel,
                _ => {} 
            }
            
            // 2. 如果是单聊且没有 receiver_id，尝试自动填充
            if message.conversation_type == ConversationType::Single && message.receiver_id.is_none() {
                 let current_user_id = self.fsm.current_user_id().await.unwrap_or_default();
                 // Find peer
                 if let Some(peer) = conversation.participants.iter().find(|p| p.user_id != current_user_id) {
                     message.receiver_id = Some(peer.user_id.clone());
                 }
            }
        }

        // 直接调用 Application 层（与 send_message 相同，但跳过媒体文件上传）
        self.command_handler.send_message_direct(message).await
    }
    
    
    /// 撤回一条消息
    ///
    /// # 参数
    /// * `client_msg_id` - 客户端消息ID（client_msg_id）
    /// * `reason` - 撤回原因（可选）
    ///
    /// 薄层：直接调用 Application 层
    pub async fn revoke_message(
        &self,
        client_msg_id: String,
        reason: Option<String>,
    ) -> Result<()> {
        use crate::application::commands::RecallMessageCommand;
        self.command_handler.recall_message(RecallMessageCommand {
            client_msg_id,
            reason,
        }).await
    }
    
    /// 编辑一条消息
    ///
    /// # 参数
    /// * `client_msg_id` - 客户端消息ID（client_msg_id）
    /// * `new_content` - 新的消息内容（序列化的 MessageContent）
    /// * `reason` - 编辑原因（可选）
    ///
    /// 薄层：直接调用 Application 层
    pub async fn edit_message(
        &self,
        client_msg_id: String,
        new_content: Vec<u8>,
        reason: Option<String>,
    ) -> Result<()> {
        use crate::application::commands::EditMessageCommand;
        self.command_handler.edit_message(EditMessageCommand {
            client_msg_id,
            new_content,
            reason,
        }).await
    }
    
    /// 编辑一条消息（文本）
    ///
    /// # 参数
    /// * `client_msg_id` - 客户端消息ID
    /// * `text` - 新的文本内容
    /// * `reason` - 编辑原因（可选）
    pub async fn edit_text_message(
        &self,
        client_msg_id: String,
        text: String,
        reason: Option<String>,
    ) -> Result<()> {
        // 构建文本内容
        use flare_proto::flare::common::v1::{MessageContent, message_content::Content, TextContent};
        use flare_proto::MessageContentExt;
        
        let text_content = TextContent {
            text,
            mentions: Vec::new(),
        };
        let mut content = MessageContent::default();
        content.content = Some(Content::Text(text_content));
        
        let new_content = content.encode_to_bytes()
            .map_err(|e| anyhow::anyhow!("Failed to encode MessageContent: {}", e))?;
            
        self.edit_message(client_msg_id, new_content, reason).await
    }

    /// 回复一条消息（文本）
    pub async fn reply_text_message(
        &self,
        conversation_id: String,
        quoted_message_id: String,
        quoted_sender_id: Option<String>,
        quoted_text_preview: Option<String>,
        text: String,
    ) -> Result<String> {
        // 构建文本内容
        use flare_proto::flare::common::v1::{MessageContent, message_content::Content, TextContent};
        use flare_proto::MessageContentExt;
        
        let text_content = TextContent {
            text,
            mentions: Vec::new(),
        };
        let mut content = MessageContent::default();
        content.content = Some(Content::Text(text_content));
        
        let reply_content = content.encode_to_bytes()
            .map_err(|e| anyhow::anyhow!("Failed to encode MessageContent: {}", e))?;
            
        self.reply_message(
            conversation_id,
            quoted_message_id,
            quoted_sender_id,
            quoted_text_preview,
            reply_content,
        ).await
    }

    /// 删除一条消息
    ///
    /// # 参数
    /// * `client_msg_id` - 客户端消息ID（client_msg_id）
    /// * `delete_type` - 删除类型
    /// * `reason` - 删除原因（可选）
    ///
    /// 薄层：直接调用 Application 层
    /// Deletes a message
    ///
    /// Deletes a message (soft delete by default). The message will be hidden
    /// from the user but may still exist in the database.
    ///
    /// # Arguments
    ///
    /// * `client_msg_id` - The client message ID of the message to delete
    /// * `delete_type` - The type of delete (soft or hard)
    /// * `reason` - Optional reason for deletion
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The message is not found
    /// - The user doesn't have permission to delete
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::MessageFacade;
    /// # use flare_im_core_sdk::domain::message::DeleteType;
    /// # async fn example(facade: &MessageFacade) -> anyhow::Result<()> {
    /// facade.delete_message(
    ///     "client_msg_123".to_string(),
    ///     DeleteType::Soft,
    ///     None,
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn delete_message(
        &self,
        client_msg_id: String,
        delete_type: DeleteType,
        reason: Option<String>,
    ) -> Result<()> {
        use crate::application::commands::DeleteMessageCommand;
        self.command_handler.delete_message(DeleteMessageCommand {
            client_msg_id,
            delete_type,
            reason,
        }).await
    }

    /// Recalls a message
    ///
    /// # Arguments
    ///
    /// * `client_msg_id` - The client message ID to recall
    /// * `reason` - Optional reason for recalling
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn recall_message(
        &self,
        client_msg_id: String,
        reason: Option<String>,
    ) -> Result<()> {
        use crate::application::commands::RecallMessageCommand;
        self.command_handler.recall_message(RecallMessageCommand {
            client_msg_id,
            reason,
        }).await
    }
    
    /// 本地删除一条消息
    ///
    /// 薄层：直接调用 Application 层
    pub async fn delete_message_from_local_storage(
        &self,
        client_msg_id: String,
    ) -> Result<()> {
        // 软删除：仅从本地存储删除，不通知服务器
        use crate::application::commands::DeleteMessageCommand;
        self.command_handler.delete_message(DeleteMessageCommand {
            client_msg_id,
            delete_type: DeleteType::Soft,
            reason: Some("Local delete".to_string()),
        }).await
    }
    
    /// 本地删除消息
    ///
    /// 薄层：直接调用 Application 层
    /// 对标微信、Telegram、飞书的批量删除功能
    pub async fn delete_all_msg_from_local(
        &self,
        conversation_id: String,
    ) -> Result<()> {
        // 查询会话中的所有消息
        use crate::application::queries::ListMessagesQuery;
        let messages = self.query_handler.list_messages(ListMessagesQuery {
            conversation_id: conversation_id.clone(),
            limit: None,
            cursor: None,
        }).await?;
        
        // 批量删除（软删除）
        for message in messages {
            // 使用 server_id 或 client_msg_id 作为 message_id
            let message_id: &str = message.server_id.as_ref().map(|s| s.as_str()).unwrap_or(&message.client_msg_id);
                // 从 MessageRepository 删除（软删除）
                let _ = self.message_repository.delete(message_id).await;
        }
        
        Ok(())
    }
    
    /// 删除消息（本地+服务器）
    ///
    /// 薄层：直接调用 Application 层
    pub async fn delete_all_msg_from_local_and_svr(
        &self,
        _conversation_id: String,
    ) -> Result<()> {
        // 硬删除：从本地和服务器都删除
        // TODO: 实现批量删除消息的命令
        // 暂时使用单个删除（需要重构）
        Err(anyhow::anyhow!("clear_conversation_and_delete_all_msg not implemented with new CommandHandler"))
    }
    
    /// 设置消息拓展信息
    ///
    /// 薄层：直接调用 Application 层
    /// 对标微信、Telegram、飞书的消息扩展信息功能
    pub async fn set_message_local_ex(
        &self,
        message_id: String,
        ext: std::collections::HashMap<String, String>,
    ) -> Result<()> {
        // 从 MessageRepository 加载消息
        if let Some(mut message) = self.message_repository.find_by_id(&message_id).await? {
            // 更新扩展信息
            message.extra.extend(ext);
            message.version += 1;
            message.updated_at = chrono::Utc::now();
            
            // 保存消息到 MessageRepository
            self.message_repository.save(&message).await?;
        }
        
        Ok(())
    }
    
    /// Replies to a message
    ///
    /// Creates a reply message that references another message using the quote field.
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - The conversation ID
    /// * `sender_id` - The sender ID
    /// * `quoted_message_id` - The ID of the message being replied to
    /// * `quoted_sender_id` - Optional sender ID of the quoted message
    /// * `quoted_text_preview` - Optional text preview of the quoted message
    /// * `reply_content` - The reply message content (serialized MessageContent)
    ///
    /// # Returns
    ///
    /// Returns the client message ID of the reply message.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::MessageFacade;
    /// # async fn example(facade: &MessageFacade) -> anyhow::Result<()> {
    /// // 不再需要租户上下文
    /// let reply_id = facade.reply_message(
    ///     "conv1".to_string(),
    ///     "msg_123".to_string(),
    ///     Some("user2".to_string()),
    ///     Some("Original message".to_string()),
    ///     b"Reply content".to_vec(),
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn reply_message(
        &self,
        conversation_id: String,
        quoted_message_id: String,
        quoted_sender_id: Option<String>,
        quoted_text_preview: Option<String>,
        reply_content: Vec<u8>,
    ) -> Result<String> {
        use crate::application::commands::ReplyMessageCommand;
        self.command_handler.reply_message(ReplyMessageCommand {
            conversation_id,
            quoted_message_id,
            quoted_sender_id,
            quoted_text_preview,
            reply_content,
        }).await
    }
    
    /// Forwards messages to another conversation
    ///
    /// # Arguments
    ///
    /// * `message_ids` - Vector of message IDs to forward
    /// * `target_conversation_id` - The target conversation ID
    /// * `merge_forward` - Whether to merge multiple messages into one forward message
    ///
    /// # Returns
    ///
    /// Returns a vector of client message IDs for the forwarded messages.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::MessageFacade;
    /// # async fn example(facade: &MessageFacade) -> anyhow::Result<()> {
    /// // 不再需要租户上下文
    /// let message_ids = facade.forward_messages(
    ///     vec!["msg1".to_string(), "msg2".to_string()],
    ///     "conv2".to_string(),
    ///     true,
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn forward_messages(
        &self,
        message_ids: Vec<String>,
        target_conversation_id: String,
        merge_forward: bool,
    ) -> Result<Vec<String>> {
        use crate::application::commands::ForwardMessagesCommand;
        self.command_handler.forward_messages(ForwardMessagesCommand {
            message_ids,
            target_conversation_id,
            merge_forward,
        }).await
    }
    /// Adds a thread reply
    ///
    /// Adds a reply to a thread (topic) within a conversation.
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - The conversation ID
    /// * `thread_id` - The thread ID
    /// * `reply_content` - The reply content (serialized MessageContent)
    ///
    /// # Returns
    ///
    /// Returns the client message ID of the thread reply.
    pub async fn add_thread_reply(
        &self,
        conversation_id: String,
        thread_id: String,
        reply_content: Vec<u8>,
    ) -> Result<String> {
        use crate::application::commands::AddThreadReplyCommand;
        self.command_handler.add_thread_reply(AddThreadReplyCommand {
            conversation_id,
            thread_id,
            reply_content,
        }).await
    }
    
    /// 添加线程回复（文本）
    pub async fn add_thread_text_reply(
        &self,
        conversation_id: String,
        thread_id: String,
        text: String,
    ) -> Result<String> {
        // 构建文本内容
        use flare_proto::flare::common::v1::{MessageContent, message_content::Content, TextContent};
        use flare_proto::MessageContentExt;
        
        let text_content = TextContent {
            text,
            mentions: Vec::new(),
        };
        let mut content = MessageContent::default();
        content.content = Some(Content::Text(text_content));
        
        let reply_content = content.encode_to_bytes()
            .map_err(|e| anyhow::anyhow!("Failed to encode MessageContent: {}", e))?;
            
        self.add_thread_reply(
            conversation_id,
            thread_id,
            reply_content,
        ).await
    }

    /// Pins a message
    ///
    /// Pins a message to the top of the conversation.
    ///
    /// # Arguments
    ///
    /// * `message_id` - The message ID to pin
    /// * `reason` - Optional reason for pinning
    /// * `expire_at` - Optional expiration time for the pin
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn pin_message(
        &self,
        message_id: String,
        reason: Option<String>,
        expire_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<()> {
        use crate::application::commands::PinMessageCommand;
        self.command_handler.pin_message(PinMessageCommand {
            message_id,
            reason,
            expire_at,
        }).await
    }
    
    /// Unpins a message
    ///
    /// # Arguments
    ///
    /// * `message_id` - The message ID to unpin
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn unpin_message(
        &self,
        message_id: String,
    ) -> Result<()> {
        use crate::application::commands::UnpinMessageCommand;
        self.command_handler.unpin_message(UnpinMessageCommand {
            message_id,
        }).await
    }
    
    /// Favorites a message
    ///
    /// # Arguments
    ///
    /// * `message_id` - The message ID to favorite
    /// * `tags` - Optional tags for the favorite
    /// * `note` - Optional note for the favorite
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn favorite_message(
        &self,
        message_id: String,
        tags: Vec<String>,
        note: Option<String>,
    ) -> Result<()> {
        use crate::application::commands::FavoriteMessageCommand;
        self.command_handler.favorite_message(FavoriteMessageCommand {
            message_id,
            tags,
            note,
        }).await
    }
    
    /// Unfavorites a message
    ///
    /// # Arguments
    ///
    /// * `message_id` - The message ID to unfavorite
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn unfavorite_message(
        &self,
        message_id: String,
    ) -> Result<()> {
        use crate::application::commands::UnfavoriteMessageCommand;
        self.command_handler.unfavorite_message(UnfavoriteMessageCommand {
            message_id,
        }).await
    }
    
    /// Marks a message with a specific mark type
    ///
    /// # Arguments
    ///
    /// * `message_id` - The message ID to mark
    /// * `mark_type` - The mark type (e.g., Important, Todo, Done)
    /// * `color` - Optional color for the mark
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn mark_message(
        &self,
        message_id: String,
        mark_type: crate::domain::message::MarkType,
        color: Option<String>,
    ) -> Result<()> {
        use crate::application::commands::MarkMessageCommand;
        self.command_handler.mark_message(MarkMessageCommand {
            message_id,
            mark_type,
            color,
        }).await
    }
    
    /// Marks multiple messages as read in batch
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - The conversation ID
    /// * `message_ids` - Vector of message IDs to mark as read
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn batch_mark_message_read(
        &self,
        conversation_id: String,
        message_ids: Option<Vec<String>>,
        burn_after_read: bool,
    ) -> Result<()> {
        use crate::application::commands::BatchMarkMessageReadCommand;
        self.command_handler.batch_mark_message_read(BatchMarkMessageReadCommand {
            conversation_id,
            message_ids,
            burn_after_read,
        }).await
    }
    
    pub async fn add_reaction(
        &self,
        message_id: String,
        emoji: String,
    ) -> Result<()> {
        use crate::application::commands::AddReactionCommand;
        self.command_handler.add_reaction(AddReactionCommand {
            message_id,
            emoji,
        }).await
    }
    
    pub async fn remove_reaction(
        &self,
        message_id: String,
        emoji: String,
    ) -> Result<()> {
        use crate::application::commands::RemoveReactionCommand;
        self.command_handler.remove_reaction(RemoveReactionCommand {
            message_id,
            emoji,
        }).await
    }
    
    
    /// Searches for messages in local storage
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - Optional conversation ID to limit search scope
    /// * `keyword` - The search keyword
    /// * `limit` - Optional limit on the number of results
    ///
    /// # Returns
    ///
    /// Returns a vector of message JSON objects matching the search criteria.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::MessageFacade;
    /// # async fn example(facade: &MessageFacade) -> anyhow::Result<()> {
    /// let results = facade.search_local_messages(
    ///     Some("conv1".to_string()),
    ///     "hello".to_string(),
    ///     Some(10),
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn search_local_messages(
        &self,
        conversation_id: Option<String>,
        keyword: String,
        limit: Option<usize>,
    ) -> Result<Vec<Message>> {
        use crate::application::queries::SearchMessagesQuery;
        self.query_handler.search_messages(SearchMessagesQuery {
            conversation_id,
            keyword,
            limit,
        }).await
    }
    
    /// Gets advanced history message list with sequence range support
    ///
    /// Similar to WeChat, Telegram, and Feishu's history message query with seq range support.
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - The conversation ID
    /// * `start_seq` - Optional start sequence number
    /// * `end_seq` - Optional end sequence number
    /// * `limit` - Optional limit on the number of results
    ///
    /// # Returns
    ///
    /// Returns a vector of message JSON objects within the specified sequence range.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::MessageFacade;
    /// # async fn example(facade: &MessageFacade) -> anyhow::Result<()> {
    /// let messages = facade.get_advanced_history_message_list(
    ///     "conv1".to_string(),
    ///     Some(100),
    ///     Some(200),
    ///     Some(50),
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_advanced_history_message_list(
        &self,
        conversation_id: String,
        start_seq: Option<u64>,
        end_seq: Option<u64>,
        limit: Option<usize>,
    ) -> Result<Vec<Message>> {
        // 实现基于 seq 范围的消息查询
        // 当前实现：先获取所有消息，然后按 seq 过滤
        // 实际实现中，ReadStore 应该支持基于 seq 的查询优化
        
        use crate::application::queries::ListMessagesQuery;
        let messages = self.query_handler.list_messages(ListMessagesQuery {
            conversation_id,
            limit: None,
            cursor: None,
        }).await?;
        
        // 按 seq 过滤
        let filtered: Vec<Message> = messages
            .into_iter()
            .filter(|msg| {
                let seq = msg.seq.unwrap_or(0);
                if let Some(start) = start_seq {
                    if seq < start {
                        return false;
                    }
                }
                if let Some(end) = end_seq {
                    if seq > end {
                        return false;
                    }
                }
                true
            })
            .take(limit.unwrap_or(100))
            .collect();
        
        Ok(filtered)
    }
    
    /// 反向获取会话内消息列表
    ///
    /// 薄层：直接调用 Application 层
    /// 对标微信、Telegram、飞书的反向历史消息查询（从新到旧）
    pub async fn get_advanced_history_message_list_reverse(
        &self,
        conversation_id: String,
        start_seq: Option<u64>,
        end_seq: Option<u64>,
        limit: Option<usize>,
    ) -> Result<Vec<Message>> {
        // 实现反向查询（从新到旧）
        // 先获取消息，然后反转顺序
        let messages = self.get_advanced_history_message_list(
            conversation_id,
            start_seq,
            end_seq,
            limit,
        ).await?;
        
        // 按时间戳或 seq 降序排序（最新的在前）
        let mut sorted = messages;
        sorted.sort_by(|a, b| {
            let a_seq = a.seq.unwrap_or(0);
            let b_seq = b.seq.unwrap_or(0);
            b_seq.cmp(&a_seq) // 降序
        });
        
        Ok(sorted)
    }
    
    /// 查找消息列表
    ///
    /// 薄层：直接调用 Application 层
    /// 对标微信、Telegram、飞书的高级消息查询（按类型、时间范围过滤）
    pub async fn find_message_list(
        &self,
        conversation_id: Option<String>,
        _message_type: Option<MessageType>,
        start_time: Option<chrono::DateTime<chrono::Utc>>,
        end_time: Option<chrono::DateTime<chrono::Utc>>,
        limit: Option<usize>,
    ) -> Result<Vec<Message>> {
        // 实现高级消息查询（按类型、时间范围过滤）
        self.message_repository
            .find_by_time_range(
                conversation_id.as_deref(),
                start_time,
                end_time,
                limit,
            )
            .await
    }
    
    // ============================================================================
    // 本地存储 API
    // ============================================================================
    
    /// 插入一条单聊消息到本地
    ///
    /// 薄层：直接调用 Application 层
    /// 对标微信、Telegram、飞书的离线消息同步机制
    /// 直接写入 ReadStore（不经过 EventStore），用于离线消息同步场景
    pub async fn insert_single_message_to_local_storage(
        &self,
        message: Message,
    ) -> Result<()> {
        // 直接写入 MessageRepository（不经过 EventStore）
        // 用于离线消息同步场景，避免重复的事件投影
        self.message_repository.save(&message).await?;
        
        // 更新或创建会话
        use crate::domain::service::ConversationDomainService;
        
        let conversation_id = message.conversation_id.clone()
            .ok_or_else(|| anyhow::anyhow!("Message has no conversation_id"))?;
        
        let mut conversation = self.conversation_repository.find_by_id(&conversation_id).await?;
        
        if let Some(ref mut conv) = conversation {
            // 更新现有会话
            let domain_service = ConversationDomainService::new();
            domain_service.update_last_message(conv, &message)?;
            self.conversation_repository.update(conv).await?;
        } else {
            // 创建新会话
            let conv = ConversationDomainService::new().create_conversation_from_message(&message)?;
            self.conversation_repository.save(&conv).await?;
        }
        
        Ok(())
    }
    
    /// 插入一条群聊消息到本地
    ///
    /// 薄层：直接调用 Application 层
    /// 对标微信、Telegram、飞书的离线消息同步机制
    /// 直接写入 ReadStore（不经过 EventStore），用于离线消息同步场景
    pub async fn insert_group_message_to_local_storage(
        &self,
        message: Message,
    ) -> Result<()> {
        // 群聊消息和单聊消息的处理逻辑相同
        // 直接调用单聊消息的插入逻辑
        self.insert_single_message_to_local_storage(message).await
    }
    
    // ============================================================================
    // 输入状态 API
    // ============================================================================
    
    /// 单聊正在输入消息
    ///
    /// 薄层：直接调用 Application 层
    pub async fn typing_status_update(
        &self,
        conversation_id: String,
        is_typing: bool,
    ) -> Result<()> {
        use crate::domain::conversation::InputStateType;
        let state_type = if is_typing {
            InputStateType::Typing
        } else {
            InputStateType::Stopped
        };
        use crate::application::commands::SetInputStateCommand;
        self.command_handler.set_input_state(SetInputStateCommand {
            conversation_id,
            state_type,
        }).await
    }

    
}
