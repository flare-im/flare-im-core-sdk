//! 会话领域服务
//!
//! 职责：包含所有会话相关的业务逻辑
//! 无状态，不依赖基础设施层

use crate::domain::conversation::{Conversation, InputStateType};
use anyhow::Result;

/// 会话领域服务
///
/// 包含所有会话相关的业务逻辑
pub struct ConversationDomainService;

impl ConversationDomainService {
    /// 创建新的会话领域服务实例
    pub fn new() -> Self {
        Self
    }
    
    /// 标记会话消息为已读
    pub fn mark_as_read(
        &self,
        conversation: &mut Conversation,
        _user_id: &str,
    ) -> Result<()> {
        conversation.clear_unread();
        Ok(())
    }
    
    /// 设置会话草稿
    pub fn set_draft(
        &self,
        conversation: &mut Conversation,
        draft: Option<String>,
    ) -> Result<()> {
        // Conversation 的 draft 字段是公开的，直接设置
        conversation.draft = draft;
        Ok(())
    }
    
    /// 隐藏会话
    pub fn hide(
        &self,
        conversation: &mut Conversation,
    ) -> Result<()> {
        use crate::domain::conversation::ConversationVisibility;
        conversation.visibility = ConversationVisibility::Private; // 隐藏会话
        Ok(())
    }
    
    /// 显示会话
    pub fn show(
        &self,
        conversation: &mut Conversation,
    ) -> Result<()> {
        use crate::domain::conversation::ConversationVisibility;
        conversation.visibility = ConversationVisibility::Public; // 显示会话
        Ok(())
    }
    
    /// 删除会话
    pub fn delete(
        &self,
        conversation: &mut Conversation,
    ) -> Result<()> {
        use crate::domain::conversation::ConversationLifecycleState;
        conversation.lifecycle_state = ConversationLifecycleState::Deleted;
        Ok(())
    }
    
    /// 设置会话静音（完整流程：应用业务逻辑）
    pub fn set_muted(
        &self,
        conversation: &mut Conversation,
        muted: bool,
        mute_until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<()> {
        conversation.set_muted(muted);
        // 统一处理 mute_until（业务逻辑在领域层）
        if muted {
            conversation.mute_until = mute_until;
        } else {
            conversation.mute_until = None;
        }
        conversation.version += 1;
        conversation.updated_at = chrono::Utc::now();
        Ok(())
    }
    
    /// 设置会话置顶
    pub fn set_pinned(
        &self,
        conversation: &mut Conversation,
        pinned: bool,
    ) -> Result<()> {
        conversation.set_pinned(pinned);
        Ok(())
    }
    
    /// 归档会话
    pub fn archive(
        &self,
        conversation: &mut Conversation,
    ) -> Result<()> {
        conversation.archive();
        Ok(())
    }
    
    /// 取消归档
    pub fn unarchive(
        &self,
        conversation: &mut Conversation,
    ) -> Result<()> {
        conversation.unarchive();
        Ok(())
    }
    
    /// 更新会话信息
    pub fn update_info(
        &self,
        conversation: &mut Conversation,
        display_name: Option<String>,
        avatar_url: Option<String>,
        description: Option<String>,
        announcement: Option<String>,
        updated_by: Option<String>,
    ) -> Result<()> {
        if let Some(name) = display_name {
            conversation.display_name = name;
            conversation.version += 1;
            conversation.updated_at = chrono::Utc::now();
        }
        if let Some(avatar) = avatar_url {
            conversation.avatar_url = Some(avatar);
            conversation.version += 1;
            conversation.updated_at = chrono::Utc::now();
        }
        if let Some(desc) = description {
            conversation.description = Some(desc);
            conversation.version += 1;
            conversation.updated_at = chrono::Utc::now();
        }
        if let Some(announce) = announcement {
            // update_announcement 需要两个参数：announcement 和 updated_by
            conversation.update_announcement(announce, updated_by.unwrap_or_else(|| "system".to_string()));
        }
        Ok(())
    }
    
    /// 清空会话消息（仅更新会话状态，不删除实际消息）
    pub fn clear_messages(
        &self,
        conversation: &mut Conversation,
    ) -> Result<()> {
        // 重置最后消息和未读数
        conversation.last_message = None;
        conversation.clear_unread();
        conversation.max_seq = 0;
        conversation.last_read_seq = 0;
        Ok(())
    }
    
    /// 设置输入状态
    pub fn set_input_state(
        &self,
        conversation: &mut Conversation,
        user_id: String,
        state_type: InputStateType,
    ) -> Result<()> {
        use crate::domain::conversation::InputState;
        let input_state = InputState {
            user_id,
            state_type,
            started_at: chrono::Utc::now(),
            duration_ms: None,
        };
        conversation.input_state = Some(input_state);
        Ok(())
    }
    
    /// 清除输入状态
    pub fn clear_input_state(
        &self,
        conversation: &mut Conversation,
    ) -> Result<()> {
        conversation.input_state = None;
        Ok(())
    }
    
    /// 验证会话是否有效
    pub fn validate(
        &self,
        conversation: &Conversation,
    ) -> Result<()> {
        if conversation.conversation_id.is_empty() {
            return Err(anyhow::anyhow!("Conversation ID cannot be empty"));
        }
        
        if conversation.conversation_type.is_empty() {
            return Err(anyhow::anyhow!("Conversation type cannot be empty"));
        }
        
        Ok(())
    }
    
    /// 计算会话排序权重（用于会话列表排序）
    ///
    /// 对标微信、Telegram、飞书的会话排序算法
    pub fn calculate_sort_weight(
        &self,
        conversation: &Conversation,
    ) -> i64 {
        let mut weight = 0i64;
        
        // 置顶会话权重最高
        if conversation.is_pinned {
            weight += 1_000_000_000;
        }
        
        // 未读数影响权重
        weight += conversation.unread_count as i64 * 1_000_000;
        
        // 最后消息时间影响权重
        if let Some(last_message) = &conversation.last_message {
            weight += last_message.time.timestamp();
        }
        
        weight
    }
    
    /// 从消息创建会话
    ///
    /// 当收到新消息时，如果会话不存在，创建新会话
    /// 对标微信、Telegram、飞书的会话自动创建机制
    pub fn create_conversation_from_message(
        &self,
        message: &crate::domain::message::Message,
    ) -> Result<Conversation> {
        use crate::domain::conversation::{ConversationVisibility, ConversationLifecycleState, MessagePreview, ConversationParticipant};
        use crate::domain::message::ConversationType;
        
        // 构建最后一条消息预览
        let last_message = MessagePreview {
            message_id: message.server_id.clone().unwrap_or_else(|| message.client_msg_id.clone()),
            sender_id: message.sender_id.clone(),
            message_type: format!("{:?}", message.message_type),
            text: self.extract_message_preview_text(message),
            time: message.timestamp,
        };
        
        // 构建参与者列表
        let mut participants = vec![];
        
        // 对于单聊，尝试从消息中提取参与者
        if message.conversation_type == ConversationType::Single {
            // 添加发送者
            participants.push(ConversationParticipant {
                user_id: message.sender_id.clone(),
                roles: vec![],
                muted: false,
                pinned: false,
                attributes: std::collections::HashMap::new(),
                joined_at: chrono::Utc::now(),
                nickname: None,
            });
            
            // 添加接收者（如果存在且不同于发送者）
            if let Some(receiver_id) = &message.receiver_id {
                if receiver_id != &message.sender_id {
                    participants.push(ConversationParticipant {
                        user_id: receiver_id.clone(),
                        roles: vec![],
                        muted: false,
                        pinned: false,
                        attributes: std::collections::HashMap::new(),
                        joined_at: chrono::Utc::now(),
                        nickname: None,
                    });
                }
            }
        }
        
        // 创建新会话
        let conversation = Conversation {
            conversation_id: message.conversation_id.clone().unwrap_or_default(),
            conversation_type: format!("{:?}", message.conversation_type),
            business_type: message.business_type.clone(),
            display_name: String::new(), // 需要从其他服务获取
            avatar_url: None,
            unread_count: 1, // 新消息未读
            max_seq: message.seq.unwrap_or(0),
            last_read_seq: 0,
            last_message: Some(last_message),
            is_muted: false,
            is_pinned: false,
            is_muted_detail: false,
            mute_until: None,
            visibility: ConversationVisibility::Public,
            lifecycle_state: ConversationLifecycleState::Active,
            attributes: std::collections::HashMap::new(),
            participants,
            policy: None,
            presence: None,
            announcement: None,
            announcement_updated_at: None,
            announcement_updated_by: None,
            description: None,
            extended_config: std::collections::HashMap::new(),
            ext: std::collections::HashMap::new(),
            labels: vec![],
            draft: None,
            input_state: None,
            created_at: chrono::Utc::now(),
            updated_at: message.timestamp,
            version: 0,
        };
        
        Ok(conversation)
    }
    
    /// 更新会话的最后一条消息
    ///
    /// 对标微信、Telegram、飞书的会话更新机制
    pub fn update_last_message(
        &self,
        conversation: &mut Conversation,
        message: &crate::domain::message::Message,
    ) -> Result<()> {
        use crate::domain::conversation::MessagePreview;
        
        // 构建消息预览
        let last_message = MessagePreview {
            message_id: message.server_id.clone().unwrap_or_else(|| message.client_msg_id.clone()),
            sender_id: message.sender_id.clone(),
            message_type: format!("{:?}", message.message_type),
            text: self.extract_message_preview_text(message),
            time: message.timestamp,
        };
        
        // 更新会话
        conversation.last_message = Some(last_message);
        conversation.max_seq = message.seq.unwrap_or(conversation.max_seq);
        conversation.updated_at = chrono::Utc::now();
        conversation.version += 1;
        
        Ok(())
    }
    
    /// 提取消息预览文本
    ///
    /// 对标微信、Telegram、飞书的消息预览生成逻辑
    fn extract_message_preview_text(&self, message: &crate::domain::message::Message) -> String {
        use crate::domain::message::MessageType;
        
        match message.message_type {
            MessageType::Operation => {
                // 操作消息预览：根据操作类型生成预览文本
                "[操作消息]".to_string()
            }
            MessageType::Text => {
                // 使用统一的解码方法从 protobuf 解码文本内容
                match flare_proto::decode_message_content(&message.content) {
                    Ok(decoded_content) => {
                        if let Some(flare_proto::flare::common::v1::message_content::Content::Text(text_content)) = decoded_content.content {
                            text_content.text
                        } else {
                            // 如果不是文本内容，则返回默认文本
                            "[文本消息]".to_string()
                        }
                    }
                    Err(_) => {
                        // 如果解码失败，返回默认文本
                        "[文本消息]".to_string()
                    }
                }
            }
            MessageType::Image => "[图片]".to_string(),
            MessageType::Video => "[视频]".to_string(),
            MessageType::Audio => "[语音]".to_string(),
            MessageType::File => {
                // 尝试从附件获取文件名
                message.attachments.first()
                    .and_then(|a| a.metadata.get("file_name"))
                    .cloned()
                    .unwrap_or_else(|| "[文件]".to_string())
            }
            MessageType::Location => "[位置]".to_string(),
            MessageType::Card => "[名片]".to_string(),
            MessageType::Custom => "[自定义消息]".to_string(),
            MessageType::Notification => "[通知]".to_string(),
        }
    }
}

impl Default for ConversationDomainService {
    fn default() -> Self {
        Self::new()
    }
}
