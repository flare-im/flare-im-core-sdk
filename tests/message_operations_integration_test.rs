//! 消息操作集成测试（事件流）
//!
//! 对齐 flare-proto：操作统一为 Event，SyncResponse/EventEnvelope 从 events 抽 Message。
//! 验证领域操作 -> 命令执行 -> 事件发布 -> Event 转换往返。

use flare_im_core_sdk::domain::message::{Message, MessageType};
use flare_im_core_sdk::domain::message::operation::{
    MessageOperation, OperationType, OperationData, ReactionAction, MarkType,
};
use flare_im_core_sdk::domain::message::operation_fsm::MessageOperationFSM;
use flare_im_core_sdk::application::commands::{MessageOperationCommand, MessageOperationCommandHandler};
use flare_im_core_sdk::infrastructure::converter::MessageOperationConverter;
use flare_im_core_sdk::prelude::EventBus;
use flare_im_core_sdk::domain::repository::{MessageRepository, MessageListResult};

use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result;
use chrono::Utc;

// ----- TestMessageRepository（实现完整 MessageRepository） -----

struct TestMessageRepository {
    messages: Arc<Mutex<std::collections::HashMap<String, Message>>>,
}

impl TestMessageRepository {
    fn new() -> Self {
        Self {
            messages: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl MessageRepository for TestMessageRepository {
    async fn save(&self, message: &Message) -> Result<()> {
        let mut m = self.messages.lock().await;
        if let Some(id) = &message.server_id {
            m.insert(id.clone(), message.clone());
        }
        Ok(())
    }

    async fn save_batch(&self, messages: &[Message]) -> Result<()> {
        let mut m = self.messages.lock().await;
        for msg in messages {
            if let Some(id) = &msg.server_id {
                m.insert(id.clone(), msg.clone());
            }
        }
        Ok(())
    }

    async fn find_by_id(&self, message_id: &str) -> Result<Option<Message>> {
        let m = self.messages.lock().await;
        Ok(m.get(message_id).cloned())
    }

    async fn find_by_conversation(
        &self,
        conversation_id: &str,
        limit: Option<usize>,
        _cursor: Option<String>,
    ) -> Result<MessageListResult> {
        let m = self.messages.lock().await;
        let mut list: Vec<Message> = m
            .values()
            .filter(|msg| msg.conversation_id.as_deref() == Some(conversation_id))
            .cloned()
            .collect();
        list.sort_by(|a, b| a.seq.cmp(&b.seq));
        let n = limit.unwrap_or(usize::MAX).min(list.len());
        list.truncate(n);
        Ok(MessageListResult {
            messages: list,
            next_cursor: None,
        })
    }

    async fn search(
        &self,
        _conversation_id: Option<&str>,
        _keyword: &str,
        _limit: Option<usize>,
    ) -> Result<Vec<Message>> {
        Ok(Vec::new())
    }

    async fn find_by_time_range(
        &self,
        _conversation_id: Option<&str>,
        _start_time: Option<chrono::DateTime<Utc>>,
        _end_time: Option<chrono::DateTime<Utc>>,
        _limit: Option<usize>,
    ) -> Result<Vec<Message>> {
        Ok(Vec::new())
    }

    async fn delete(&self, message_id: &str) -> Result<()> {
        let mut m = self.messages.lock().await;
        m.remove(message_id);
        Ok(())
    }

    async fn delete_by_conversation(&self, conversation_id: &str) -> Result<()> {
        let mut m = self.messages.lock().await;
        m.retain(|_, msg| msg.conversation_id.as_deref() != Some(conversation_id));
        Ok(())
    }
}

// ----- 生命周期测试：命令执行 + 事件流 -----

#[tokio::test]
async fn test_full_message_operation_lifecycle() {
    let repo = Arc::new(TestMessageRepository::new());
    let event_bus = Arc::new(EventBus::new(100));
    let collected = Arc::new(Mutex::new(Vec::new()));
    let collected_clone = collected.clone();
    let mut rx = event_bus.subscribe();
    tokio::spawn(async move {
        while let Ok(ev) = rx.recv().await {
            collected_clone.lock().await.push(ev);
        }
    });

    let command_handler = MessageOperationCommandHandler::new(repo.clone(), event_bus.clone());

    let mut test_message = Message::new(
        Some("test_msg_001".to_string()),
        "client_msg_001".to_string(),
        "user_001".to_string(),
        MessageType::Text,
        b"Original message content".to_vec(),
    );
    test_message.conversation_id = Some("conv_001".to_string());
    repo.save(&test_message).await.unwrap();

    let recall_operation = MessageOperation {
        operation_type: OperationType::Recall,
        target_message_id: "test_msg_001".to_string(),
        operator_id: "user_001".to_string(),
        timestamp: Utc::now(),
        show_notice: true,
        notice_text: Some("Message recalled".to_string()),
        target_user_id: None,
        operation_data: OperationData::Recall {
            reason: Some("Wrong message".to_string()),
            time_limit_seconds: Some(300),
            allow_admin_recall: true,
        },
        metadata: std::collections::HashMap::new(),
    };

    let loaded = repo.find_by_id("test_msg_001").await.unwrap().unwrap();
    assert!(MessageOperationFSM::can_apply_operation(&loaded, &recall_operation).unwrap());

    let recall_command = MessageOperationCommand {
        operation: recall_operation.clone(),
        conversation_id: "conv_001".to_string(),
    };
    let result = command_handler.execute(recall_command).await;
    assert!(result.is_ok());

    let updated = repo.find_by_id("test_msg_001").await.unwrap().unwrap();
    assert!(updated.is_recalled);

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    let events = collected.lock().await;
    assert!(!events.is_empty());
    assert!(events.iter().any(|e| e.event_type == "MessageOperationApplied"));
    drop(events);

    // 编辑已撤回消息应被 FSM 拒绝
    let edit_operation = MessageOperation {
        operation_type: OperationType::Edit,
        target_message_id: "test_msg_001".to_string(),
        operator_id: "user_001".to_string(),
        timestamp: Utc::now(),
        show_notice: true,
        notice_text: Some("Message edited".to_string()),
        target_user_id: None,
        operation_data: OperationData::Edit {
            new_content: b"Edited content".to_vec(),
            edit_version: 1,
            reason: Some("Correction".to_string()),
            show_edited_mark: true,
        },
        metadata: std::collections::HashMap::new(),
    };
    let loaded2 = repo.find_by_id("test_msg_001").await.unwrap().unwrap();
    assert!(!MessageOperationFSM::can_apply_operation(&loaded2, &edit_operation).unwrap());

    // 新消息：反应、置顶、标记
    let mut fresh_message = Message::new(
        Some("fresh_msg_001".to_string()),
        "client_fresh_001".to_string(),
        "user_001".to_string(),
        MessageType::Text,
        b"Fresh message".to_vec(),
    );
    fresh_message.conversation_id = Some("conv_001".to_string());
    repo.save(&fresh_message).await.unwrap();

    let reaction_operation = MessageOperation {
        operation_type: OperationType::ReactionAdd,
        target_message_id: "fresh_msg_001".to_string(),
        operator_id: "user_002".to_string(),
        timestamp: Utc::now(),
        show_notice: true,
        notice_text: Some("Reaction added".to_string()),
        target_user_id: None,
        operation_data: OperationData::Reaction {
            emoji: "👍".to_string(),
            action: ReactionAction::Add,
            count: 1,
        },
        metadata: std::collections::HashMap::new(),
    };
    let reaction_command = MessageOperationCommand {
        operation: reaction_operation,
        conversation_id: "conv_001".to_string(),
    };
    assert!(command_handler.execute(reaction_command).await.is_ok());

    let with_reaction = repo.find_by_id("fresh_msg_001").await.unwrap().unwrap();
    assert_eq!(with_reaction.reactions.len(), 1);
    assert_eq!(with_reaction.reactions[0].emoji, "👍");

    let pin_operation = MessageOperation {
        operation_type: OperationType::Pin,
        target_message_id: "fresh_msg_001".to_string(),
        operator_id: "user_001".to_string(),
        timestamp: Utc::now(),
        show_notice: true,
        notice_text: Some("Message pinned".to_string()),
        target_user_id: None,
        operation_data: OperationData::Pin {
            reason: Some("Important message".to_string()),
            expire_at: None,
        },
        metadata: std::collections::HashMap::new(),
    };
    let pin_command = MessageOperationCommand {
        operation: pin_operation,
        conversation_id: "conv_001".to_string(),
    };
    assert!(command_handler.execute(pin_command).await.is_ok());

    let pinned = repo.find_by_id("fresh_msg_001").await.unwrap().unwrap();
    assert_eq!(pinned.attributes.get("pinned"), Some(&"true".to_string()));

    let mark_operation = MessageOperation {
        operation_type: OperationType::Mark,
        target_message_id: "fresh_msg_001".to_string(),
        operator_id: "user_001".to_string(),
        timestamp: Utc::now(),
        show_notice: true,
        notice_text: Some("Message marked".to_string()),
        target_user_id: None,
        operation_data: OperationData::Mark {
            mark_type: MarkType::Important,
            color: Some("#FF0000".to_string()),
        },
        metadata: std::collections::HashMap::new(),
    };
    let mark_command = MessageOperationCommand {
        operation: mark_operation,
        conversation_id: "conv_001".to_string(),
    };
    assert!(command_handler.execute(mark_command).await.is_ok());

    let marked = repo.find_by_id("fresh_msg_001").await.unwrap().unwrap();
    assert_eq!(marked.attributes.get("mark_type_Important"), Some(&"true".to_string()));
}

// ----- Event 往返（领域 <-> proto Event） -----

#[tokio::test]
async fn test_protobuf_conversion_roundtrip() {
    let original_operation = MessageOperation {
        operation_type: OperationType::Recall,
        target_message_id: "test_msg_001".to_string(),
        operator_id: "user_001".to_string(),
        timestamp: Utc::now(),
        show_notice: true,
        notice_text: Some("Recall test".to_string()),
        target_user_id: Some("target_user".to_string()),
        operation_data: OperationData::Recall {
            reason: Some("Test recall".to_string()),
            time_limit_seconds: Some(300),
            allow_admin_recall: true,
        },
        metadata: std::collections::HashMap::from([
            ("key1".to_string(), "value1".to_string()),
            ("key2".to_string(), "value2".to_string()),
        ]),
    };

    let event = MessageOperationConverter::to_proto(&original_operation).unwrap();
    let converted_operation = MessageOperationConverter::from_proto(&event).unwrap();

    assert_eq!(original_operation.operation_type, converted_operation.operation_type);
    assert_eq!(original_operation.target_message_id, converted_operation.target_message_id);
    assert_eq!(original_operation.operator_id, converted_operation.operator_id);
    // show_notice/notice_text/target_user_id 不在 proto Event 中，往返后为默认值

    match (&original_operation.operation_data, &converted_operation.operation_data) {
        (
            OperationData::Recall {
                reason: r1,
                time_limit_seconds: t1,
                allow_admin_recall: a1,
            },
            OperationData::Recall {
                reason: r2,
                time_limit_seconds: t2,
                allow_admin_recall: a2,
            },
        ) => {
            assert_eq!(r1, r2);
            assert_eq!(t1, t2);
            assert_eq!(a1, a2);
        }
        _ => panic!("OperationData not correctly converted"),
    }
}

// ----- 状态机 -----

#[tokio::test]
async fn test_operation_state_machine() {
    let mut message = Message::new(
        Some("state_msg_001".to_string()),
        "client_state_001".to_string(),
        "user_001".to_string(),
        MessageType::Text,
        b"State test message".to_vec(),
    );
    message.conversation_id = Some("conv_001".to_string());

    let recall_op = MessageOperation {
        operation_type: OperationType::Recall,
        target_message_id: "state_msg_001".to_string(),
        operator_id: "user_001".to_string(),
        timestamp: Utc::now(),
        show_notice: true,
        notice_text: None,
        target_user_id: None,
        operation_data: OperationData::Recall {
            reason: Some("Test".to_string()),
            time_limit_seconds: Some(300),
            allow_admin_recall: true,
        },
        metadata: std::collections::HashMap::new(),
    };

    assert!(MessageOperationFSM::can_apply_operation(&message, &recall_op).unwrap());
    message
        .recall("user_001".to_string(), Some("Test".to_string()))
        .unwrap();
    assert!(!MessageOperationFSM::can_apply_operation(&message, &recall_op).unwrap());

    let edit_op = MessageOperation {
        operation_type: OperationType::Edit,
        target_message_id: "state_msg_001".to_string(),
        operator_id: "user_001".to_string(),
        timestamp: Utc::now(),
        show_notice: true,
        notice_text: None,
        target_user_id: None,
        operation_data: OperationData::Edit {
            new_content: b"Edited".to_vec(),
            edit_version: 1,
            reason: Some("Test edit".to_string()),
            show_edited_mark: true,
        },
        metadata: std::collections::HashMap::new(),
    };
    assert!(!MessageOperationFSM::can_apply_operation(&message, &edit_op).unwrap());

    let reaction_op = MessageOperation {
        operation_type: OperationType::ReactionAdd,
        target_message_id: "state_msg_001".to_string(),
        operator_id: "user_002".to_string(),
        timestamp: Utc::now(),
        show_notice: true,
        notice_text: None,
        target_user_id: None,
        operation_data: OperationData::Reaction {
            emoji: "👍".to_string(),
            action: ReactionAction::Add,
            count: 1,
        },
        metadata: std::collections::HashMap::new(),
    };
    assert!(!MessageOperationFSM::can_apply_operation(&message, &reaction_op).unwrap());
}

// ----- 多种操作类型 -----

#[tokio::test]
async fn test_multiple_operation_types() {
    let repo = Arc::new(TestMessageRepository::new());
    let event_bus = Arc::new(EventBus::new(100));
    let command_handler = MessageOperationCommandHandler::new(repo.clone(), event_bus.clone());

    let mut test_message = Message::new(
        Some("multi_op_msg_001".to_string()),
        "client_multi_001".to_string(),
        "user_001".to_string(),
        MessageType::Text,
        b"Multi-operation test".to_vec(),
    );
    test_message.conversation_id = Some("conv_multi_001".to_string());
    repo.save(&test_message).await.unwrap();

    let reaction_op = MessageOperation {
        operation_type: OperationType::ReactionAdd,
        target_message_id: "multi_op_msg_001".to_string(),
        operator_id: "user_002".to_string(),
        timestamp: Utc::now(),
        show_notice: true,
        notice_text: Some("Reaction added".to_string()),
        target_user_id: None,
        operation_data: OperationData::Reaction {
            emoji: "❤️".to_string(),
            action: ReactionAction::Add,
            count: 1,
        },
        metadata: std::collections::HashMap::new(),
    };
    assert!(command_handler
        .execute(MessageOperationCommand {
            operation: reaction_op,
            conversation_id: "conv_multi_001".to_string(),
        })
        .await
        .is_ok());

    let mark_op = MessageOperation {
        operation_type: OperationType::Mark,
        target_message_id: "multi_op_msg_001".to_string(),
        operator_id: "user_001".to_string(),
        timestamp: Utc::now(),
        show_notice: true,
        notice_text: Some("Marked as important".to_string()),
        target_user_id: None,
        operation_data: OperationData::Mark {
            mark_type: MarkType::Important,
            color: Some("#FFD700".to_string()),
        },
        metadata: std::collections::HashMap::new(),
    };
    assert!(command_handler
        .execute(MessageOperationCommand {
            operation: mark_op,
            conversation_id: "conv_multi_001".to_string(),
        })
        .await
        .is_ok());

    let pin_op = MessageOperation {
        operation_type: OperationType::Pin,
        target_message_id: "multi_op_msg_001".to_string(),
        operator_id: "admin_user".to_string(),
        timestamp: Utc::now(),
        show_notice: true,
        notice_text: Some("Pinned".to_string()),
        target_user_id: None,
        operation_data: OperationData::Pin {
            reason: Some("Important announcement".to_string()),
            expire_at: None,
        },
        metadata: std::collections::HashMap::new(),
    };
    assert!(command_handler
        .execute(MessageOperationCommand {
            operation: pin_op,
            conversation_id: "conv_multi_001".to_string(),
        })
        .await
        .is_ok());

    let final_message = repo.find_by_id("multi_op_msg_001").await.unwrap().unwrap();
    assert_eq!(final_message.reactions.len(), 1);
    assert_eq!(final_message.reactions[0].emoji, "❤️");
    assert_eq!(
        final_message.attributes.get("mark_type_Important"),
        Some(&"true".to_string())
    );
    assert_eq!(final_message.attributes.get("pinned"), Some(&"true".to_string()));
}
