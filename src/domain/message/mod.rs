//! Message 聚合根
//!
//! 职责：管理消息生命周期
//! 对齐 flare-proto 的 Message 定义，达到生产级别

pub mod builder;
pub mod operation;
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
pub use text_processor::TextContentProcessor;

/// Message 聚合根
///
/// 对齐 flare-proto/common/message.proto 的 Message 定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    // ========== 消息头（路由索引层）==========
    /// 消息ID（全局唯一）
    pub id: String,
    
    /// 会话ID
    pub conversation_id: String,
    
    /// 客户端消息ID（用于去重）
    pub client_msg_id: String,
    
    /// 发送者ID
    pub sender_id: String,
    
    /// 消息来源
    pub source: MessageSource,
    
    /// 消息序列号（用于排序）
    pub seq: Option<u64>,
    
    /// 时间戳
    pub timestamp: DateTime<Utc>,
    
    /// 会话类型
    pub conversation_type: ConversationType,
    
    /// 消息类型
    pub message_type: MessageType,
    
    /// 业务类型（可选，业务系统扩展）
    pub business_type: Option<String>,
    
    // ========== 路由字段 ==========
    /// 接收者ID（单聊时必需，群聊时为空）
    pub receiver_id: Option<String>,
    
    /// 通道ID（群聊/频道时使用）
    pub channel_id: Option<String>,
    
    // ========== 消息体 ==========
    /// 消息内容（序列化的 MessageContent）
    pub content: Vec<u8>,
    
    /// 内容子类型（用于文本格式区分）
    pub content_type: ContentType,
    
    /// 媒体附件列表
    pub attachments: Vec<MediaAttachment>,
    
    /// 系统扩展字段
    pub extra: HashMap<String, String>,
    
    /// 业务扩展字段
    pub attributes: HashMap<String, String>,
    
    // ========== 消息状态（生命周期状态层）==========
    /// 当前状态
    pub state: MessageState,
    
    /// 是否已撤回
    pub is_recalled: bool,
    
    /// 撤回时间
    pub recalled_at: Option<DateTime<Utc>>,
    
    /// 撤回原因
    pub recall_reason: Option<String>,
    
    /// 是否阅后即焚
    pub is_burn_after_read: bool,
    
    /// 阅后即焚秒数
    pub burn_after_seconds: Option<i32>,
    
    /// 时间线信息
    pub timeline: MessageTimeline,
    
    /// 可见性状态（user_id -> VisibilityStatus）
    pub visibility: HashMap<String, VisibilityStatus>,
    
    /// 已读记录列表
    pub read_by: Vec<MessageReadRecord>,
    
    /// 反应列表
    pub reactions: Vec<Reaction>,
    
    /// 编辑历史列表
    pub edit_history: Vec<EditHistory>,
    
    // ========== 上下文信息 ==========
    /// 租户上下文
    pub tenant: TenantContext,
    
    /// 审计上下文（可选）
    pub audit: Option<AuditContext>,
    
    // ========== 扩展信息 ==========
    /// 标签列表
    pub tags: Vec<String>,
    
    /// 离线推送信息
    pub offline_push_info: Option<OfflinePushInfo>,
    
    // ========== 内部状态 ==========
    /// 版本（用于乐观锁）
    pub version: u64,
    
    /// 创建时间
    pub created_at: DateTime<Utc>,
    
    /// 更新时间
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

/// 消息类型（对齐 MessageType，简化版）
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

/// 租户上下文
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantContext {
    pub tenant_id: String,
    pub user_id: String,
}

/// 审计上下文
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditContext {
    pub operator_id: String,
    pub operation_type: String,
    pub operation_time: DateTime<Utc>,
    pub ip_address: Option<String>,
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
        id: String,
        client_msg_id: String,
        conversation_id: String,
        sender_id: String,
        message_type: MessageType,
        content: Vec<u8>,
        tenant: TenantContext,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            conversation_id,
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
            tenant,
            audit: None,
            tags: Vec::new(),
            offline_push_info: None,
            version: 0,
            created_at: now,
            updated_at: now,
        }
    }
    
    /// 开始发送
    ///
    /// # 参数
    /// * `allow_retry` - 是否允许重试（如果为 true，允许从 Sent/Failed 状态重新发送）
    pub fn start_sending(&mut self, allow_retry: bool) -> anyhow::Result<()> {
        if self.state == MessageState::Created {
            // 正常发送流程
        self.state = MessageState::Sent;
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
        } else if allow_retry && (self.state == MessageState::Sent || self.state == MessageState::Failed) {
            // 重试流程：允许从 Sent 或 Failed 状态重新发送
            // 重置状态为 Sent，准备重新发送
            self.state = MessageState::Sent;
            self.version += 1;
            self.updated_at = Utc::now();
            // 清除之前的错误信息（如果有）
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Message is not in Created state (current state: {:?}), and retry is not allowed",
                self.state
            ))
        }
    }
    
    /// 发送成功（收到 ACK）
    pub fn send_success(&mut self, seq: u64) -> anyhow::Result<()> {
        if self.state != MessageState::Sent {
            return Err(anyhow::anyhow!("Message is not in Sent state"));
        }
        self.seq = Some(seq);
        self.timeline.persisted_at = Some(Utc::now());
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    /// 标记已送达
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
    
    /// 标记已读
    pub fn mark_read(&mut self, user_id: String) -> anyhow::Result<()> {
        if self.state == MessageState::Recalled || self.state == MessageState::Failed {
            return Err(anyhow::anyhow!("Message is in invalid state for read"));
        }
        
        // 检查是否已读
        if !self.read_by.iter().any(|r| r.user_id == user_id) {
            self.read_by.push(MessageReadRecord {
                user_id: user_id.clone(),
                read_at: Utc::now(),
                burned_at: None,
            });
        }
        
        // 如果是阅后即焚，设置销毁时间
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
    
    /// 标记发送失败
    pub fn mark_failed(&mut self) -> anyhow::Result<()> {
        if self.state != MessageState::Sent {
            return Err(anyhow::anyhow!("Message is not in Sent state"));
        }
        self.state = MessageState::Failed;
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    /// 撤回消息
    pub fn recall(&mut self, recaller_id: String, reason: Option<String>) -> anyhow::Result<()> {
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
    
    /// 添加反应
    pub fn add_reaction(&mut self, emoji: String, user_id: String) {
        if let Some(reaction) = self.reactions.iter_mut().find(|r| r.emoji == emoji) {
            if !reaction.user_ids.contains(&user_id) {
                reaction.user_ids.push(user_id);
                reaction.count = reaction.user_ids.len() as i32;
                reaction.last_updated = Utc::now();
            }
        } else {
            self.reactions.push(Reaction {
                emoji: emoji.clone(),
                user_ids: vec![user_id],
                count: 1,
                last_updated: Utc::now(),
                created_at: Utc::now(),
            });
        }
        self.version += 1;
        self.updated_at = Utc::now();
    }
    
    /// 移除反应
    pub fn remove_reaction(&mut self, emoji: String, user_id: String) {
        if let Some(reaction) = self.reactions.iter_mut().find(|r| r.emoji == emoji) {
            reaction.user_ids.retain(|id| id != &user_id);
            reaction.count = reaction.user_ids.len() as i32;
            reaction.last_updated = Utc::now();
            
            // 如果没有用户了，移除反应
            if reaction.user_ids.is_empty() {
                self.reactions.retain(|r| r.emoji != emoji);
            }
        }
        self.version += 1;
        self.updated_at = Utc::now();
    }
    
    /// 编辑消息
    pub fn edit(&mut self, new_content: Vec<u8>, editor_id: String, reason: Option<String>) -> anyhow::Result<()> {
        if self.state == MessageState::Recalled {
            return Err(anyhow::anyhow!("Cannot edit recalled message"));
        }
        
        let edit_version = self.edit_history.len() as i32 + 1;
        self.edit_history.push(EditHistory {
            edit_version,
            content: self.content.clone(),
            edited_at: Utc::now(),
            editor_id: editor_id.clone(),
            reason: reason.clone(),
            show_edited_mark: true,
        });
        
        self.content = new_content;
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
}
