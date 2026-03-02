use crate::domain::message::{
    Message, MessageOperation, OperationType, OperationData,
    DeleteType, ReactionAction, MarkType, MessageType, ContentType,
};
use anyhow::Result;
use chrono::Utc;
use flare_proto::MessageContentExt;

pub struct MessageDomainService;

impl MessageDomainService {
    pub fn new() -> Self {
        Self
    }
    
    pub fn apply_operation(
        &self,
        operation: MessageOperation,
        message: &mut Message,
    ) -> Result<()> {
        match operation.operation_data {
            OperationData::Recall { reason, .. } => {
                message.recall(operation.operator_id, reason)?;
            }
            OperationData::Edit { new_content, .. } => {
                message.edit(new_content, operation.operator_id.clone(), None)?;
            }
            OperationData::Delete { delete_type, .. } => {
                let now = chrono::Utc::now();
                let deleted_marker = match delete_type {
                    DeleteType::Soft => "true",
                    DeleteType::Hard => "hard",
                };
                message.version += 1;
                message.updated_at = now;
                message.extra.insert("deleted".to_string(), deleted_marker.to_string());
                message.extra.insert("deleted_at".to_string(), now.to_rfc3339());
            }
            OperationData::Read { message_ids, .. } => {
                if let Some(server_id) = &message.server_id {
                    if message_ids.contains(server_id) {
                        message.mark_read(operation.operator_id.clone())?;
                    }
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
            _ => return Err(anyhow::anyhow!("Operation not implemented")),
        }
        Ok(())
    }
    
    pub fn can_recall(
        &self,
        message: &Message,
        operator_id: &str,
        time_limit_seconds: Option<i32>,
    ) -> Result<bool> {
        if message.is_recalled || message.sender_id != operator_id {
            return Ok(false);
        }
        
        if let Some(limit) = time_limit_seconds {
            let elapsed = (Utc::now() - message.timestamp).num_seconds();
            if elapsed > limit as i64 {
                return Ok(false);
            }
        }
        
        Ok(true)
    }
    
    pub fn can_edit(&self, message: &Message, editor_id: &str) -> Result<bool> {
        Ok(!message.is_recalled 
            && message.sender_id == editor_id 
            && message.message_type == MessageType::Text)
    }
    
    pub fn can_delete(
        &self,
        message: &Message,
        operator_id: &str,
        _delete_type: DeleteType,
    ) -> Result<bool> {
        Ok(message.sender_id == operator_id)
    }
    
    /// 验证消息是否可以添加反应
    pub fn can_add_reaction(
        &self,
        message: &Message,
        _user_id: &str,
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
        _operator_id: &str,
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
        _operator_id: &str,
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
        _operator_id: &str,
    ) -> Result<bool> {
        // 检查消息是否已被撤回
        if message.is_recalled {
            return Ok(false);
        }
        
        Ok(true)
    }
    
    pub fn execute_recall_operation(
        &self,
        message: &mut Message,
        operator_id: String,
        reason: Option<String>,
        time_limit_seconds: Option<i32>,
    ) -> Result<MessageOperation> {
        if !self.can_recall(message, &operator_id, time_limit_seconds)? {
            return Err(anyhow::anyhow!("Message cannot be recalled"));
        }
        
        message.recall(operator_id.clone(), reason.clone())?;
        
        Ok(MessageOperation {
            operation_type: OperationType::Recall,
            target_message_id: message.server_id.clone().unwrap_or_default(),
            operator_id,
            timestamp: Utc::now(),
            show_notice: true,
            notice_text: Some("消息已撤回".to_string()),
            target_user_id: None,
            operation_data: OperationData::Recall {
                reason,
                time_limit_seconds,
                allow_admin_recall: false,
            },
            metadata: std::collections::HashMap::new(),
        })
    }
    
    pub fn execute_edit_operation(
        &self,
        message: &mut Message,
        editor_id: String,
        new_content: Vec<u8>,
        reason: Option<String>,
    ) -> Result<MessageOperation> {
        if !self.can_edit(message, &editor_id)? {
            return Err(anyhow::anyhow!("Message cannot be edited"));
        }
        
        let edit_version = message.edit_history.len() as i32 + 1;
        message.edit(new_content.clone(), editor_id.clone(), reason.clone())?;
        
        Ok(MessageOperation {
            operation_type: OperationType::Edit,
            target_message_id: message.server_id.clone().unwrap_or_default(),
            operator_id: editor_id,
            timestamp: Utc::now(),
            show_notice: true,
            notice_text: Some("消息已编辑".to_string()),
            target_user_id: None,
            operation_data: OperationData::Edit {
                new_content,
                edit_version,
                reason,
                show_edited_mark: true,
            },
            metadata: std::collections::HashMap::new(),
        })
    }
    
    pub fn execute_delete_operation(
        &self,
        message: &mut Message,
        operator_id: String,
        delete_type: DeleteType,
        reason: Option<String>,
    ) -> Result<MessageOperation> {
        if !self.can_delete(message, &operator_id, delete_type)? {
            return Err(anyhow::anyhow!("Message cannot be deleted"));
        }
        
        let now = Utc::now();
        match delete_type {
            DeleteType::Soft => {
                message.version += 1;
                message.updated_at = now;
                message.extra.insert("deleted".to_string(), "true".to_string());
                message.extra.insert("deleted_at".to_string(), now.to_rfc3339());
            }
            DeleteType::Hard => {
                message.version += 1;
                message.updated_at = Utc::now();
                message.extra.insert("deleted".to_string(), "hard".to_string());
                message.extra.insert("deleted_at".to_string(), Utc::now().to_rfc3339());
            }
        }
        
        // 3. 构建 MessageOperation
        Ok(MessageOperation {
            operation_type: OperationType::Delete,
            target_message_id: message.server_id.clone().unwrap_or_default(),
            operator_id,
            timestamp: Utc::now(),
            show_notice: false,
            notice_text: None,
            target_user_id: None,
            operation_data: OperationData::Delete {
                delete_type,
                reason,
                notify_others: false,
            },
            metadata: std::collections::HashMap::new(),
        })
    }
    
    /// 执行添加反应操作（完整流程：验证 -> 应用 -> 构建操作数据）
    pub fn execute_add_reaction_operation(
        &self,
        message: &mut Message,
        user_id: String,
        emoji: String,
    ) -> Result<MessageOperation> {
        // 1. 验证
        if !self.can_add_reaction(message, &user_id)? {
            return Err(anyhow::anyhow!("Cannot add reaction to this message"));
        }
        
        // 2. 应用操作
        message.add_reaction(emoji.clone(), user_id.clone());
        
        // 3. 构建 MessageOperation
        let count = message.reactions.iter()
            .find(|r| r.emoji == emoji)
            .map(|r| r.count)
            .unwrap_or(1);
        
        Ok(MessageOperation {
            operation_type: OperationType::ReactionAdd,
            target_message_id: message.server_id.clone().unwrap_or_default(),
            operator_id: user_id,
            timestamp: Utc::now(),
            show_notice: false,
            notice_text: None,
            target_user_id: None,
            operation_data: OperationData::Reaction {
                emoji,
                action: ReactionAction::Add,
                count,
            },
            metadata: std::collections::HashMap::new(),
        })
    }
    
    /// 执行移除反应操作（完整流程：验证 -> 应用 -> 构建操作数据）
    pub fn execute_remove_reaction_operation(
        &self,
        message: &mut Message,
        user_id: String,
        emoji: String,
    ) -> Result<MessageOperation> {
        // 1. 应用操作（移除反应不需要验证，任何人都可以移除自己的反应）
        message.remove_reaction(emoji.clone(), user_id.clone());
        
        // 2. 构建 MessageOperation
        let count = message.reactions.iter()
            .find(|r| r.emoji == emoji)
            .map(|r| r.count)
            .unwrap_or(0);
        
        Ok(MessageOperation {
            operation_type: OperationType::ReactionRemove,
            target_message_id: message.server_id.clone().unwrap_or_default(),
            operator_id: user_id,
            timestamp: Utc::now(),
            show_notice: false,
            notice_text: None,
            target_user_id: None,
            operation_data: OperationData::Reaction {
                emoji,
                action: ReactionAction::Remove,
                count,
            },
            metadata: std::collections::HashMap::new(),
        })
    }
    
    /// 执行置顶操作（完整流程：验证 -> 应用 -> 构建操作数据）
    pub fn execute_pin_operation(
        &self,
        message: &mut Message,
        operator_id: String,
        reason: Option<String>,
        expire_at: Option<chrono::DateTime<Utc>>,
    ) -> Result<MessageOperation> {
        // 1. 验证
        if !self.can_pin(message, &operator_id)? {
            return Err(anyhow::anyhow!("Cannot pin this message"));
        }
        
        // 2. 应用操作
        message.extra.insert("is_pinned".to_string(), "true".to_string());
        if let Some(reason) = &reason {
            message.extra.insert("pin_reason".to_string(), reason.clone());
        }
        if let Some(expire) = expire_at {
            message.extra.insert("pin_expire_at".to_string(), expire.to_rfc3339());
        }
        message.version += 1;
        message.updated_at = Utc::now();
        
        // 3. 构建 MessageOperation
        Ok(MessageOperation {
            operation_type: OperationType::Pin,
            target_message_id: message.server_id.clone().unwrap_or_default(),
            operator_id,
            timestamp: Utc::now(),
            show_notice: true,
            notice_text: Some("消息已置顶".to_string()),
            target_user_id: None,
            operation_data: OperationData::Pin {
                reason,
                expire_at,
            },
            metadata: std::collections::HashMap::new(),
        })
    }
    
    /// 执行取消置顶操作（完整流程：应用 -> 构建操作数据）
    pub fn execute_unpin_operation(
        &self,
        message: &mut Message,
        operator_id: String,
    ) -> Result<MessageOperation> {
        // 1. 应用操作
        message.extra.remove("is_pinned");
        message.extra.remove("pin_reason");
        message.extra.remove("pin_expire_at");
        message.version += 1;
        message.updated_at = Utc::now();
        
        // 2. 构建 MessageOperation
        Ok(MessageOperation {
            operation_type: OperationType::Unpin,
            target_message_id: message.server_id.clone().unwrap_or_default(),
            operator_id,
            timestamp: Utc::now(),
            show_notice: true,
            notice_text: Some("消息已取消置顶".to_string()),
            target_user_id: None,
            operation_data: OperationData::Pin {
                reason: None,
                expire_at: None,
            },
            metadata: std::collections::HashMap::new(),
        })
    }
    
    /// 执行标记操作（完整流程：验证 -> 应用 -> 构建操作数据）
    pub fn execute_mark_operation(
        &self,
        message: &mut Message,
        operator_id: String,
        mark_type: MarkType,
        color: Option<String>,
    ) -> Result<MessageOperation> {
        // 1. 验证
        if !self.can_mark(message, &operator_id)? {
            return Err(anyhow::anyhow!("Cannot mark this message"));
        }
        
        // 2. 应用操作
        message.attributes.insert(
            "mark_type".to_string(),
            format!("{:?}", mark_type),
        );
        message.attributes.insert("marked_at".to_string(), Utc::now().to_rfc3339());
        if let Some(color) = &color {
            message.attributes.insert("mark_color".to_string(), color.clone());
        }
        message.version += 1;
        message.updated_at = Utc::now();
        
        // 3. 构建 MessageOperation
        Ok(MessageOperation {
            operation_type: OperationType::Mark,
            target_message_id: message.server_id.clone().unwrap_or_default(),
            operator_id,
            timestamp: Utc::now(),
            show_notice: false,
            notice_text: None,
            target_user_id: None,
            operation_data: OperationData::Mark {
                mark_type,
                color,
            },
            metadata: std::collections::HashMap::new(),
        })
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

        
        // 检查会话ID（发送时必须有 conversation_id）
        if message.conversation_id.is_none() || message.conversation_id.as_ref().unwrap().is_empty() {
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
            MessageType::Operation => {
                // 操作消息预览：根据操作类型生成预览文本
                "[操作消息]".to_string()
            }
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
        // 使用统一的解码方法（高性能、一致性）
        let message_content = flare_proto::decode_message_content(content)?;
        
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
        conversation_id: Option<String>,
        sender_id: String,
        text: String,
        mentions: Vec<MentionInfo>,
    ) -> Result<Message> {
        use uuid::Uuid;
        use crate::domain::message::{MessageBuilder, MessageType};
        
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
                start: mention.start,
                length: mention.length,
            };
            proto_mentions.push(proto_mention);
        }
        
        let text_content = TextContent {
            text: text.clone(),
            mentions: proto_mentions,
        };
        let mut content = MessageContent::default();
        content.content = Some(Content::Text(text_content));
        
        let buf = content.encode_to_bytes()
            .map_err(|e| anyhow::anyhow!("Failed to encode MessageContent: {}", e))?;
        
        let mut builder = MessageBuilder::new()
            .with_server_id(Uuid::new_v4().to_string())
            .with_client_msg_id(Uuid::new_v4().to_string())
            .with_sender_id(sender_id)
            .with_message_type(MessageType::Text);
        
        // 如果提供了 conversation_id，则设置
        if let Some(conv_id) = conversation_id {
            builder = builder.with_conversation_id(conv_id);
        }
        
        builder
            .with_content(buf)
            .with_content_type(ContentType::PlainText)
            .build()
    }
    
    /// 创建合并消息
    pub fn create_merge_message(
        &self,
        conversation_id: Option<String>,
        sender_id: String,
        message_ids: Vec<String>,
    ) -> Result<Message> {
        use uuid::Uuid;
        use crate::domain::message::{MessageBuilder, MessageType};
        
        // 构建合并转发内容
        use flare_proto::flare::common::v1::{MessageContent, message_content::Content, ForwardContent};
        let forward_content = ForwardContent {
            message_ids: message_ids.clone(),
            forward_reason: String::new(),
            forwarded_previews: Vec::new(), // 转发预览列表（可选）
        };
        let mut content = MessageContent::default();
        content.content = Some(Content::Forward(forward_content));
        
        let buf = content.encode_to_bytes()
            .map_err(|e| anyhow::anyhow!("Failed to encode MessageContent: {}", e))?;
        
        let mut builder = MessageBuilder::new()
            .with_server_id(Uuid::new_v4().to_string())
            .with_client_msg_id(Uuid::new_v4().to_string())
            .with_sender_id(sender_id)
            .with_message_type(MessageType::Custom)
            .with_business_type("merge_forward".to_string());
        
        // 如果提供了 conversation_id，则设置
        if let Some(conv_id) = conversation_id {
            builder = builder.with_conversation_id(conv_id);
        }
        
        builder
            .with_content(buf)
            .build()
    }
    
    /// 创建转发消息
    pub fn create_forward_message(
        &self,
        conversation_id: Option<String>,
        sender_id: String,
        message_ids: Vec<String>,
        forward_reason: Option<String>,
    ) -> Result<Message> {
        use uuid::Uuid;
        use crate::domain::message::{MessageBuilder, MessageType};
        
        use flare_proto::flare::common::v1::{MessageContent, message_content::Content, ForwardContent};
        let forward_content = ForwardContent {
            message_ids: message_ids.clone(),
            forward_reason: forward_reason.unwrap_or_default(),
            forwarded_previews: Vec::new(), // 转发预览列表（可选）
        };
        let mut content = MessageContent::default();
        content.content = Some(Content::Forward(forward_content));
        
        let buf = content.encode_to_bytes()
            .map_err(|e| anyhow::anyhow!("Failed to encode MessageContent: {}", e))?;
        
        let mut builder = MessageBuilder::new()
            .with_server_id(Uuid::new_v4().to_string())
            .with_client_msg_id(Uuid::new_v4().to_string())
            .with_sender_id(sender_id)
            .with_message_type(MessageType::Custom)
            .with_business_type("forward".to_string());
        
        // 如果提供了 conversation_id，则设置
        if let Some(conv_id) = conversation_id {
            builder = builder.with_conversation_id(conv_id);
        }
        
        builder
            .with_content(buf)
            .build()
    }
    
    /// 创建定位消息
    pub fn create_location_message(
        &self,
        conversation_id: Option<String>,
        sender_id: String,
        longitude: f64,
        latitude: f64,
        address: String,
        description: Option<String>,
        poi_id: Option<String>,
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
        
        let buf = content.encode_to_bytes()
            .map_err(|e| anyhow::anyhow!("Failed to encode MessageContent: {}", e))?;
        
        let mut builder = MessageBuilder::new()
            .with_server_id(Uuid::new_v4().to_string())
            .with_client_msg_id(Uuid::new_v4().to_string())
            .with_sender_id(sender_id)
            .with_message_type(MessageType::Location);
        
        // 如果提供了 conversation_id，则设置
        if let Some(conv_id) = conversation_id {
            builder = builder.with_conversation_id(conv_id);
        }
        
        builder
            .with_content(buf)
            .build()
    }
    
    /// 创建引用消息（使用 quote 字段）
    pub fn create_quote_message(
        &self,
        conversation_id: Option<String>,
        sender_id: String,
        quoted_message_id: String,
        quoted_sender_id: Option<String>,
        quoted_text_preview: Option<String>,
        reply_content: Vec<u8>,
    ) -> Result<Message> {
        use crate::domain::message::{MessageBuilder, MessageType, QuoteContent};
        use uuid::Uuid;
        
        let quote = QuoteContent {
            quoted_message_id: quoted_message_id.clone(),
            quoted_sender_id: quoted_sender_id.unwrap_or_default(),
            quoted_text_preview: quoted_text_preview.unwrap_or_default(),
            quoted_content: None,
        };
        
        let mut builder = MessageBuilder::new()
            .with_server_id(Uuid::new_v4().to_string())
            .with_client_msg_id(Uuid::new_v4().to_string())
            .with_sender_id(sender_id)
            .with_message_type(MessageType::Text);
        
        // 如果提供了 conversation_id，则设置
        if let Some(conv_id) = conversation_id {
            builder = builder.with_conversation_id(conv_id);
        }
        
        builder
            .with_content(reply_content)
            .with_quote(quote)
            .build()
    }
    
    /// 创建名片消息
    pub fn create_card_message(
        &self,
        conversation_id: Option<String>,
        sender_id: String,
        user_id: String,
        nickname: String,
        avatar_url: String,
        description: Option<String>,
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
        
        let buf = content.encode_to_bytes()
            .map_err(|e| anyhow::anyhow!("Failed to encode MessageContent: {}", e))?;
        
        let mut builder = MessageBuilder::new()
            .with_server_id(Uuid::new_v4().to_string())
            .with_client_msg_id(Uuid::new_v4().to_string())
            .with_sender_id(sender_id)
            .with_message_type(MessageType::Card);
        
        // 如果提供了 conversation_id，则设置
        if let Some(conv_id) = conversation_id {
            builder = builder.with_conversation_id(conv_id);
        }
        
        builder
            .with_content(buf)
            .build()
    }
    
    /// 创建自定义消息
    pub fn create_custom_message(
        &self,
        conversation_id: Option<String>,
        sender_id: String,
        custom_type: String,
        payload: Vec<u8>,
        description: Option<String>,
        metadata: Option<std::collections::HashMap<String, String>>,
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
        };
        let mut content = MessageContent::default();
        content.content = Some(Content::Custom(custom_content));
        
        let buf = content.encode_to_bytes()
            .map_err(|e| anyhow::anyhow!("Failed to encode MessageContent: {}", e))?;
        
        let mut builder = MessageBuilder::new()
            .with_server_id(Uuid::new_v4().to_string())
            .with_client_msg_id(Uuid::new_v4().to_string())
            .with_sender_id(sender_id)
            .with_message_type(MessageType::Custom);
        
        // 如果提供了 conversation_id，则设置
        if let Some(conv_id) = conversation_id {
            builder = builder.with_conversation_id(conv_id);
        }
        
        builder
            .with_content(buf)
            .with_business_type(custom_type)
            .build()
    }
    
    /// 创建表情消息
    pub fn create_face_message(
        &self,
        conversation_id: Option<String>,
        sender_id: String,
        emoji: String,
    ) -> Result<Message> {
        use uuid::Uuid;
        use crate::domain::message::{MessageBuilder, MessageType};
        
        // 表情消息可以作为文本消息的特殊类型
        use flare_proto::flare::common::v1::{MessageContent, message_content::Content, TextContent};
        let text_content = TextContent {
            text: emoji.clone(),
            mentions: Vec::new(),
        };
        let mut content = MessageContent::default();
        content.content = Some(Content::Text(text_content));
        
        let buf = content.encode_to_bytes()
            .map_err(|e| anyhow::anyhow!("Failed to encode MessageContent: {}", e))?;
        
        MessageBuilder::new()
            .with_server_id(Uuid::new_v4().to_string())
            .with_client_msg_id(Uuid::new_v4().to_string())
            .with_conversation_id(conversation_id)
            .with_sender_id(sender_id)
            .with_message_type(MessageType::Text)
            .with_content(buf)
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
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
