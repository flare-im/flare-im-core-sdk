//! 消息操作
//!
//! 实现所有消息操作，对齐 flare-proto 定义
//! 对标微信、Telegram、飞书的生产级别实现

use crate::domain::message::*;
use chrono::Utc;
use prost_types::Timestamp;

/// 消息操作类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationType {
    /// 撤回消息
    Recall,
    
    /// 编辑消息
    Edit,
    
    /// 删除消息
    Delete,
    
    /// 已读回执
    Read,
    
    /// 回复消息
    Reply,
    
    /// 转发消息
    Forward,
    
    /// 添加反应
    ReactionAdd,
    
    /// 移除反应
    ReactionRemove,
    
    /// 引用消息
    Quote,
    
    /// 话题回复
    ThreadReply,
    
    /// 置顶消息
    Pin,
    
    /// 取消置顶
    Unpin,
    
    /// 收藏消息
    Favorite,
    
    /// 取消收藏
    Unfavorite,
    
    /// 标记消息
    Mark,
}

/// 删除类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteType {
    /// 软删除（仅对当前用户隐藏）
    Soft,
    
    /// 硬删除（永久删除，仅管理员）
    Hard,
}

/// 反应操作类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReactionAction {
    /// 添加反应
    Add,
    
    /// 移除反应
    Remove,
}

/// 标记类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkType {
    /// 重要
    Important,
    
    /// 待办
    Todo,
    
    /// 已处理
    Done,
    
    /// 自定义
    Custom,
}

/// 消息操作
///
/// 统一的操作结构，用于审计和追踪
#[derive(Debug, Clone)]
pub struct MessageOperation {
    pub operation_type: OperationType,
    pub target_message_id: String,
    pub operator_id: String,
    pub timestamp: chrono::DateTime<Utc>,
    pub show_notice: bool,
    pub notice_text: Option<String>,
    pub target_user_id: Option<String>,
    pub operation_data: OperationData,
    pub metadata: std::collections::HashMap<String, String>,
}

/// 操作数据（oneof）
#[derive(Debug, Clone)]
pub enum OperationData {
    /// 撤回操作
    Recall {
        reason: Option<String>,
        time_limit_seconds: Option<i32>,
        allow_admin_recall: bool,
    },
    
    /// 编辑操作
    Edit {
        new_content: Vec<u8>,
        edit_version: i32,
        reason: Option<String>,
        show_edited_mark: bool,
    },
    
    /// 删除操作
    Delete {
        delete_type: DeleteType,
        reason: Option<String>,
        notify_others: bool,
    },
    
    /// 已读操作
    Read {
        message_ids: Vec<String>,
        read_at: Option<chrono::DateTime<Utc>>,
        burn_after_read: bool,
    },
    
    /// 回复操作
    Reply {
        reply_to_message_id: String,
        reply_content: Vec<u8>,
        quote_original: bool,
    },
    
    /// 转发操作
    Forward {
        message_ids: Vec<String>,
        target_conversation_id: String,
        reason: Option<String>,
        merge_forward: bool,
    },
    
    /// 反应操作
    Reaction {
        emoji: String,
        action: ReactionAction,
        count: i32,
    },
    
    /// 引用操作
    Quote {
        quoted_message_id: String,
        preview_text: Option<String>,
    },
    
    /// 话题操作
    Thread {
        thread_id: String,
        thread_title: Option<String>,
        reply_content: Vec<u8>,
    },
    
    /// 置顶操作
    Pin {
        reason: Option<String>,
        expire_at: Option<chrono::DateTime<Utc>>,
    },
    
    /// 收藏操作
    Favorite {
        tags: Vec<String>,
        note: Option<String>,
    },
    
    /// 标记操作
    Mark {
        mark_type: MarkType,
        color: Option<String>,
    },
}

/// 消息操作处理器
pub struct MessageOperationHandler;

impl MessageOperationHandler {
    /// 执行消息操作
    pub async fn execute(
        operation: MessageOperation,
        message: &mut Message,
    ) -> anyhow::Result<()> {
        match operation.operation_data {
            OperationData::Recall { reason, .. } => {
                message.recall(operation.operator_id, reason)?;
            }
            
            OperationData::Edit { new_content, edit_version, reason, show_edited_mark } => {
                message.edit(new_content, operation.operator_id.clone(), reason)?;
            }
            
            OperationData::Delete { delete_type, .. } => {
                match delete_type {
                    DeleteType::Soft => {
                        // 软删除：设置可见性为隐藏
                        message.visibility.insert(
                            operation.operator_id.clone(),
                            VisibilityStatus::Hidden,
                        );
                    }
                    DeleteType::Hard => {
                        // 硬删除：设置可见性为已删除
                        message.visibility.insert(
                            operation.operator_id.clone(),
                            VisibilityStatus::Deleted,
                        );
                    }
                }
                message.version += 1;
                message.updated_at = Utc::now();
            }
            
            OperationData::Read { message_ids, read_at, burn_after_read } => {
                if message_ids.contains(&message.id) {
                    message.mark_read(operation.operator_id.clone())?;
                    if burn_after_read {
                        message.is_burn_after_read = true;
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
            
            OperationData::Pin { reason, expire_at } => {
                // 置顶逻辑：在 attributes 中标记为置顶
                message.attributes.insert("pinned".to_string(), "true".to_string());
                message.attributes.insert("pinned_at".to_string(), chrono::Utc::now().to_rfc3339());
                if let Some(reason) = reason {
                    message.attributes.insert("pin_reason".to_string(), reason);
                }
                if let Some(expire) = expire_at {
                    message.attributes.insert("pin_expire_at".to_string(), expire.to_rfc3339());
                }
                message.version += 1;
                message.updated_at = Utc::now();
            }
            
            OperationData::Favorite { tags, note } => {
                // 收藏逻辑：添加到 tags 和 attributes
                message.tags.extend(tags.clone());
                message.attributes.insert("favorited".to_string(), "true".to_string());
                message.attributes.insert("favorited_at".to_string(), chrono::Utc::now().to_rfc3339());
                if let Some(note) = note {
                    message.attributes.insert("favorite_note".to_string(), note);
                }
                if !tags.is_empty() {
                    message.attributes.insert("favorite_tags".to_string(), tags.join(","));
                }
                message.version += 1;
                message.updated_at = Utc::now();
            }
            
            OperationData::Mark { mark_type, color } => {
                // 标记逻辑：在 attributes 中记录标记信息
                message.attributes.insert(
                    "mark_type".to_string(),
                    format!("{:?}", mark_type),
                );
                message.attributes.insert("marked_at".to_string(), chrono::Utc::now().to_rfc3339());
                if let Some(color) = color {
                    message.attributes.insert("mark_color".to_string(), color);
                }
                message.version += 1;
                message.updated_at = Utc::now();
            }
            
            _ => {
                // 其他操作暂未实现
                return Err(anyhow::anyhow!("Operation not implemented yet"));
            }
        }
        
        Ok(())
    }
}
