//! 消息事件转发
//!
//! 将 SDK 消息事件自动转发到 Tauri 前端

use crate::state::SdkState;
use crate::utils::ensure_message_content_text;
use tauri::{AppHandle, Emitter, Manager};
use flare_im_core_sdk::{
    interface::event::MessageEventSubscriber,
    domain::event::*,
    application::queries::GetMessageQuery,
};
use anyhow::Result as AnyhowResult;

/// 消息事件订阅器（转发到前端）
pub struct MessageEventForwarder {
    app: AppHandle,
}

impl MessageEventForwarder {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
    
    /// 查询消息并转发（带重试机制）
    async fn query_and_forward_message(
        app: AppHandle,
        message_id: String,
        max_retries: usize,
    ) {
        if message_id.trim().is_empty() {
            eprintln!("[MessageEventForwarder] Skipping query: message_id is empty");
            return;
        }
        // 添加小延迟，确保消息已保存到 ReadStore
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        // 安全地获取 SDK 状态
        let sdk_guard = match app.try_state::<SdkState>() {
            Some(state) => state,
            None => {
                eprintln!("[MessageEventForwarder] SDK state not available");
                return;
            }
        };
        
        let sdk_opt = sdk_guard.get_sdk().await;
        let sdk = match sdk_opt.as_ref() {
            Some(sdk) => sdk,
            None => {
                eprintln!("[MessageEventForwarder] SDK not initialized");
                return;
            }
        };
        
        // 重试查询消息
        let mut retry_count = 0;
        let mut message_found = false;
        
        while retry_count < max_retries && !message_found {
            let query = GetMessageQuery {
                message_id: message_id.clone(),
            };
            
            match sdk.sdk_context().query_handler.get_message(query).await {
                Ok(mut msg) => {
                    // 确保 extra.content_text 存在
                    ensure_message_content_text(&mut msg);
                    
                    // 发送完整的消息对象
                    let _ = app.emit("im://message", &msg);
                    message_found = true;
                    break;
                }
                Err(e) => {
                    let id_display = if message_id.is_empty() { "(empty id)" } else { message_id.as_str() };
                    eprintln!("[MessageEventForwarder] Failed to query message {} (retry {}/{}): {}", 
                        id_display, retry_count + 1, max_retries, e);
                }
            }
            
            if !message_found && retry_count < max_retries - 1 {
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            }
            
            retry_count += 1;
        }
        
        if !message_found {
            let id_display = if message_id.is_empty() { "(empty id)" } else { message_id.as_str() };
            eprintln!("[MessageEventForwarder] WARNING: Message {} not found after {} retries", 
                id_display, max_retries);
        }
    }
}

#[async_trait::async_trait]
impl MessageEventSubscriber for MessageEventForwarder {
    async fn on_message_created(&self, event: &MessageCreated) -> AnyhowResult<()> {
        let app = self.app.clone();
        let message_id = event.message_id.clone();
        
        tokio::spawn(async move {
            Self::query_and_forward_message(app, message_id, 3).await;
        });
        
        Ok(())
    }

    async fn on_message_sent(&self, event: &MessageSent) -> AnyhowResult<()> {
        let app = self.app.clone();
        let message_id = event.message_id.clone();
        
        tokio::spawn(async move {
            Self::query_and_forward_message(app, message_id, 3).await;
        });
        
        Ok(())
    }

    async fn on_message_send_failed(&self, event: &MessageSendFailed) -> AnyhowResult<()> {
        let app = self.app.clone();
        let message_id = event.message_id.clone();
        
        tokio::spawn(async move {
            Self::query_and_forward_message(app, message_id, 3).await;
        });
        
        Ok(())
    }

    async fn on_message_delivered(&self, event: &MessageDelivered) -> AnyhowResult<()> {
        let _ = self.app.emit("im://message_delivered", serde_json::json!({
            "message_id": event.message_id,
        }));
        Ok(())
    }

    async fn on_message_read(&self, event: &MessageRead) -> AnyhowResult<()> {
        let _ = self.app.emit("im://message_read", serde_json::json!({
            "message_id": event.message_id,
            "reader_id": event.reader_id,
        }));
        Ok(())
    }

    async fn on_message_recalled(&self, event: &MessageRecalled) -> AnyhowResult<()> {
        let app = self.app.clone();
        let message_id = event.message_id.clone();
        
        // 重新查询消息并转发（包含更新后的 is_recalled 状态）
        tokio::spawn(async move {
            Self::query_and_forward_message(app, message_id, 3).await;
        });

        let _ = self.app.emit("im://message_recalled", serde_json::json!({
            "message_id": event.message_id,
            "recaller_id": event.recaller_id,
        }));
        Ok(())
    }

    async fn on_message_edited(&self, event: &MessageEdited) -> AnyhowResult<()> {
        let app = self.app.clone();
        let message_id = event.message_id.clone();
        let event_clone = event.clone();
        
        // 重新查询消息并转发
        tokio::spawn(async move {
            Self::query_and_forward_message(app.clone(), message_id.clone(), 3).await;
        });
        
        // 同时发送编辑事件（用于前端特殊处理）
        let _ = self.app.emit("im://message_edited", serde_json::json!({
            "message_id": event_clone.message_id,
            "editor_id": event_clone.editor_id,
            "new_content": event_clone.new_content,
        }));
        
        Ok(())
    }

    async fn on_message_deleted(&self, event: &MessageDeleted) -> AnyhowResult<()> {
        let _ = self.app.emit("im://message_deleted", serde_json::json!({
            "message_id": event.message_id,
            "operator_id": event.operator_id,
            "delete_type": event.delete_type,
        }));
        Ok(())
    }

    async fn on_message_reaction_added(&self, event: &MessageReactionAdded) -> AnyhowResult<()> {
        let app = self.app.clone();
        let message_id = event.message_id.clone();
        
        // 重新查询消息并转发（包含更新后的 reactions）
        tokio::spawn(async move {
            Self::query_and_forward_message(app, message_id, 3).await;
        });
        
        Ok(())
    }

    async fn on_message_reaction_removed(&self, event: &MessageReactionRemoved) -> AnyhowResult<()> {
        let app = self.app.clone();
        let message_id = event.message_id.clone();
        
        // 重新查询消息并转发（reaction 已移除）
        tokio::spawn(async move {
            Self::query_and_forward_message(app, message_id, 3).await;
        });
        
        Ok(())
    }

    async fn on_message_pinned(&self, event: &MessagePinned) -> AnyhowResult<()> {
        let _ = self.app.emit("im://message_pinned", serde_json::json!({
            "message_id": event.message_id,
            "operator_id": event.operator_id,
        }));
        Ok(())
    }

    async fn on_message_unpinned(&self, event: &MessageUnpinned) -> AnyhowResult<()> {
        let _ = self.app.emit("im://message_unpinned", serde_json::json!({
            "message_id": event.message_id,
            "operator_id": event.operator_id,
        }));
        Ok(())
    }

    async fn on_message_favorited(&self, event: &MessageFavorited) -> AnyhowResult<()> {
        let _ = self.app.emit("im://message_favorited", serde_json::json!({
            "message_id": event.message_id,
            "user_id": event.user_id,
        }));
        Ok(())
    }

    async fn on_message_unfavorited(&self, event: &MessageUnfavorited) -> AnyhowResult<()> {
        let _ = self.app.emit("im://message_unfavorited", serde_json::json!({
            "message_id": event.message_id,
            "user_id": event.user_id,
        }));
        Ok(())
    }

    async fn on_message_marked(&self, event: &MessageMarked) -> AnyhowResult<()> {
        let _ = self.app.emit("im://message_marked", serde_json::json!({
            "message_id": event.message_id,
            "user_id": event.user_id,
            "mark_type": event.mark_type,
        }));
        Ok(())
    }

    async fn on_message_unmarked(&self, event: &MessageUnmarked) -> AnyhowResult<()> {
        let _ = self.app.emit("im://message_unmarked", serde_json::json!({
            "message_id": event.message_id,
        }));
        Ok(())
    }

    async fn on_message_forwarded(&self, event: &MessageForwarded) -> AnyhowResult<()> {
        let _ = self.app.emit("im://message_forwarded", serde_json::json!({
            "message_id": event.message_id,
            "target_conversation_id": event.target_conversation_id,
        }));
        Ok(())
    }

    async fn on_message_replied(&self, event: &MessageReplied) -> AnyhowResult<()> {
        let _ = self.app.emit("im://message_replied", serde_json::json!({
            "message_id": event.message_id,
            "reply_to_message_id": event.quoted_message_id,
        }));
        Ok(())
    }
}
