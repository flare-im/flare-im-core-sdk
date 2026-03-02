pub mod builder;
pub mod operation;
pub mod operation_fsm;
pub mod text_processor;

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

pub use builder::{
    MessageBuilder,
    build_text_message,
    build_image_message,
    build_file_message,
    build_video_message,
    build_audio_message,
    build_reply_message,
};
pub use operation::{
    MessageOperationHandler,
    MessageOperation,
    OperationType,
    OperationData,
    DeleteType,
    ReactionAction,
    MarkType,
};
pub use operation_fsm::MessageOperationFSM;
pub use text_processor::TextContentProcessor;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Message {
    pub server_id: Option<String>,
    pub conversation_id: Option<String>, 
    pub client_msg_id: String,
    pub sender_id: String,
    pub source: MessageSource,
    pub seq: Option<u64>,
    pub timestamp: DateTime<Utc>,
    pub conversation_type: ConversationType,
    pub message_type: MessageType,
    pub business_type: Option<String>,
    pub receiver_id: Option<String>,
    pub channel_id: Option<String>,
    pub content: Vec<u8>,
    pub content_type: ContentType,
    pub attachments: Vec<MediaAttachment>,
    pub quote: Option<QuoteContent>,
    pub extra: HashMap<String, String>,
    pub attributes: HashMap<String, String>,
    pub state: MessageState,
    pub is_recalled: bool,
    pub recalled_at: Option<DateTime<Utc>>,
    pub recall_reason: Option<String>,
    pub is_burn_after_read: bool,
    pub burn_after_seconds: Option<i32>,
    pub timeline: MessageTimeline,
    pub visibility: HashMap<String, VisibilityStatus>,
    pub read_by: Vec<MessageReadRecord>,
    pub reactions: Vec<Reaction>,
    pub edit_history: Vec<EditHistory>,
    pub audit: Option<AuditContext>,
    pub tags: Vec<String>,
    pub offline_push_info: Option<OfflinePushInfo>,
    pub version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 消息状态（对齐 MessageStatus）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageState {
    /// 已创建
    Created,
    
    /// 已发送
    Sent,
    
    /// 已送达
    Delivered,
    
    /// 已读
    Read,
    
    /// 发送失败
    Failed,
    
    /// 已撤回
    Recalled,
}

/// 消息来源（对齐 MessageSource）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageSource {
    /// 用户消息
    User,
    
    /// 系统消息
    System,
    
    /// 机器人消息
    Bot,
    
    /// 管理员消息
    Admin,
}

/// 会话类型（对齐 ConversationType）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConversationType {
    /// 单聊
    Single,
    
    /// 群聊
    Group,
    
    /// 频道
    Channel,
}

/// 消息类型（对齐 MessageType）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageType {
    /// 文本消息
    Text,
    
    /// 图片消息
    Image,
    
    /// 视频消息
    Video,
    
    /// 语音消息
    Audio,
    
    /// 文件消息
    File,
    
    /// 位置消息
    Location,
    
    /// 名片消息
    Card,
    
    /// 自定义消息
    Custom,
    
    /// 通知消息
    Notification,
    
    /// 消息操作（统一操作类型，包含撤回/编辑/删除/置顶等）
    Operation,
}

/// 内容类型（对齐 ContentType）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentType {
    /// 纯文本
    PlainText,
    
    /// Markdown
    Markdown,
    
    /// HTML
    Html,
    
    /// JSON格式
    Json,
}

/// 媒体附件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaAttachment {
    pub attachment_id: String,
    pub attachment_type: String,
    pub url: String,
    pub size: u64,
    pub mime_type: String,
    pub metadata: HashMap<String, String>,
}

/// 消息时间线
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageTimeline {
    pub created_at: DateTime<Utc>,
    pub persisted_at: Option<DateTime<Utc>>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub read_at: Option<DateTime<Utc>>,
}

/// 可见性状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VisibilityStatus {
    /// 可见
    Visible,
    
    /// 隐藏（软删除）
    Hidden,
    
    /// 已删除（永久删除）
    Deleted,
}

/// 消息已读记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReadRecord {
    pub user_id: String,
    pub read_at: DateTime<Utc>,
    pub burned_at: Option<DateTime<Utc>>,
}

/// 反应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reaction {
    pub emoji: String,
    pub user_ids: Vec<String>,
    pub count: i32,
    pub last_updated: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// 编辑历史
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditHistory {
    pub edit_version: i32,
    pub content: Vec<u8>,
    pub edited_at: DateTime<Utc>,
    pub editor_id: String,
    pub reason: Option<String>,
    pub show_edited_mark: bool,
}


/// 审计上下文
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditContext {
    pub operator_id: String,
    pub operation_type: String,
    pub operation_time: DateTime<Utc>,
    pub ip_address: Option<String>,
}

/// 引用内容（用于在消息中展示被引用的消息，不是消息类型）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteContent {
    /// 被引用的消息ID（用于标识回复关系）
    pub quoted_message_id: String,
    
    /// 被引用消息的发送者ID
    pub quoted_sender_id: String,
    
    /// 引用内容预览（用于显示）
    pub quoted_text_preview: String,
    
    /// 被引用的完整消息内容（可选，用于富文本展示）
    pub quoted_content: Option<Vec<u8>>,
}

/// 离线推送信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflinePushInfo {
    pub title: String,
    pub desc: String,
    pub ios_push_sound: Option<String>,
    pub ios_badge_count: bool,
    pub signal_info: Option<String>,
}

impl Message {
    /// 创建新消息
    pub fn new(
        server_id: Option<String>,
        client_msg_id: String,
        sender_id: String,
        message_type: MessageType,
        content: Vec<u8>,
    ) -> Self {
        let now = Utc::now();
        Self {
            server_id,
            conversation_id: None,
            client_msg_id,
            sender_id,
            source: MessageSource::User,
            seq: None,
            timestamp: now,
            conversation_type: ConversationType::Single,
            message_type,
            business_type: None,
            receiver_id: None,
            channel_id: None,
            content,
            content_type: ContentType::PlainText,
            attachments: Vec::new(),
            quote: None,
            extra: HashMap::new(),
            attributes: HashMap::new(),
            state: MessageState::Created,
            is_recalled: false,
            recalled_at: None,
            recall_reason: None,
            is_burn_after_read: false,
            burn_after_seconds: None,
            timeline: MessageTimeline {
                created_at: now,
                persisted_at: None,
                delivered_at: None,
                read_at: None,
            },
            visibility: HashMap::new(),
            read_by: Vec::new(),
            reactions: Vec::new(),
            edit_history: Vec::new(),
            audit: None,
            tags: Vec::new(),
            offline_push_info: None,
            version: 0,
            created_at: now,
            updated_at: now,
        }
    }
    
    pub fn start_sending(&mut self, allow_retry: bool) -> anyhow::Result<()> {
        match self.state {
            MessageState::Created => {
                self.state = MessageState::Sent;
                self.version += 1;
                self.updated_at = Utc::now();
                Ok(())
            }
            state if allow_retry && (state == MessageState::Sent || state == MessageState::Failed) => {
                self.state = MessageState::Sent;
                self.version += 1;
                self.updated_at = Utc::now();
                Ok(())
            }
            _ => Err(anyhow::anyhow!(
                "Message state {:?} does not allow sending",
                self.state
            )),
        }
    }
    
    pub fn send_success(&mut self, seq: u64, server_msg_id: String) -> anyhow::Result<()> {
        if self.state != MessageState::Sent {
            return Err(anyhow::anyhow!("Message is not in Sent state"));
        }
        self.seq = Some(seq);
        self.timeline.persisted_at = Some(Utc::now());
        self.server_id = Some(server_msg_id);
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    pub fn mark_delivered(&mut self) -> anyhow::Result<()> {
        if self.state != MessageState::Sent {
            return Err(anyhow::anyhow!("Message is not in Sent state"));
        }
        self.state = MessageState::Delivered;
        self.timeline.delivered_at = Some(Utc::now());
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    pub fn mark_read(&mut self, user_id: String) -> anyhow::Result<()> {
        if matches!(self.state, MessageState::Recalled | MessageState::Failed) {
            return Err(anyhow::anyhow!("Message is in invalid state for read"));
        }
        
        if !self.read_by.iter().any(|r| r.user_id == user_id) {
            self.read_by.push(MessageReadRecord {
                user_id: user_id.clone(),
                read_at: Utc::now(),
                burned_at: None,
            });
        }
        
        if self.is_burn_after_read {
            if let Some(record) = self.read_by.iter_mut().find(|r| r.user_id == user_id) {
                record.burned_at = Some(Utc::now());
            }
        }
        
        self.state = MessageState::Read;
        self.timeline.read_at = Some(Utc::now());
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    pub fn mark_failed(&mut self) -> anyhow::Result<()> {
        if self.state != MessageState::Sent {
            return Err(anyhow::anyhow!("Message is not in Sent state"));
        }
        self.state = MessageState::Failed;
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    pub fn recall(&mut self, _recaller_id: String, reason: Option<String>) -> anyhow::Result<()> {
        if self.state == MessageState::Recalled {
            return Err(anyhow::anyhow!("Message is already recalled"));
        }
        self.is_recalled = true;
        self.recalled_at = Some(Utc::now());
        self.recall_reason = reason;
        self.state = MessageState::Recalled;
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    pub fn add_reaction(&mut self, emoji: String, user_id: String) {
        match self.reactions.iter_mut().find(|r| r.emoji == emoji) {
            Some(reaction) => {
                if !reaction.user_ids.contains(&user_id) {
                    reaction.user_ids.push(user_id);
                    reaction.count = reaction.user_ids.len() as i32;
                    reaction.last_updated = Utc::now();
                }
            }
            None => {
                self.reactions.push(Reaction {
                    emoji: emoji.clone(),
                    user_ids: vec![user_id],
                    count: 1,
                    last_updated: Utc::now(),
                    created_at: Utc::now(),
                });
            }
        }
        self.version += 1;
        self.updated_at = Utc::now();
    }
    
    pub fn remove_reaction(&mut self, emoji: String, user_id: String) {
        if let Some(reaction) = self.reactions.iter_mut().find(|r| r.emoji == emoji) {
            reaction.user_ids.retain(|id| id != &user_id);
            reaction.count = reaction.user_ids.len() as i32;
            reaction.last_updated = Utc::now();
            
            if reaction.user_ids.is_empty() {
                self.reactions.retain(|r| r.emoji != emoji);
            }
        }
        self.version += 1;
        self.updated_at = Utc::now();
    }
    
    pub fn edit(&mut self, new_content: Vec<u8>, editor_id: String, reason: Option<String>) -> anyhow::Result<()> {
        self.edit_with_details(new_content, editor_id, reason, true, 0)
    }
    
    pub fn edit_with_details(
        &mut self, 
        new_content: Vec<u8>, 
        editor_id: String, 
        reason: Option<String>,
        show_edited_mark: bool,
        edit_version: i32,
    ) -> anyhow::Result<()> {
        if self.state == MessageState::Recalled {
            return Err(anyhow::anyhow!("Cannot edit recalled message"));
        }
        
        let actual_version = if edit_version > 0 { edit_version } else { self.edit_history.len() as i32 + 1 };
        self.edit_history.push(EditHistory {
            edit_version: actual_version,
            content: self.content.clone(),
            edited_at: Utc::now(),
            editor_id: editor_id.clone(),
            reason: reason.clone(),
            show_edited_mark,
        });
        
        self.content = new_content;
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
}
