//! 消息领域服务
//!
//! 职责：包含所有消息相关的业务逻辑
//! 无状态，不依赖基础设施层

use crate::domain::message::{
    Message, MessageOperation, MessageOperationHandler, OperationType, OperationData,
    DeleteType, ReactionAction, MarkType, MessageType, TenantContext,
};
use anyhow::Result;
use chrono::Utc;

/// 消息领域服务
///
/// 包含所有消息相关的业务逻辑
pub struct MessageDomainService;

impl MessageDomainService {
    /// 创建新的消息领域服务实例
    pub fn new() -> Self {
        Self
    }
    
    /// 应用消息操作（撤回、编辑、删除等）
    ///
    /// 注意：这是一个同步方法，直接调用 Message 的方法
    pub fn apply_operation(
        &self,
        operation: MessageOperation,
        message: &mut Message,
    ) -> Result<()> {
        // 直接调用 Message 的方法，不通过 MessageOperationHandler
        // 因为 MessageOperationHandler::execute 是 async 的，不适合在领域服务中使用
        match operation.operation_data {
            OperationData::Recall { reason, .. } => {
                message.recall(operation.operator_id, reason)?;
            }
            OperationData::Edit { new_content, .. } => {
                message.edit(new_content, operation.operator_id.clone(), None)?;
            }
            OperationData::Delete { delete_type, .. } => {
                // 实现删除逻辑
                match delete_type {
                    DeleteType::Soft => {
                        // 软删除：标记为已删除，但保留数据
                        message.version += 1;
                        message.updated_at = chrono::Utc::now();
                        // 在 extra 中标记为已删除
                        message.extra.insert("deleted".to_string(), "true".to_string());
                        message.extra.insert("deleted_at".to_string(), chrono::Utc::now().to_rfc3339());
                    }
                    DeleteType::Hard => {
                        // 硬删除：标记为已删除，后续可以物理删除
                        message.version += 1;
                        message.updated_at = chrono::Utc::now();
                        message.extra.insert("deleted".to_string(), "hard".to_string());
                        message.extra.insert("deleted_at".to_string(), chrono::Utc::now().to_rfc3339());
                    }
                }
            }
            OperationData::Read { message_ids, .. } => {
                if message_ids.contains(&message.id) {
                    message.mark_read(operation.operator_id.clone())?;
                }
            }
            OperationData::Reaction { emoji, action, .. } => {
                match action {
                    ReactionAction::Add => {
                        message.add_reaction(emoji, operation.operator_id.clone());
                    }
                    ReactionAction::Remove => {
                        message.remove_reaction(emoji, operation.operator_id.clone());
                    }
                }
            }
            _ => {
                // 其他操作暂未实现
                return Err(anyhow::anyhow!("Operation not implemented yet"));
            }
        }
        Ok(())
    }
    
    /// 验证消息是否可以撤回
    pub fn can_recall(
        &self,
        message: &Message,
        operator_id: &str,
        time_limit_seconds: Option<i32>,
    ) -> Result<bool> {
        // 检查消息是否已被撤回
        if message.is_recalled {
            return Ok(false);
        }
        
        // 检查操作者是否有权限（只有发送者可以撤回）
        if message.sender_id != operator_id {
            return Ok(false);
        }
        
        // 检查时间限制
        if let Some(limit) = time_limit_seconds {
            let elapsed = (Utc::now() - message.timestamp).num_seconds();
            if elapsed > limit as i64 {
                return Ok(false);
            }
        }
        
        Ok(true)
    }
    
    /// 验证消息是否可以编辑
    pub fn can_edit(
        &self,
        message: &Message,
        editor_id: &str,
    ) -> Result<bool> {
        // 检查消息是否已被撤回
        if message.is_recalled {
            return Ok(false);
        }
        
        // 检查操作者是否有权限（只有发送者可以编辑）
        if message.sender_id != editor_id {
            return Ok(false);
        }
        
        // 检查消息类型是否支持编辑（文本消息可以编辑）
        if message.message_type != MessageType::Text {
            return Ok(false);
        }
        
        Ok(true)
    }
    
    /// 验证消息是否可以删除
    pub fn can_delete(
        &self,
        message: &Message,
        operator_id: &str,
        delete_type: DeleteType,
    ) -> Result<bool> {
        match delete_type {
            DeleteType::Soft => {
                // 软删除：只有发送者可以删除
                Ok(message.sender_id == operator_id)
            }
            DeleteType::Hard => {
                // 硬删除：发送者或管理员可以删除
                // TODO: 检查管理员权限
                Ok(message.sender_id == operator_id)
            }
        }
    }
    
    /// 验证消息是否可以添加反应
    pub fn can_add_reaction(
        &self,
        message: &Message,
        user_id: &str,
    ) -> Result<bool> {
        // 检查消息是否已被撤回
        if message.is_recalled {
            return Ok(false);
        }
        
        // 任何人都可以添加反应
        Ok(true)
    }
    
    /// 验证消息是否可以置顶
    pub fn can_pin(
        &self,
        message: &Message,
        operator_id: &str,
    ) -> Result<bool> {
        // 检查消息是否已被撤回
        if message.is_recalled {
            return Ok(false);
        }
        
        // TODO: 检查操作者是否有权限（管理员或群主）
        // 暂时允许所有人
        Ok(true)
    }
    
    /// 验证消息是否可以收藏
    pub fn can_favorite(
        &self,
        message: &Message,
        operator_id: &str,
    ) -> Result<bool> {
        // 检查消息是否已被撤回
        if message.is_recalled {
            return Ok(false);
        }
        
        // 任何人都可以收藏自己的消息
        Ok(true)
    }
    
    /// 验证消息是否可以标记
    pub fn can_mark(
        &self,
        message: &Message,
        operator_id: &str,
    ) -> Result<bool> {
        // 检查消息是否已被撤回
        if message.is_recalled {
            return Ok(false);
        }
        
        // 任何人都可以标记消息
        Ok(true)
    }
    
    /// 计算消息的过期时间（用于阅后即焚）
    pub fn calculate_expire_at(
        &self,
        message: &Message,
        burn_after_seconds: i32,
    ) -> chrono::DateTime<Utc> {
        message.timestamp + chrono::Duration::seconds(burn_after_seconds as i64)
    }
    
    /// 验证消息内容是否有效
    pub fn validate_message_content(
        &self,
        message: &Message,
    ) -> Result<()> {
        // 检查消息ID
        if message.id.is_empty() {
            return Err(anyhow::anyhow!("Message ID cannot be empty"));
        }
        
        // 检查会话ID
        if message.conversation_id.is_empty() {
            return Err(anyhow::anyhow!("Conversation ID cannot be empty"));
        }
        
        // 检查发送者ID
        if message.sender_id.is_empty() {
            return Err(anyhow::anyhow!("Sender ID cannot be empty"));
        }
        
        // 检查消息内容
        if message.content.is_empty() {
            return Err(anyhow::anyhow!("Message content cannot be empty"));
        }
        
        Ok(())
    }
    
    /// 生成消息预览文本
    pub fn generate_preview(
        &self,
        message: &Message,
    ) -> String {
        match message.message_type {
            MessageType::Text => {
                // 尝试解析文本内容
                if let Ok(text_content) = self.parse_text_content(&message.content) {
                    // 限制预览长度
                    let max_preview_len = 50;
                    if text_content.len() > max_preview_len {
                        format!("{}...", &text_content[..max_preview_len])
                    } else {
                        text_content
                    }
                } else {
                    "[文本消息]".to_string()
                }
            }
            MessageType::Image => {
                // 尝试从附件获取文件名
                if let Some(attachment) = message.attachments.first() {
                    if let Some(file_name) = attachment.metadata.get("file_name") {
                        format!("[图片] {}", file_name)
                    } else {
                        "[图片]".to_string()
                    }
                } else {
                    "[图片]".to_string()
                }
            }
            MessageType::Video => {
                if let Some(attachment) = message.attachments.first() {
                    if let Some(file_name) = attachment.metadata.get("file_name") {
                        format!("[视频] {}", file_name)
                    } else {
                        "[视频]".to_string()
                    }
                } else {
                    "[视频]".to_string()
                }
            }
            MessageType::Audio => {
                if let Some(attachment) = message.attachments.first() {
                    if let Some(duration) = attachment.metadata.get("duration_ms") {
                        format!("[语音] {}ms", duration)
                    } else {
                        "[语音]".to_string()
                    }
                } else {
                    "[语音]".to_string()
                }
            }
            MessageType::File => {
                if let Some(attachment) = message.attachments.first() {
                    if let Some(file_name) = attachment.metadata.get("file_name") {
                        format!("[文件] {}", file_name)
                    } else {
                        "[文件]".to_string()
                    }
                } else {
                    "[文件]".to_string()
                }
            }
            MessageType::Location => "[位置]".to_string(),
            MessageType::Card => "[名片]".to_string(),
            MessageType::Custom => {
                if let Some(business_type) = &message.business_type {
                    format!("[{}]", business_type)
                } else {
                    "[自定义消息]".to_string()
                }
            }
            MessageType::Notification => "[通知]".to_string(),
        }
    }
    
    /// 解析文本内容
    fn parse_text_content(&self, content: &[u8]) -> Result<String> {
        use flare_proto::flare::common::v1::MessageContent;
        use prost::Message as ProstMessage;
        let message_content = MessageContent::decode(content)?;
        
        if let Some(flare_proto::flare::common::v1::message_content::Content::Text(text_content)) = message_content.content {
            Ok(text_content.text)
        } else {
            Err(anyhow::anyhow!("Not a text message"))
        }
    }
    
    // ============================================================================
    // 消息创建方法（从 Facade 移到这里）
    // ============================================================================
    
    /// 创建@消息
    pub fn create_text_at_message(
        &self,
        conversation_id: String,
        sender_id: String,
        text: String,
        mentions: Vec<MentionInfo>,
        tenant: TenantContext,
    ) -> Result<Message> {
        use uuid::Uuid;
        use crate::domain::message::{MessageBuilder, MessageType, ContentType};
        
        // 构建@提及列表
        use flare_proto::flare::common::v1::{MessageContent, message_content::Content, TextContent, Mention};
        let mut proto_mentions = Vec::new();
        
        for mention in mentions {
            let proto_mention = Mention {
                r#type: match mention.mention_type {
                    MentionInfoType::User => 1, // MENTION_TYPE_USER
                    MentionInfoType::All => 2,  // MENTION_TYPE_ALL
                    MentionInfoType::Role => 3,  // MENTION_TYPE_ROLE
                    MentionInfoType::Multi => 4, // MENTION_TYPE_MULTI
                },
                user_id: mention.user_id.unwrap_or_default(),
                user_ids: mention.user_ids.unwrap_or_default(),
                role_id: mention.role_id.unwrap_or_default(),
                role_name: mention.role_name.unwrap_or_default(),
                start: mention.start,
                length: mention.length,
                metadata: mention.metadata.unwrap_or_default(),
            };
            proto_mentions.push(proto_mention);
        }
        
        let text_content = TextContent {
            text: text.clone(),
            mentions: proto_mentions,
        };
        let mut content = MessageContent::default();
        content.content = Some(Content::Text(text_content));
        
        let mut buf = Vec::new();
        prost::Message::encode(&content, &mut buf)?;
        
        MessageBuilder::new()
            .with_id(Uuid::new_v4().to_string())
            .with_client_msg_id(Uuid::new_v4().to_string())
            .with_conversation_id(conversation_id)
            .with_sender_id(sender_id)
            .with_message_type(MessageType::Text)
            .with_content(buf)
            .with_content_type(ContentType::PlainText)
            .with_tenant(tenant)
            .build()
    }
    
    /// 创建合并消息
    pub fn create_merge_message(
        &self,
        conversation_id: String,
        sender_id: String,
        message_ids: Vec<String>,
        tenant: TenantContext,
    ) -> Result<Message> {
        use uuid::Uuid;
        use crate::domain::message::{MessageBuilder, MessageType};
        
        // 构建合并转发内容
        use flare_proto::flare::common::v1::{MessageContent, message_content::Content, ForwardContent};
        let forward_content = ForwardContent {
            message_ids: message_ids.clone(),
            forward_reason: String::new(),
        };
        let mut content = MessageContent::default();
        content.content = Some(Content::Forward(forward_content));
        
        let mut buf = Vec::new();
        prost::Message::encode(&content, &mut buf)?;
        
        MessageBuilder::new()
            .with_id(Uuid::new_v4().to_string())
            .with_client_msg_id(Uuid::new_v4().to_string())
            .with_conversation_id(conversation_id)
            .with_sender_id(sender_id)
            .with_message_type(MessageType::Custom)
            .with_business_type("merge_forward".to_string())
            .with_content(buf)
            .with_tenant(tenant)
            .build()
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
        use uuid::Uuid;
        use crate::domain::message::{MessageBuilder, MessageType};
        
        use flare_proto::flare::common::v1::{MessageContent, message_content::Content, ForwardContent};
        let forward_content = ForwardContent {
            message_ids: message_ids.clone(),
            forward_reason: forward_reason.unwrap_or_default(),
        };
        let mut content = MessageContent::default();
        content.content = Some(Content::Forward(forward_content));
        
        let mut buf = Vec::new();
        prost::Message::encode(&content, &mut buf)?;
        
        MessageBuilder::new()
            .with_id(Uuid::new_v4().to_string())
            .with_client_msg_id(Uuid::new_v4().to_string())
            .with_conversation_id(conversation_id)
            .with_sender_id(sender_id)
            .with_message_type(MessageType::Custom)
            .with_business_type("forward".to_string())
            .with_content(buf)
            .with_tenant(tenant)
            .build()
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
        use uuid::Uuid;
        use crate::domain::message::{MessageBuilder, MessageType};
        
        // 构建位置内容
        use flare_proto::flare::common::v1::{MessageContent, message_content::Content, LocationContent};
        let location_content = LocationContent {
            longitude,
            latitude,
            address: address.clone(),
            description: description.unwrap_or_default(),
            poi_id: poi_id.unwrap_or_default(),
        };
        let mut content = MessageContent::default();
        content.content = Some(Content::Location(location_content));
        
        let mut buf = Vec::new();
        prost::Message::encode(&content, &mut buf)?;
        
        MessageBuilder::new()
            .with_id(Uuid::new_v4().to_string())
            .with_client_msg_id(Uuid::new_v4().to_string())
            .with_conversation_id(conversation_id)
            .with_sender_id(sender_id)
            .with_message_type(MessageType::Location)
            .with_content(buf)
            .with_tenant(tenant)
            .build()
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
        use uuid::Uuid;
        use crate::domain::message::{MessageBuilder, MessageType};
        
        // 构建引用内容
        use flare_proto::flare::common::v1::{MessageContent, message_content::Content, QuoteContent};
        let quote_content = QuoteContent {
            quoted_message_id: quoted_message_id.clone(),
            quoted_sender_id: quoted_sender_id.clone(),
            quoted_text_preview: quoted_text_preview.clone(),
            quoted_content: None,
        };
        let mut content = MessageContent::default();
        content.content = Some(Content::Quote(Box::new(quote_content)));
        
        let mut buf = Vec::new();
        prost::Message::encode(&content, &mut buf)?;
        
        MessageBuilder::new()
            .with_id(Uuid::new_v4().to_string())
            .with_client_msg_id(Uuid::new_v4().to_string())
            .with_conversation_id(conversation_id)
            .with_sender_id(sender_id)
            .with_message_type(MessageType::Text)
            .with_content(reply_content)
            .with_tenant(tenant)
            .with_extra("is_quote".to_string(), "true".to_string())
            .with_extra("reply_to_message_id".to_string(), quoted_message_id)
            .build()
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
        use uuid::Uuid;
        use crate::domain::message::{MessageBuilder, MessageType};
        
        // 构建名片内容
        use flare_proto::flare::common::v1::{MessageContent, message_content::Content, CardContent};
        let card_content = CardContent {
            user_id: user_id.clone(),
            nickname: nickname.clone(),
            avatar_url: avatar_url.clone(),
            description: description.unwrap_or_default(),
            extra: std::collections::HashMap::new(),
        };
        let mut content = MessageContent::default();
        content.content = Some(Content::Card(card_content));
        
        let mut buf = Vec::new();
        prost::Message::encode(&content, &mut buf)?;
        
        MessageBuilder::new()
            .with_id(Uuid::new_v4().to_string())
            .with_client_msg_id(Uuid::new_v4().to_string())
            .with_conversation_id(conversation_id)
            .with_sender_id(sender_id)
            .with_message_type(MessageType::Card)
            .with_content(buf)
            .with_tenant(tenant)
            .build()
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
        use uuid::Uuid;
        use crate::domain::message::{MessageBuilder, MessageType};
        
        // 构建自定义内容
        use flare_proto::flare::common::v1::{MessageContent, message_content::Content, CustomContent};
        let custom_content = CustomContent {
            r#type: custom_type.clone(),
            payload: payload.clone(),
            description: description.unwrap_or_default(),
            metadata: metadata.unwrap_or_default(),
            extensions: Vec::new(),
        };
        let mut content = MessageContent::default();
        content.content = Some(Content::Custom(custom_content));
        
        let mut buf = Vec::new();
        prost::Message::encode(&content, &mut buf)?;
        
        MessageBuilder::new()
            .with_id(Uuid::new_v4().to_string())
            .with_client_msg_id(Uuid::new_v4().to_string())
            .with_conversation_id(conversation_id)
            .with_sender_id(sender_id)
            .with_message_type(MessageType::Custom)
            .with_content(buf)
            .with_business_type(custom_type)
            .with_tenant(tenant)
            .build()
    }
    
    /// 创建表情消息
    pub fn create_face_message(
        &self,
        conversation_id: String,
        sender_id: String,
        emoji: String,
        tenant: TenantContext,
    ) -> Result<Message> {
        use uuid::Uuid;
        use crate::domain::message::{MessageBuilder, MessageType, ContentType};
        
        // 表情消息可以作为文本消息的特殊类型
        use flare_proto::flare::common::v1::{MessageContent, message_content::Content, TextContent};
        let text_content = TextContent {
            text: emoji.clone(),
            mentions: Vec::new(),
        };
        let mut content = MessageContent::default();
        content.content = Some(Content::Text(text_content));
        
        let mut buf = Vec::new();
        prost::Message::encode(&content, &mut buf)?;
        
        MessageBuilder::new()
            .with_id(Uuid::new_v4().to_string())
            .with_client_msg_id(Uuid::new_v4().to_string())
            .with_conversation_id(conversation_id)
            .with_sender_id(sender_id)
            .with_message_type(MessageType::Text)
            .with_content(buf)
            .with_tenant(tenant)
            .with_extra("is_face".to_string(), "true".to_string())
            .build()
    }
    
    /// 检测文件 MIME 类型（委托给 MediaDomainService）
    pub fn detect_mime_type(&self, file_path: &str) -> Result<String> {
        use crate::domain::service::MediaDomainService;
        let media_service = MediaDomainService::new();
        media_service.detect_mime_type(file_path)
    }
}

/// @提及信息（用于 API）
#[derive(Debug, Clone)]
pub struct MentionInfo {
    pub mention_type: MentionInfoType,
    pub user_id: Option<String>,
    pub user_ids: Option<Vec<String>>,
    pub role_id: Option<String>,
    pub role_name: Option<String>,
    pub start: i32,
    pub length: i32,
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

/// @提及类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MentionInfoType {
    User,
    All,
    Role,
    Multi,
}

impl Default for MessageDomainService {
    fn default() -> Self {
        Self::new()
    }
}
