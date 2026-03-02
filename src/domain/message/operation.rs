//! 消息操作
//!
//! 实现所有消息操作，对齐 flare-proto 定义
//! 对标微信、Telegram、飞书的生产级别实现

use crate::domain::message::*;
use chrono::Utc;

/// 消息操作类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationType {
    /// 撤回消息
    Recall,
    
    /// 编辑消息
    Edit,
    
    /// 删除消息
    Delete,
    
    /// 已读回执
    Read,
    
    /// 转发消息
    Forward,
    
    /// 添加反应
    ReactionAdd,
    
    /// 移除反应
    ReactionRemove,
    
    /// 置顶消息
    Pin,
    
    /// 取消置顶
    Unpin,
    
    /// 标记消息
    Mark,
    
    /// 取消标记
    Unmark,
}

/// 删除类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DeleteType {
    /// 软删除（仅对当前用户隐藏）
    Soft,
    
    /// 硬删除（永久删除，仅管理员）
    Hard,
}

/// 反应操作类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReactionAction {
    /// 添加反应
    Add,
    
    /// 移除反应
    Remove,
}

/// 标记类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
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
    
    /// 置顶操作
    Pin {
        reason: Option<String>,
        expire_at: Option<chrono::DateTime<Utc>>,
    },
    
    /// 取消置顶操作
    Unpin,
    
    /// 标记操作
    Mark {
        mark_type: MarkType,
        color: Option<String>,
    },
    
    /// 取消标记操作
    Unmark {
        mark_type: Option<MarkType>,
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
        let operator_id = operation.operator_id;
        let operation_data = operation.operation_data;
        
        match operation_data {
            OperationData::Recall { reason, .. } => {
                message.recall(operator_id, reason)?;
            }
            
            OperationData::Edit { new_content, edit_version, reason, show_edited_mark } => {
                message.edit_with_details(new_content, operator_id, reason, show_edited_mark, edit_version)?;
            }
            
            OperationData::Delete { delete_type, reason, notify_others } => {
                match delete_type {
                    DeleteType::Soft => {
                        // 软删除：设置可见性为隐藏
                        message.visibility.insert(
                            operator_id.clone(),
                            VisibilityStatus::Hidden,
                        );
                    }
                    DeleteType::Hard => {
                        // 硬删除：设置可见性为已删除
                        message.visibility.insert(
                            operator_id.clone(),
                            VisibilityStatus::Deleted,
                        );
                    }
                }
                // 记录删除原因
                if let Some(r) = reason {
                    message.attributes.insert("delete_reason".to_string(), r);
                }
                // 记录是否通知他人
                message.attributes.insert("delete_notify_others".to_string(), notify_others.to_string());
                message.version += 1;
                message.updated_at = Utc::now();
            }
            
            OperationData::Read { message_ids, .. } => {
                if let Some(server_id) = &message.server_id {
                    if message_ids.contains(server_id) {
                        message.mark_read(operator_id)?;
                    }
                }
            }
            
            OperationData::Reaction { emoji, action, .. } => {
                match action {
                    ReactionAction::Add => {
                        message.add_reaction(emoji, operator_id);
                    }
                    ReactionAction::Remove => {
                        message.remove_reaction(emoji, operator_id);
                    }
                }
            }
            
            OperationData::Pin { reason, expire_at } => {
                // 置顶逻辑：在 attributes 中标记为置顶
                message.attributes.insert("pinned".to_string(), "true".to_string());
                message.attributes.insert("pinned_at".to_string(), chrono::Utc::now().to_rfc3339());
                message.attributes.insert("pinned_by".to_string(), operator_id);
                if let Some(r) = reason {
                    message.attributes.insert("pin_reason".to_string(), r);
                }
                if let Some(expire) = expire_at {
                    message.attributes.insert("pin_expire_at".to_string(), expire.to_rfc3339());
                }
                message.version += 1;
                message.updated_at = Utc::now();
            }
            
            OperationData::Unpin => {
                // 取消置顶：移除置顶相关属性
                message.attributes.remove("pinned");
                message.attributes.remove("pinned_at");
                message.attributes.remove("pinned_by");
                message.attributes.remove("pin_reason");
                message.attributes.remove("pin_expire_at");
                message.version += 1;
                message.updated_at = Utc::now();
            }
            
            OperationData::Mark { mark_type, color } => {
                // 标记逻辑：在 attributes 中记录标记信息
                message.attributes.insert(
                    format!("mark_type_{:?}", mark_type),
                    "true".to_string(),
                );
                message.attributes.insert("marked_at".to_string(), chrono::Utc::now().to_rfc3339());
                message.attributes.insert("marked_by".to_string(), operator_id);
                if let Some(c) = color {
                    message.attributes.insert("mark_color".to_string(), c);
                }
                message.version += 1;
                message.updated_at = Utc::now();
            }
            
            OperationData::Unmark { mark_type } => {
                // 取消标记逻辑：移除标记信息
                if let Some(mt) = mark_type {
                    // 取消特定类型的标记
                    message.attributes.remove(&format!("mark_type_{:?}", mt));
                } else {
                    // 取消所有标记
                    message.attributes.retain(|k, _| !k.starts_with("mark_"));
                }
                message.version += 1;
                message.updated_at = Utc::now();
            }
            
            OperationData::Forward { message_ids, target_conversation_id, reason, merge_forward } => {
                // 转发操作：记录转发信息
                // 转发本身不影响原消息状态，只是创建转发记录
                message.attributes.insert("forwarded".to_string(), "true".to_string());
                message.attributes.insert("forwarded_at".to_string(), chrono::Utc::now().to_rfc3339());
                message.attributes.insert("forwarded_by".to_string(), operator_id);
                
                if let Some(r) = reason {
                    message.attributes.insert("forward_reason".to_string(), r);
                }
                
                // 记录转发的目标会话和消息ID
                message.attributes.insert("forward_target_conversation".to_string(), target_conversation_id);
                message.attributes.insert("forward_message_ids".to_string(), serde_json::to_string(&message_ids)?);
                message.attributes.insert("forward_merge".to_string(), merge_forward.to_string());
                
                message.version += 1;
                message.updated_at = Utc::now();
            }
        }
        
        Ok(())
    }
}
