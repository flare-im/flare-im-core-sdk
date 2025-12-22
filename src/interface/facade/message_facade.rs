//! 消息 Facade
//!
//! 提供所有消息相关的 API
//! 对标微信、Telegram、飞书的生产级别实现

use std::sync::Arc;
use crate::application::handlers::{CommandHandler, QueryHandler};
use crate::domain::message::*;
use crate::domain::service::{
    MessageDomainService, 
    MediaDomainService,
    MentionInfo, 
    MentionInfoType,
};
use crate::infrastructure::storage::media_cache::MediaCacheManager;
use anyhow::Result;

/// 消息 Facade
///
/// 职责：薄薄的一层，只负责调用 Application 层
/// 所有业务逻辑都在领域服务中实现
pub struct MessageFacade {
    command_handler: Arc<CommandHandler>,
    query_handler: Arc<QueryHandler>,
    read_store: Arc<dyn crate::domain::repository::ReadStore>,
    domain_service: MessageDomainService,
    media_service: MediaDomainService,
    media_cache: Arc<MediaCacheManager>,
}

impl MessageFacade {
    pub fn new(
        command_handler: Arc<CommandHandler>,
        query_handler: Arc<QueryHandler>,
        read_store: Arc<dyn crate::domain::repository::ReadStore>,
        media_cache: Arc<MediaCacheManager>,
    ) -> Self {
        Self {
            command_handler,
            query_handler,
            read_store,
            domain_service: MessageDomainService::new(),
            media_service: MediaDomainService::new(),
            media_cache,
        }
    }
    
    // ============================================================================
    // 消息创建 API
    // ============================================================================
    
    /// 创建文本消息
    ///
    /// # 参数
    /// * `conversation_id` - 会话 ID
    /// * `sender_id` - 发送者 ID
    /// * `text` - 消息文本内容
    /// * `tenant` - 租户上下文
    /// * `receiver_id` - 接收者 ID（单聊时必需，群聊时可选）
    ///
    /// # 注意
    /// 对于单聊消息，`receiver_id` 是必需的，Message Orchestrator 会验证此字段
    pub fn create_text_message(
        &self,
        conversation_id: String,
        sender_id: String,
        text: String,
        tenant: TenantContext,
        receiver_id: Option<String>,
    ) -> Result<Message> {
        // 薄层：直接调用领域服务
        use crate::domain::message::build_text_message;
        build_text_message(conversation_id, sender_id, text, tenant, receiver_id)
    }
    
    /// 创建@消息
    pub fn create_text_at_message(
        &self,
        conversation_id: String,
        sender_id: String,
        text: String,
        mentions: Vec<MentionInfo>,
        tenant: TenantContext,
    ) -> Result<Message> {
        // 薄层：直接调用领域服务
        self.domain_service.create_text_at_message(conversation_id, sender_id, text, mentions, tenant)
    }
    
    /// 根据文件绝对路径创建图片消息
    pub async fn create_image_message_from_full_path(
        &self,
        conversation_id: String,
        sender_id: String,
        file_path: String,
        tenant: TenantContext,
    ) -> Result<Message> {
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
            &tenant,
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
            conversation_id,
            sender_id,
            image_url,
            Some(local_path.to_string_lossy().to_string()),
            tenant,
        )?;
        
        // 添加附件信息
        message.attachments.push(attachment);
        
        Ok(message)
    }
    
    /// 自行上传文件并创建图片消息
    pub async fn create_image_message_by_url(
        &self,
        conversation_id: String,
        sender_id: String,
        image_url: String,
        tenant: TenantContext,
    ) -> Result<Message> {
        use crate::domain::message::build_image_message;
        build_image_message(
            conversation_id,
            sender_id,
            image_url,
            None, // 没有本地路径
            tenant,
        )
    }
    
    /// 根据文件对象创建图片消息（Web）
    #[cfg(target_arch = "wasm32")]
    pub async fn create_image_message_by_file(
        &self,
        conversation_id: String,
        sender_id: String,
        file: web_sys::File,
        tenant: TenantContext,
    ) -> Result<Message> {
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
            conversation_id,
            sender_id,
            image_url,
            Some(local_path.to_string_lossy().to_string()),
            tenant,
        )
    }
    
    /// 根据文件绝对路径创建语音消息
    pub async fn create_sound_message_from_full_path(
        &self,
        conversation_id: String,
        sender_id: String,
        file_path: String,
        duration_ms: u64,
        tenant: TenantContext,
    ) -> Result<Message> {
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
            &tenant,
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
            conversation_id,
            sender_id,
            audio_url,
            Some(local_path.to_string_lossy().to_string()),
            duration_ms,
            tenant,
        )?;
        
        // 添加附件信息
        message.attachments.push(attachment);
        
        Ok(message)
    }
    
    /// 自行上传文件并创建语音消息
    pub async fn create_sound_message_by_url(
        &self,
        conversation_id: String,
        sender_id: String,
        audio_url: String,
        duration_ms: u64,
        tenant: TenantContext,
    ) -> Result<Message> {
        use crate::domain::message::build_audio_message;
        build_audio_message(
            conversation_id,
            sender_id,
            audio_url,
            None,
            duration_ms,
            tenant,
        )
    }
    
    /// 根据文件对象创建语音消息（Web）
    #[cfg(target_arch = "wasm32")]
    pub async fn create_sound_message_by_file(
        &self,
        conversation_id: String,
        sender_id: String,
        file: web_sys::File,
        duration_ms: u64,
        tenant: TenantContext,
    ) -> Result<Message> {
        // 类似 create_image_message_by_file 的实现
        // TODO: 实现 Web 平台的文件读取
        Err(anyhow::anyhow!("Not implemented for Web platform"))
    }
    
    /// 根据文件绝对路径创建视频消息
    pub async fn create_video_message_from_full_path(
        &self,
        conversation_id: String,
        sender_id: String,
        file_path: String,
        duration_ms: u64,
        width: i32,
        height: i32,
        tenant: TenantContext,
    ) -> Result<Message> {
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
            &tenant,
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
            conversation_id,
            sender_id,
            video_url,
            Some(local_path.to_string_lossy().to_string()),
            duration_ms,
            width,
            height,
            tenant,
        )?;
        
        // 添加附件信息
        message.attachments.push(attachment);
        
        Ok(message)
    }
    
    /// 自行上传文件并创建视频消息
    pub async fn create_video_message_by_url(
        &self,
        conversation_id: String,
        sender_id: String,
        video_url: String,
        duration_ms: u64,
        width: i32,
        height: i32,
        tenant: TenantContext,
    ) -> Result<Message> {
        use crate::domain::message::build_video_message;
        build_video_message(
            conversation_id,
            sender_id,
            video_url,
            None,
            duration_ms,
            width,
            height,
            tenant,
        )
    }
    
    /// 根据文件对象创建视频消息（Web）
    #[cfg(target_arch = "wasm32")]
    pub async fn create_video_message_by_file(
        &self,
        conversation_id: String,
        sender_id: String,
        file: web_sys::File,
        duration_ms: u64,
        width: i32,
        height: i32,
        tenant: TenantContext,
    ) -> Result<Message> {
        // TODO: 实现 Web 平台的文件读取
        Err(anyhow::anyhow!("Not implemented for Web platform"))
    }
    
    /// 根据文件绝对路径创建文件消息
    pub async fn create_file_message_from_full_path(
        &self,
        conversation_id: String,
        sender_id: String,
        file_path: String,
        tenant: TenantContext,
    ) -> Result<Message> {
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
            &tenant,
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
            conversation_id,
            sender_id,
            file_url,
            file_name,
            file_size,
            mime_type,
            Some(local_path.to_string_lossy().to_string()),
            tenant,
        )?;
        
        // 添加附件信息
        message.attachments.push(attachment);
        
        Ok(message)
    }
    
    /// 自行上传文件并创建文件消息
    pub async fn create_file_message_by_url(
        &self,
        conversation_id: String,
        sender_id: String,
        file_url: String,
        file_name: String,
        file_size: u64,
        mime_type: String,
        tenant: TenantContext,
    ) -> Result<Message> {
        use crate::domain::message::build_file_message;
        build_file_message(
            conversation_id,
            sender_id,
            file_url,
            file_name,
            file_size,
            mime_type,
            None,
            tenant,
        )
    }
    
    /// 根据文件对象创建文件消息（Web）
    #[cfg(target_arch = "wasm32")]
    pub async fn create_file_message_by_file(
        &self,
        conversation_id: String,
        sender_id: String,
        file: web_sys::File,
        tenant: TenantContext,
    ) -> Result<Message> {
        // TODO: 实现 Web 平台的文件读取
        Err(anyhow::anyhow!("Not implemented for Web platform"))
    }
    
    /// 创建合并消息
    pub fn create_merge_message(
        &self,
        conversation_id: String,
        sender_id: String,
        message_ids: Vec<String>,
        tenant: TenantContext,
    ) -> Result<Message> {
        // 薄层：直接调用领域服务
        self.domain_service.create_merge_message(conversation_id, sender_id, message_ids, tenant)
    }
    
    /// 创建转发消息
    pub fn create_forward_message(
        &self,
        conversation_id: String,
        sender_id: String,
        message_ids: Vec<String>,
        forward_reason: Option<String>,
        tenant: TenantContext,
    ) -> Result<Message> {
        // 薄层：直接调用领域服务
        self.domain_service.create_forward_message(conversation_id, sender_id, message_ids, forward_reason, tenant)
    }
    
    /// 创建定位消息
    pub fn create_location_message(
        &self,
        conversation_id: String,
        sender_id: String,
        longitude: f64,
        latitude: f64,
        address: String,
        description: Option<String>,
        poi_id: Option<String>,
        tenant: TenantContext,
    ) -> Result<Message> {
        // 薄层：直接调用领域服务
        self.domain_service.create_location_message(
            conversation_id, sender_id, longitude, latitude, address, description, poi_id, tenant
        )
    }
    
    /// 创建引用消息
    pub fn create_quote_message(
        &self,
        conversation_id: String,
        sender_id: String,
        quoted_message_id: String,
        quoted_sender_id: String,
        quoted_text_preview: String,
        reply_content: Vec<u8>,
        tenant: TenantContext,
    ) -> Result<Message> {
        // 薄层：直接调用领域服务
        self.domain_service.create_quote_message(
            conversation_id, sender_id, quoted_message_id, quoted_sender_id, quoted_text_preview, reply_content, tenant
        )
    }
    
    /// 创建名片消息
    pub fn create_card_message(
        &self,
        conversation_id: String,
        sender_id: String,
        user_id: String,
        nickname: String,
        avatar_url: String,
        description: Option<String>,
        tenant: TenantContext,
    ) -> Result<Message> {
        // 薄层：直接调用领域服务
        self.domain_service.create_card_message(
            conversation_id, sender_id, user_id, nickname, avatar_url, description, tenant
        )
    }
    
    /// 创建自定义消息
    pub fn create_custom_message(
        &self,
        conversation_id: String,
        sender_id: String,
        custom_type: String,
        payload: Vec<u8>,
        description: Option<String>,
        metadata: Option<std::collections::HashMap<String, String>>,
        tenant: TenantContext,
    ) -> Result<Message> {
        // 薄层：直接调用领域服务
        self.domain_service.create_custom_message(
            conversation_id, sender_id, custom_type, payload, description, metadata, tenant
        )
    }
    
    /// 创建表情消息
    pub fn create_face_message(
        &self,
        conversation_id: String,
        sender_id: String,
        emoji: String,
        tenant: TenantContext,
    ) -> Result<Message> {
        // 薄层：直接调用领域服务
        self.domain_service.create_face_message(conversation_id, sender_id, emoji, tenant)
    }
    
    // ============================================================================
    // 消息发送 API
    // ============================================================================
    
    /// 发送消息
    ///
    /// 薄层：直接调用 Application 层
    pub async fn send_message(&self, message: Message) -> Result<()> {
        use crate::application::commands::SendMessageCommand;
        self.command_handler.send_message_direct(message).await
    }
    
    /// 发送消息不通过 SDK 内置 OSS 上传多媒体文件
    ///
    /// 薄层：直接调用 Application 层
    pub async fn send_message_not_oss(&self, message: Message) -> Result<()> {
        // 与 send_message 相同，但跳过媒体文件上传
        // 假设 URL 已经由外部上传服务提供
        self.command_handler.send_message_direct(message).await
    }
    
    // ============================================================================
    // 消息操作 API
    // ============================================================================
    
    /// 撤回一条消息
    ///
    /// 薄层：直接调用 Application 层
    pub async fn revoke_message(
        &self,
        message_id: String,
        recaller_id: String,
        reason: Option<String>,
    ) -> Result<()> {
        use crate::application::commands::RecallMessageCommand;
        self.command_handler.recall_message(RecallMessageCommand {
            message_id,
            recaller_id,
            reason,
        }).await
    }
    
    /// 删除一条消息
    ///
    /// 薄层：直接调用 Application 层
    pub async fn delete_message(
        &self,
        message_id: String,
        operator_id: String,
        delete_type: DeleteType,
        reason: Option<String>,
    ) -> Result<()> {
        use crate::application::commands::DeleteMessageCommand;
        self.command_handler.delete_message(DeleteMessageCommand {
            message_id,
            operator_id,
            delete_type,
            reason,
        }).await
    }
    
    /// 本地删除一条消息
    ///
    /// 薄层：直接调用 Application 层
    pub async fn delete_message_from_local_storage(
        &self,
        message_id: String,
    ) -> Result<()> {
        // 软删除：仅从本地存储删除，不通知服务器
        use crate::application::commands::DeleteMessageCommand;
        self.command_handler.delete_message(DeleteMessageCommand {
            message_id,
            operator_id: "local".to_string(),
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
        for msg_json in messages {
            if let Some(message_id) = msg_json.get("message_id").and_then(|v| v.as_str()) {
                // 从 ReadStore 删除（软删除）
                let _ = self.read_store.delete_message(message_id).await;
            }
        }
        
        Ok(())
    }
    
    /// 删除消息（本地+服务器）
    ///
    /// 薄层：直接调用 Application 层
    pub async fn delete_all_msg_from_local_and_svr(
        &self,
        conversation_id: String,
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
        // 从 ReadStore 加载消息
        use crate::domain::repository::{Query, QueryResult};
        let query = Query::MessageDetail {
            message_id: message_id.clone(),
        };
        
        let result = self.read_store.query(query).await?;
        
        if let QueryResult::MessageDetail { item } = result {
            if !item.is_null() && item.get("message_id").is_some() {
                // 反序列化消息
                if let Ok(mut message) = serde_json::from_value::<Message>(item) {
                    // 更新扩展信息
                    message.extra.extend(ext);
                    message.version += 1;
                    message.updated_at = chrono::Utc::now();
                    
                    // 保存消息到 ReadStore
                    self.read_store.write_message(&message).await?;
                }
            }
        }
        
        Ok(())
    }
    
    // ============================================================================
    // 消息查询 API
    // ============================================================================
    
    /// 查找本地消息
    ///
    /// 薄层：直接调用 Application 层
    pub async fn search_local_messages(
        &self,
        conversation_id: Option<String>,
        keyword: String,
        limit: Option<usize>,
    ) -> Result<Vec<serde_json::Value>> {
        use crate::application::queries::SearchMessagesQuery;
        self.query_handler.search_messages(SearchMessagesQuery {
            conversation_id,
            keyword,
            limit,
        }).await
    }
    
    /// 获取会话内消息列表
    ///
    /// 薄层：直接调用 Application 层
    /// 对标微信、Telegram、飞书的历史消息查询（支持 seq 范围）
    pub async fn get_advanced_history_message_list(
        &self,
        conversation_id: String,
        start_seq: Option<u64>,
        end_seq: Option<u64>,
        limit: Option<usize>,
    ) -> Result<Vec<serde_json::Value>> {
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
        let filtered: Vec<serde_json::Value> = messages
            .into_iter()
            .filter(|msg| {
                let seq = msg.get("seq").and_then(|v| v.as_u64()).unwrap_or(0);
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
    ) -> Result<Vec<serde_json::Value>> {
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
            let a_seq = a.get("seq").and_then(|v| v.as_u64()).unwrap_or(0);
            let b_seq = b.get("seq").and_then(|v| v.as_u64()).unwrap_or(0);
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
        message_type: Option<MessageType>,
        start_time: Option<chrono::DateTime<chrono::Utc>>,
        end_time: Option<chrono::DateTime<chrono::Utc>>,
        limit: Option<usize>,
    ) -> Result<Vec<serde_json::Value>> {
        // 实现高级消息查询（按类型、时间范围过滤）
        use crate::domain::repository::{Query, QueryResult};
        
        let query = Query::FindMessages {
            conversation_id,
            message_type: message_type.map(|t| format!("{:?}", t)),
            start_time,
            end_time,
            limit,
        };
        
        let result = self.read_store.query(query).await?;
        
        match result {
            QueryResult::FindMessages { items } => Ok(items),
            _ => Err(anyhow::anyhow!("Unexpected query result type")),
        }
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
        // 直接写入 ReadStore（不经过 EventStore）
        // 用于离线消息同步场景，避免重复的事件投影
        self.read_store.write_message(&message).await?;
        
        // 更新或创建会话
        use crate::domain::conversation::Conversation;
        use crate::domain::service::ConversationDomainService;
        use crate::domain::repository::{Query, QueryResult};
        
        let query = Query::ConversationDetail {
            conversation_id: message.conversation_id.clone(),
        };
        
        let mut conversation = match self.read_store.query(query).await? {
            QueryResult::ConversationDetail { item } => {
                if !item.is_null() && item.get("conversation_id").is_some() {
                    serde_json::from_value::<Conversation>(item).ok()
                } else {
                    None
                }
            }
            _ => None,
        };
        
        if let Some(ref mut conv) = conversation {
            // 更新现有会话
            let domain_service = ConversationDomainService::new();
            domain_service.update_last_message(conv, &message)?;
            self.read_store.write_conversation(conv).await?;
        } else {
            // 创建新会话
            let conv = ConversationDomainService::new().create_conversation_from_message(&message)?;
            self.read_store.write_conversation(&conv).await?;
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
        user_id: String,
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
            user_id,
            state_type,
        }).await
    }
    
}
