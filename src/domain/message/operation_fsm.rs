//! 消息操作状态机
//!
//! 实现消息操作的有限状态机，确保操作的一致性和有效性
//! 对标微信、Telegram、飞书的生产级别实现

use crate::domain::message::{Message, MessageState};
use crate::domain::message::operation::{MessageOperation, OperationType, OperationData, DeleteType};
use anyhow::Result;

/// 消息操作状态机
#[derive(Debug, Clone)]
pub struct MessageOperationFSM;

impl MessageOperationFSM {
    /// 验证操作是否可以应用于消息
    pub fn can_apply_operation(message: &Message, operation: &MessageOperation) -> Result<bool> {
        // 基本状态检查 - Created 状态的消息也可以接受操作
        if message.state == MessageState::Failed {
            return Ok(false);
        }

        // 根据操作类型进行特定验证
        match operation.operation_type {
            OperationType::Recall => {
                // 撤回操作：消息不能已经被撤回
                Ok(!message.is_recalled)
            }
            OperationType::Edit => {
                // 编辑操作：消息不能已经被撤回
                Ok(!message.is_recalled)
            }
            OperationType::Delete => {
                // 删除操作：总是允许（软删除）
                Ok(true)
            }
            OperationType::Read => {
                // 已读操作：消息必须存在且未被撤回
                Ok(!message.is_recalled && message.state != MessageState::Failed)
            }
            OperationType::ReactionAdd | OperationType::ReactionRemove => {
                // 反应操作：消息必须存在且未被撤回
                Ok(!message.is_recalled && message.state != MessageState::Failed)
            }
            OperationType::Pin | OperationType::Unpin => {
                // 置顶操作：消息必须存在且未被撤回
                Ok(!message.is_recalled && message.state != MessageState::Failed)
            }
            OperationType::Mark | OperationType::Unmark => {
                // 标记操作：消息必须存在且未被撤回
                Ok(!message.is_recalled && message.state != MessageState::Failed)
            }
            OperationType::Forward => {
                // 转发操作：消息必须存在且未被撤回
                Ok(!message.is_recalled && message.state != MessageState::Failed)
            }
        }
    }

    /// 验证操作前的状态
    pub fn validate_before_operation(message: &Message, operation: &MessageOperation) -> Result<()> {
        // 基本验证
        if message.state == MessageState::Failed {
            return Err(anyhow::anyhow!("Cannot operate on failed message"));
        }

        // 特定操作验证
        match operation.operation_type {
            OperationType::Recall => {
                if message.is_recalled {
                    return Err(anyhow::anyhow!("Message already recalled"));
                }
            }
            OperationType::Edit => {
                if message.is_recalled {
                    return Err(anyhow::anyhow!("Cannot edit recalled message"));
                }
            }
            _ => {
                // 其他操作类型的基本验证已通过
            }
        }

        Ok(())
    }

    /// 更新消息状态以反映操作完成
    pub fn update_state_after_operation(message: &mut Message, operation: &MessageOperation) {
        match operation.operation_type {
            OperationType::Recall => {
                message.state = MessageState::Recalled;
            }
            OperationType::Edit => {
                // 编辑不会改变基本状态，只是更新内容
                message.updated_at = chrono::Utc::now();
            }
            OperationType::Delete => {
                if let OperationData::Delete { delete_type, .. } = &operation.operation_data {
                    match delete_type {
                        DeleteType::Soft => {
                            // 软删除：更新可见性，不改变基本状态
                        }
                        DeleteType::Hard => {
                            // 硬删除：更新可见性，不改变基本状态
                        }
                    }
                }
            }
            OperationType::Read => {
                if message.state != MessageState::Recalled && message.state != MessageState::Failed {
                    message.state = MessageState::Read;
                }
            }
            OperationType::ReactionAdd => {
                // 反应添加：更新反应统计，不影响消息基本状态
            }
            OperationType::ReactionRemove => {
                // 反应移除：更新反应统计，不影响消息基本状态
            }
            OperationType::Pin => {
                // 置顶：更新属性，不影响消息基本状态
            }
            OperationType::Unpin => {
                // 取消置顶：更新属性，不影响消息基本状态
            }
            OperationType::Mark => {
                // 标记：更新属性，不影响消息基本状态
            }
            OperationType::Unmark => {
                // 取消标记：更新属性，不影响消息基本状态
            }
            OperationType::Forward => {
                // 转发：不影响原消息状态，只是创建新的转发记录
            }
        }

        // 更新版本号
        message.version += 1;
        message.updated_at = chrono::Utc::now();
    }

    /// 检查消息是否可以接受某种类型的操作
    pub fn can_accept_operation(message: &Message, operation_type: OperationType) -> bool {
        // 基本状态检查 - Created 状态也可以接受操作
        if message.state == MessageState::Failed {
            return false;
        }

        match operation_type {
            OperationType::Recall => !message.is_recalled,
            OperationType::Edit => !message.is_recalled,
            OperationType::Delete => true, // 删除总是允许
            OperationType::Read => !message.is_recalled && message.state != MessageState::Failed,
            OperationType::ReactionAdd | OperationType::ReactionRemove => {
                !message.is_recalled && message.state != MessageState::Failed
            }
            OperationType::Pin | OperationType::Unpin => {
                !message.is_recalled && message.state != MessageState::Failed
            }
            OperationType::Mark | OperationType::Unmark => {
                !message.is_recalled && message.state != MessageState::Failed
            }
            OperationType::Forward => !message.is_recalled && message.state != MessageState::Failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::message::operation::{OperationType, OperationData};

    #[test]
    fn test_can_apply_operation_recall() {
        let mut message = crate::domain::message::Message::new(
            Some("msg1".to_string()),
            "client1".to_string(),
            "user1".to_string(),
            crate::domain::message::MessageType::Text,
            b"hello".to_vec(),
        );

        let operation = MessageOperation {
            operation_type: OperationType::Recall,
            target_message_id: "msg1".to_string(),
            operator_id: "user1".to_string(),
            timestamp: chrono::Utc::now(),
            show_notice: true,
            notice_text: None,
            target_user_id: None,
            operation_data: OperationData::Recall {
                reason: Some("test".to_string()),
                time_limit_seconds: Some(300),
                allow_admin_recall: true,
            },
            metadata: std::collections::HashMap::new(),
        };

        // 有效消息可以撤回
        assert!(MessageOperationFSM::can_apply_operation(&message, &operation).unwrap());

        // 已撤回的消息不能再次撤回
        message.is_recalled = true;
        assert!(!MessageOperationFSM::can_apply_operation(&message, &operation).unwrap());
    }

    #[test]
    fn test_can_apply_operation_edit() {
        let mut message = crate::domain::message::Message::new(
            Some("msg1".to_string()),
            "client1".to_string(),
            "user1".to_string(),
            crate::domain::message::MessageType::Text,
            b"hello".to_vec(),
        );

        let operation = MessageOperation {
            operation_type: OperationType::Edit,
            target_message_id: "msg1".to_string(),
            operator_id: "user1".to_string(),
            timestamp: chrono::Utc::now(),
            show_notice: true,
            notice_text: None,
            target_user_id: None,
            operation_data: OperationData::Edit {
                new_content: b"updated hello".to_vec(),
                edit_version: 1,
                reason: Some("test edit".to_string()),
                show_edited_mark: true,
            },
            metadata: std::collections::HashMap::new(),
        };

        // 有效消息可以编辑
        assert!(MessageOperationFSM::can_apply_operation(&message, &operation).unwrap());

        // 已撤回的消息不能编辑
        message.is_recalled = true;
        assert!(!MessageOperationFSM::can_apply_operation(&message, &operation).unwrap());
    }

    #[test]
    fn test_validate_before_operation() {
        let mut message = crate::domain::message::Message::new(
            Some("msg1".to_string()),
            "client1".to_string(),
            "user1".to_string(),
            crate::domain::message::MessageType::Text,
            b"hello".to_vec(),
        );

        let operation = MessageOperation {
            operation_type: OperationType::Recall,
            target_message_id: "msg1".to_string(),
            operator_id: "user1".to_string(),
            timestamp: chrono::Utc::now(),
            show_notice: true,
            notice_text: None,
            target_user_id: None,
            operation_data: OperationData::Recall {
                reason: Some("test".to_string()),
                time_limit_seconds: Some(300),
                allow_admin_recall: true,
            },
            metadata: std::collections::HashMap::new(),
        };

        // 有效消息验证通过
        assert!(MessageOperationFSM::validate_before_operation(&message, &operation).is_ok());

        // 已撤回的消息验证失败
        message.is_recalled = true;
        assert!(MessageOperationFSM::validate_before_operation(&message, &operation).is_err());
    }

    #[test]
    fn test_update_state_after_operation() {
        let mut message = crate::domain::message::Message::new(
            Some("msg1".to_string()),
            "client1".to_string(),
            "user1".to_string(),
            crate::domain::message::MessageType::Text,
            b"hello".to_vec(),
        );
        let initial_version = message.version;

        let operation = MessageOperation {
            operation_type: OperationType::Recall,
            target_message_id: "msg1".to_string(),
            operator_id: "user1".to_string(),
            timestamp: chrono::Utc::now(),
            show_notice: true,
            notice_text: None,
            target_user_id: None,
            operation_data: OperationData::Recall {
                reason: Some("test".to_string()),
                time_limit_seconds: Some(300),
                allow_admin_recall: true,
            },
            metadata: std::collections::HashMap::new(),
        };

        MessageOperationFSM::update_state_after_operation(&mut message, &operation);
        
        // 消息状态应该变为已撤回
        assert_eq!(message.state, crate::domain::message::MessageState::Recalled);
        // 版本号应该增加
        assert_eq!(message.version, initial_version + 1);
    }

    #[test]
    fn test_can_accept_operation_type() {
        let mut message = crate::domain::message::Message::new(
            Some("msg1".to_string()),
            "client1".to_string(),
            "user1".to_string(),
            crate::domain::message::MessageType::Text,
            b"hello".to_vec(),
        );

        // 未撤回的消息可以接受撤回操作
        assert!(MessageOperationFSM::can_accept_operation(&message, OperationType::Recall));

        // 撤回后就不能再接受撤回操作
        message.is_recalled = true;
        assert!(!MessageOperationFSM::can_accept_operation(&message, OperationType::Recall));
    }
}
