//! 命令接收器
//!
//! 处理服务端推送的命令（如消息撤回通知、已读回执等）
//!
//! ## 职责
//!
//! 1. 接收服务端推送的命令（从 infrastructure 层调用）
//! 2. 解析命令类型
//! 3. 分发到对应的 CommandHandler 处理
//! 4. 发布领域事件

use crate::application::handlers::{MessageCommandHandler, SessionCommandHandler};
use anyhow::Result;
use std::sync::Arc;

/// 命令接收器
///
/// 处理从服务端推送的命令
pub struct CommandReceiver {
    message_command_handler: Arc<MessageCommandHandler>,
    session_command_handler: Arc<SessionCommandHandler>,
}

impl CommandReceiver {
    pub fn new(
        message_command_handler: Arc<MessageCommandHandler>,
        session_command_handler: Arc<SessionCommandHandler>,
    ) -> Self {
        Self {
            message_command_handler,
            session_command_handler,
        }
    }

    /// 处理消息撤回通知
    pub async fn handle_recall_notification(
        &self,
        message_id: String,
        user_id: String,
    ) -> Result<()> {
        use crate::application::commands::message::RecallMessageCommand;
        use crate::domain::{MessageId, UserId};

        self.message_command_handler
            .handle_recall_message(RecallMessageCommand {
                message_id: MessageId::new(message_id),
                user_id: UserId::new(user_id),
                reason: None,
            })
            .await
    }

    /// 处理已读回执
    pub async fn handle_read_receipt(
        &self,
        message_id: String,
        user_id: String,
        session_id: String,
    ) -> Result<()> {
        use crate::application::commands::session::MarkReadCommand;
        use crate::domain::{MessageId, SessionId};

        // 标记会话已读
        self.session_command_handler
            .handle_mark_read(MarkReadCommand {
                session_id: SessionId::new(session_id),
                message_seq: None, // 已读回执通常不指定具体 seq，表示已读所有消息
            })
            .await
    }

    /// 处理会话更新通知
    pub async fn handle_session_update_notification(
        &self,
        session_id: String,
        updates: std::collections::HashMap<String, String>,
    ) -> Result<()> {
        use crate::application::commands::session::UpdateSessionCommand;
        use crate::domain::SessionId;

        self.session_command_handler
            .handle_update_session(UpdateSessionCommand {
                session_id: SessionId::new(session_id),
                updates,
            })
            .await
    }

    /// 处理其他服务端命令
    ///
    /// 按照微信/Telegram/飞书标准：根据命令类型分发到不同的处理器
    pub async fn handle_command(&self, command_type: &str, data: &[u8]) -> Result<()> {
        use crate::domain::{MessageId, SessionId, UserId};

        tracing::debug!(command_type = %command_type, "Received command from server");

        // 根据命令类型分发到不同的处理器
        match command_type {
            "recall" => {
                // 消息撤回通知
                if let Ok(message_id) = String::from_utf8(data.to_vec()) {
                    // 从 metadata 或其他地方获取 user_id
                    // 这里简化处理，实际应该从命令数据中解析
                    let user_id = "system".to_string(); // 占位符
                    self.handle_recall_notification(message_id, user_id).await?;
                }
            }
            "read_receipt" => {
                // 已读回执
                // 解析 data 获取 message_id, user_id, session_id
                // 这里简化处理，实际应该解析 protobuf 或其他格式
                if let Ok(data_str) = String::from_utf8(data.to_vec()) {
                    // 假设格式为 "message_id:user_id:session_id"
                    let parts: Vec<&str> = data_str.split(':').collect();
                    if parts.len() >= 3 {
                        self.handle_read_receipt(
                            parts[0].to_string(),
                            parts[1].to_string(),
                            parts[2].to_string(),
                        )
                        .await?;
                    }
                }
            }
            "session_update" => {
                // 会话更新通知
                // 解析 data 获取 session_id 和 updates
                // 这里简化处理，实际应该解析 protobuf
                if let Ok(data_str) = String::from_utf8(data.to_vec()) {
                    // 假设格式为 "session_id:key1=value1:key2=value2"
                    let parts: Vec<&str> = data_str.split(':').collect();
                    if !parts.is_empty() {
                        let session_id = parts[0].to_string();
                        let mut updates = std::collections::HashMap::new();
                        for part in parts.iter().skip(1) {
                            if let Some((key, value)) = part.split_once('=') {
                                updates.insert(key.to_string(), value.to_string());
                            }
                        }
                        self.handle_session_update_notification(session_id, updates)
                            .await?;
                    }
                }
            }
            _ => {
                tracing::warn!(command_type = %command_type, "Unknown command type");
            }
        }

        Ok(())
    }
}
