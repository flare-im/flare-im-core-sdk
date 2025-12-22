//! 消息命令处理器
//!
//! 职责：编排消息相关的写操作，调用领域服务处理业务逻辑

use std::sync::Arc;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use crate::application::fsm::FsmManager;
use crate::domain::repository::{EventStore, ReadStore};
use crate::domain::message::{Message, MessageType};
use crate::domain::service::MessageDomainService;
use crate::infrastructure::messaging::MessageSender;
use crate::infrastructure::storage::media_cache::MediaCacheManager;
use crate::infrastructure::event_bus::EventBus;
use crate::infrastructure::metrics;
use crate::application::commands::*;

/// 重试消息项
#[derive(Debug, Clone)]
struct RetryMessage {
    message: Message,
    retry_count: u32,
    max_retries: u32,
    last_error: Option<String>,
}

impl RetryMessage {
    fn new(message: Message, max_retries: u32) -> Self {
        Self {
            message,
            retry_count: 0,
            max_retries,
            last_error: None,
        }
    }
    
    fn can_retry(&self) -> bool {
        self.retry_count < self.max_retries
    }
}

/// 消息命令处理器
pub struct MessageCommandHandler {
    fsm: Arc<FsmManager>,
    event_store: Arc<dyn EventStore>,
    read_store: Arc<dyn ReadStore>,
    message_sender: Arc<MessageSender>,
    media_cache: Arc<MediaCacheManager>,
    event_bus: Arc<EventBus>,
    domain_service: MessageDomainService,
    retry_queue: Arc<Mutex<HashMap<String, RetryMessage>>>,
}

impl MessageCommandHandler {
    pub fn new(
        fsm: Arc<FsmManager>,
        event_store: Arc<dyn EventStore>,
        read_store: Arc<dyn ReadStore>,
        message_sender: Arc<MessageSender>,
        media_cache: Arc<MediaCacheManager>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            fsm,
            event_store,
            read_store,
            message_sender,
            media_cache,
            event_bus,
            domain_service: MessageDomainService::new(),
            retry_queue: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    /// 处理发送消息命令
    pub async fn handle(&self, cmd: SendMessageCommand) -> anyhow::Result<()> {
        self.send_message_internal(cmd.message, false).await
    }
    
    /// 内部发送消息方法（支持重试）
    pub(crate) async fn send_message_internal(&self, mut message: Message, is_retry: bool) -> anyhow::Result<()> {
        // 1. 处理媒体消息的本地缓存
        if self.is_media_message(&message) {
            self.prepare_media_message(&mut message).await?;
        }
        
        // 2. 发布 MessageCreated 事件（消息创建时）
        self.publish_message_created_event(&message).await?;
        
        // 3. 通过 FSM 开始发送（领域层状态管理）
        self.fsm.message_start_sending(&mut message, is_retry).await?;
        
        // 3. 发送消息并等待 ACK（基础设施层）
        let send_start = Instant::now();
        let timeout = Duration::from_secs(30);
        let send_result = self.message_sender.send_message_and_wait_ack(&message, timeout).await;
        
        match send_result {
            Ok(result) => {
                let latency_ms = send_start.elapsed().as_millis() as u64;
                
                use flare_proto::common::AckStatus;
                match result.status {
                    AckStatus::Success => {
                        metrics::record_message_send(true, latency_ms).await;
                        self.fsm.message_send_success(&mut message, result.seq).await?;
                        
                        tracing::info!(
                            message_id = %result.message_id,
                            seq = result.seq,
                            latency_ms = latency_ms,
                            "✅ 消息发送成功"
                        );
                        
                        self.publish_message_sent_event(&message, is_retry).await?;
                        Ok(())
                    }
                    AckStatus::Failed => {
                        metrics::record_message_send(false, latency_ms).await;
                        let error_msg = if !result.error_message.is_empty() {
                            result.error_message
                        } else {
                            format!("服务器返回错误码: {}", result.error_code)
                        };
                        self.handle_send_failure(message, result.error_code, error_msg, latency_ms).await
                    }
                    _ => {
                        self.fsm.message_send_success(&mut message, result.seq).await?;
                        self.publish_message_sent_event(&message, is_retry).await?;
                        Ok(())
                    }
                }
            }
            Err(e) => {
                let latency_ms = send_start.elapsed().as_millis() as u64;
                metrics::record_message_send(false, latency_ms).await;
                
                if e.to_string().contains("timeout") {
                    metrics::record_ack_timeout().await;
                }
                
                self.handle_network_error(message, e, latency_ms).await
            }
        }
    }
    
    /// 处理撤回消息命令
    pub async fn handle_recall(&self, cmd: RecallMessageCommand) -> anyhow::Result<()> {
        // 从 ReadStore 加载消息
        let mut message = self.load_message(&cmd.message_id).await?
            .ok_or_else(|| anyhow::anyhow!("Message not found: {}", cmd.message_id))?;
        
        // 使用领域服务验证是否可以撤回
        let can_recall = self.domain_service.can_recall(&message, &cmd.recaller_id, Some(120))?;
        if !can_recall {
            return Err(anyhow::anyhow!("Message cannot be recalled"));
        }
        
        // 使用 Message 的领域方法处理撤回
        message.recall(cmd.recaller_id.clone(), cmd.reason.clone())?;
        
        // 保存消息
        self.save_message(&message).await?;
        
        // 发布领域事件
        self.publish_message_event(message_events::RECALLED, message, None).await?;
        
        Ok(())
    }
    
    /// 处理编辑消息命令
    pub async fn handle_edit(&self, cmd: EditMessageCommand) -> anyhow::Result<()> {
        let mut message = self.load_message(&cmd.message_id).await?
            .ok_or_else(|| anyhow::anyhow!("Message not found: {}", cmd.message_id))?;
        
        // 使用领域服务验证是否可以编辑
        let can_edit = self.domain_service.can_edit(&message, &cmd.editor_id)?;
        if !can_edit {
            return Err(anyhow::anyhow!("Message cannot be edited"));
        }
        
        // 使用 Message 的领域方法处理编辑
        message.edit(cmd.new_content, cmd.editor_id.clone(), cmd.reason.clone())?;
        
        self.save_message(&message).await?;
        self.publish_message_event(message_events::EDITED, message, None).await?;
        
        Ok(())
    }
    
    /// 处理删除消息命令
    pub async fn handle_delete(&self, cmd: DeleteMessageCommand) -> anyhow::Result<()> {
        let mut message = self.load_message(&cmd.message_id).await?
            .ok_or_else(|| anyhow::anyhow!("Message not found: {}", cmd.message_id))?;
        
        // 使用领域服务验证是否可以删除
        let can_delete = self.domain_service.can_delete(&message, &cmd.operator_id, cmd.delete_type)?;
        if !can_delete {
            return Err(anyhow::anyhow!("Message cannot be deleted"));
        }
        
        // 使用领域服务处理删除（通过 apply_operation）
        use crate::domain::message::{MessageOperation, OperationType, OperationData};
        let operation = MessageOperation {
            operation_type: OperationType::Delete,
            target_message_id: cmd.message_id.clone(),
            operator_id: cmd.operator_id.clone(),
            timestamp: chrono::Utc::now(),
            show_notice: false,
            notice_text: None,
            target_user_id: None,
            operation_data: OperationData::Delete {
                delete_type: cmd.delete_type,
                reason: cmd.reason.clone(),
                notify_others: false,
            },
            metadata: std::collections::HashMap::new(),
        };
        self.domain_service.apply_operation(operation, &mut message)?;
        
        self.save_message(&message).await?;
        self.publish_message_event(message_events::DELETED, message, None).await?;
        
        Ok(())
    }
    
    /// 处理标记消息已读命令
    pub async fn handle_mark_read(&self, cmd: MarkMessagesReadCommand) -> anyhow::Result<()> {
        // 使用 Message 的领域方法处理标记已读
        for message_id in &cmd.message_ids {
            if let Some(mut message) = self.load_message(message_id).await? {
                // 使用 Message 的 mark_read 方法
                message.mark_read(cmd.user_id.clone())?;
                
                // 如果是阅后即焚，设置销毁时间
                if cmd.burn_after_read {
                    // 使用领域服务计算过期时间
                    let burn_seconds = message.burn_after_seconds.unwrap_or(60); // 默认 60 秒
                    let expire_at = self.domain_service.calculate_expire_at(&message, burn_seconds);
                    // 在消息的 extra 中记录过期时间
                    message.extra.insert("burn_expire_at".to_string(), expire_at.to_rfc3339());
                }
                
                self.save_message(&message).await?;
            }
        }
        
        Ok(())
    }
    
    /// 处理回复消息命令
    pub async fn handle_reply(&self, cmd: ReplyMessageCommand) -> anyhow::Result<String> {
        // 加载被回复的消息
        let original_message = self.load_message(&cmd.reply_to_message_id).await?
            .ok_or_else(|| anyhow::anyhow!("Original message not found: {}", cmd.reply_to_message_id))?;
        
        // 使用 build_text_message 创建回复消息（暂时使用文本消息）
        use crate::domain::message::build_text_message;
        let mut reply_message = build_text_message(
            cmd.conversation_id,
            cmd.sender_id,
            String::from_utf8_lossy(&cmd.reply_content).to_string(),
            cmd.tenant,
            None, // receiver_id 暂时为 None
        )?;
        
        // 设置 reply_to_message_id 到消息的 extra 字段
        reply_message.extra.insert("reply_to_message_id".to_string(), cmd.reply_to_message_id);
        
        // 发送回复消息
        self.send_message_internal(reply_message.clone(), false).await?;
        
        Ok(reply_message.id)
    }
    
    /// 处理转发消息命令
    pub async fn handle_forward(&self, cmd: ForwardMessagesCommand) -> anyhow::Result<Vec<String>> {
        // 使用领域服务创建转发消息
        let forwarded_message = if cmd.merge_forward {
            // 合并转发：创建一条包含多条消息的转发消息
            self.domain_service.create_forward_message(
                cmd.target_conversation_id.clone(),
                cmd.sender_id.clone(),
                cmd.message_ids.clone(),
                None, // forward_reason
                cmd.tenant.clone(),
            )?
        } else {
            // 逐条转发：为每条消息创建一条转发消息
            if cmd.message_ids.is_empty() {
                return Err(anyhow::anyhow!("No messages to forward"));
            }
            
            // 为每条消息创建转发消息
            let mut forwarded_ids = Vec::new();
            for message_id in &cmd.message_ids {
                let forwarded_message = self.domain_service.create_forward_message(
                    cmd.target_conversation_id.clone(),
                    cmd.sender_id.clone(),
                    vec![message_id.clone()],
                    None,
                    cmd.tenant.clone(),
                )?;
                
                // 发送转发消息
                self.send_message_internal(forwarded_message.clone(), false).await?;
                forwarded_ids.push(forwarded_message.id);
            }
            
            return Ok(forwarded_ids);
        };
        
        // 发送转发消息
        self.send_message_internal(forwarded_message.clone(), false).await?;
        
        Ok(vec![forwarded_message.id])
    }
    
    /// 处理添加反应命令
    pub async fn handle_add_reaction(&self, cmd: AddReactionCommand) -> anyhow::Result<()> {
        let mut message = self.load_message(&cmd.message_id).await?
            .ok_or_else(|| anyhow::anyhow!("Message not found: {}", cmd.message_id))?;
        
        // 使用领域服务验证是否可以添加反应
        let can_add = self.domain_service.can_add_reaction(&message, &cmd.user_id)?;
        if !can_add {
            return Err(anyhow::anyhow!("Cannot add reaction to this message"));
        }
        
        // 使用 Message 的领域方法处理添加反应
        message.add_reaction(cmd.emoji.clone(), cmd.user_id.clone());
        
        self.save_message(&message).await?;
        self.publish_message_event(message_events::REACTION_ADDED, message, None).await?;
        
        Ok(())
    }
    
    /// 处理移除反应命令
    pub async fn handle_remove_reaction(&self, cmd: RemoveReactionCommand) -> anyhow::Result<()> {
        let mut message = self.load_message(&cmd.message_id).await?
            .ok_or_else(|| anyhow::anyhow!("Message not found: {}", cmd.message_id))?;
        
        // 使用 Message 的领域方法处理移除反应
        message.remove_reaction(cmd.emoji.clone(), cmd.user_id.clone());
        
        self.save_message(&message).await?;
        self.publish_message_event(message_events::REACTION_REMOVED, message, None).await?;
        
        Ok(())
    }
    
    /// 处理引用消息命令
    pub async fn handle_quote(&self, cmd: QuoteMessageCommand) -> anyhow::Result<String> {
        // 加载被引用的消息
        let quoted_message = self.load_message(&cmd.quoted_message_id).await?
            .ok_or_else(|| anyhow::anyhow!("Quoted message not found: {}", cmd.quoted_message_id))?;
        
        // 生成被引用消息的预览文本
        let quoted_text_preview = self.domain_service.generate_preview(&quoted_message);
        
        // 使用领域服务创建引用消息
        let quote_message = self.domain_service.create_quote_message(
            cmd.conversation_id.clone(),
            cmd.sender_id.clone(),
            cmd.quoted_message_id.clone(),
            quoted_message.sender_id.clone(),
            quoted_text_preview,
            cmd.reply_content.clone(),
            cmd.tenant.clone(),
        )?;
        
        self.send_message_internal(quote_message.clone(), false).await?;
        
        Ok(quote_message.id)
    }
    
    /// 处理置顶消息命令
    pub async fn handle_pin(&self, cmd: PinMessageCommand) -> anyhow::Result<()> {
        let mut message = self.load_message(&cmd.message_id).await?
            .ok_or_else(|| anyhow::anyhow!("Message not found: {}", cmd.message_id))?;
        
        // 使用领域服务验证是否可以置顶
        let can_pin = self.domain_service.can_pin(&message, &cmd.operator_id)?;
        if !can_pin {
            return Err(anyhow::anyhow!("Cannot pin this message"));
        }
        
        // 在 extra 字段中标记为置顶
        message.extra.insert("is_pinned".to_string(), "true".to_string());
        if let Some(reason) = cmd.reason {
            message.extra.insert("pin_reason".to_string(), reason);
        }
        if let Some(expire_at) = cmd.expire_at {
            message.extra.insert("pin_expire_at".to_string(), expire_at.to_rfc3339());
        }
        message.version += 1;
        message.updated_at = chrono::Utc::now();
        
        self.save_message(&message).await?;
        self.publish_message_event(message_events::PINNED, message, None).await?;
        
        Ok(())
    }
    
    /// 处理取消置顶命令
    pub async fn handle_unpin(&self, cmd: UnpinMessageCommand) -> anyhow::Result<()> {
        let mut message = self.load_message(&cmd.message_id).await?
            .ok_or_else(|| anyhow::anyhow!("Message not found: {}", cmd.message_id))?;
        
        // 移除置顶标记
        message.extra.remove("is_pinned");
        message.extra.remove("pin_reason");
        message.extra.remove("pin_expire_at");
        message.version += 1;
        message.updated_at = chrono::Utc::now();
        
        self.save_message(&message).await?;
        self.publish_message_event(message_events::UNPINNED, message, None).await?;
        
        Ok(())
    }
    
    /// 处理收藏消息命令
    pub async fn handle_favorite(&self, cmd: FavoriteMessageCommand) -> anyhow::Result<()> {
        let mut message = self.load_message(&cmd.message_id).await?
            .ok_or_else(|| anyhow::anyhow!("Message not found: {}", cmd.message_id))?;
        
        // 使用领域服务验证是否可以收藏
        let can_favorite = self.domain_service.can_favorite(&message, &cmd.operator_id)?;
        if !can_favorite {
            return Err(anyhow::anyhow!("Cannot favorite this message"));
        }
        
        // 在 extra 字段中标记为收藏
        message.extra.insert("is_favorited".to_string(), "true".to_string());
        message.extra.insert("favorited_by".to_string(), cmd.operator_id.clone());
        if !cmd.tags.is_empty() {
            message.extra.insert("favorite_tags".to_string(), cmd.tags.join(","));
        }
        if let Some(note) = cmd.note {
            message.extra.insert("favorite_note".to_string(), note);
        }
        message.version += 1;
        message.updated_at = chrono::Utc::now();
        
        self.save_message(&message).await?;
        self.publish_message_event(message_events::FAVORITED, message, None).await?;
        
        Ok(())
    }
    
    /// 处理取消收藏命令
    pub async fn handle_unfavorite(&self, cmd: UnfavoriteMessageCommand) -> anyhow::Result<()> {
        let mut message = self.load_message(&cmd.message_id).await?
            .ok_or_else(|| anyhow::anyhow!("Message not found: {}", cmd.message_id))?;
        
        // 移除收藏标记
        message.extra.remove("is_favorited");
        message.extra.remove("favorited_by");
        message.extra.remove("favorite_tags");
        message.extra.remove("favorite_note");
        message.version += 1;
        message.updated_at = chrono::Utc::now();
        
        self.save_message(&message).await?;
        self.publish_message_event(message_events::UNFAVORITED, message, None).await?;
        
        Ok(())
    }
    
    /// 处理标记消息命令
    pub async fn handle_mark(&self, cmd: MarkMessageCommand) -> anyhow::Result<()> {
        let mut message = self.load_message(&cmd.message_id).await?
            .ok_or_else(|| anyhow::anyhow!("Message not found: {}", cmd.message_id))?;
        
        // 使用领域服务验证是否可以标记
        let can_mark = self.domain_service.can_mark(&message, &cmd.operator_id)?;
        if !can_mark {
            return Err(anyhow::anyhow!("Cannot mark this message"));
        }
        
        // 在 extra 字段中标记
        message.extra.insert("mark_type".to_string(), format!("{:?}", cmd.mark_type));
        message.extra.insert("marked_by".to_string(), cmd.operator_id.clone());
        if let Some(color) = cmd.color {
            message.extra.insert("mark_color".to_string(), color);
        }
        message.version += 1;
        message.updated_at = chrono::Utc::now();
        
        self.save_message(&message).await?;
        self.publish_message_event(message_events::MARKED, message, None).await?;
        
        Ok(())
    }
    
    // ============================================================================
    // 辅助方法
    // ============================================================================
    
    fn is_media_message(&self, message: &Message) -> bool {
        matches!(
            message.message_type,
            MessageType::Image | MessageType::Video | MessageType::Audio | MessageType::File
        )
    }
    
    async fn prepare_media_message(&self, message: &mut Message) -> anyhow::Result<()> {
        for attachment in &mut message.attachments {
            if let Some(local_path_str) = attachment.metadata.get("local_path") {
                if let Ok(data) = tokio::fs::read(local_path_str).await {
                    let _cached_path = self.media_cache.save_media(attachment, data).await?;
                }
            } else if !attachment.url.is_empty() {
                // 从 URL 下载并保存媒体文件
                let cached_path = self.media_cache.download_and_save(attachment).await?;
                // 将本地路径保存到 metadata 中
                attachment.metadata.insert("local_path".to_string(), cached_path.to_string_lossy().to_string());
            }
        }
        Ok(())
    }
    
    async fn load_message(&self, message_id: &str) -> anyhow::Result<Option<Message>> {
        use crate::domain::repository::{Query, QueryResult};
        let query = Query::MessageDetail {
            message_id: message_id.to_string(),
        };
        
        match self.read_store.query(query).await? {
            QueryResult::MessageDetail { item } => {
                if item.is_null() || item.get("message_id").is_none() {
                    Ok(None)
                } else {
                    Ok(serde_json::from_value::<Message>(item).ok())
                }
            }
            _ => Ok(None),
        }
    }
    
    async fn save_message(&self, message: &Message) -> anyhow::Result<()> {
        self.read_store.write_message(message).await
    }
    
    async fn publish_message_event(
        &self,
        event_type: &'static str,
        message: Message,
        additional_data: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        use crate::domain::event::DomainEvent;
        let mut data = serde_json::json!({
            "message_id": message.id,
            "conversation_id": message.conversation_id,
            "sender_id": message.sender_id,
        });
        
        if let Some(additional) = additional_data {
            if let Some(obj) = data.as_object_mut() {
                for (k, v) in additional.as_object().unwrap() {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }
        
        let event = DomainEvent::new(
            event_type,
            &message.id,
            message.version,
            data,
        );
        
        self.event_store.append(event).await?;
        Ok(())
    }
    
    /// 发布消息创建事件
    async fn publish_message_created_event(&self, message: &Message) -> anyhow::Result<()> {
        use crate::domain::event::{DomainEvent, message_events, MessageCreated};
        
        // 构建 MessageCreated 事件数据
        let message_created = MessageCreated {
            message_id: message.id.clone(),
            conversation_id: message.conversation_id.clone(),
            sender_id: message.sender_id.clone(),
            content: serde_json::json!(message.content), // 将 Vec<u8> 转换为 JSON
        };
        
        // 发布到 EventStore（持久化）
        let event = DomainEvent::new(
            message_events::CREATED,
            &message.id,
            message.version,
            serde_json::to_value(&message_created)?,
        );
        self.event_store.append(event.clone()).await?;
        
        // 发布到 EventBus（实时通知 UI 层）
        self.event_bus.publish(event).await?;
        
        Ok(())
    }
    
    async fn publish_message_sent_event(&self, message: &Message, is_retry: bool) -> anyhow::Result<()> {
        use crate::domain::event::{DomainEvent, message_events, MessageSent};
        
        // 构建 MessageSent 事件数据
        let message_sent = MessageSent {
            message_id: message.id.clone(),
            seq: message.seq.unwrap_or(0),
        };
        
        let event = DomainEvent::new(
            message_events::SENT,
            &message.id,
            message.version,
            serde_json::to_value(&message_sent)?,
        );
        
        // 发布到 EventStore（持久化）
        self.event_store.append(event.clone()).await?;
        
        // 发布到 EventBus（实时通知 UI 层）
        self.event_bus.publish(event).await?;
        
        Ok(())
    }
    
    async fn handle_send_failure(
        &self,
        message: Message,
        error_code: i32,
        error_msg: String,
        latency_ms: u64,
    ) -> anyhow::Result<()> {
        let is_retryable = self.is_retryable_error(error_code);
        
        if is_retryable {
            let mut retry_queue = self.retry_queue.lock().await;
            let retry_msg = RetryMessage::new(message.clone(), 3);
            retry_queue.insert(message.id.clone(), retry_msg);
            
            tracing::warn!(
                message_id = %message.id,
                error = %error_msg,
                "消息发送失败，已加入重试队列"
            );
            
            self.start_retry_task().await;
            Err(anyhow::anyhow!("消息发送失败，已加入重试队列: {}", error_msg))
        } else {
            tracing::error!(
                message_id = %message.id,
                error = %error_msg,
                "❌ 消息发送失败（不可重试）"
            );
            Err(anyhow::anyhow!("消息发送失败: {}", error_msg))
        }
    }
    
    async fn handle_network_error(
        &self,
        message: Message,
        error: anyhow::Error,
        latency_ms: u64,
    ) -> anyhow::Result<()> {
        let mut retry_queue = self.retry_queue.lock().await;
        let retry_msg = RetryMessage::new(message.clone(), 3);
        retry_queue.insert(message.id.clone(), retry_msg);
        
        self.start_retry_task().await;
        Err(anyhow::anyhow!("消息发送失败: {}", error))
    }
    
    fn is_retryable_error(&self, error_code: i32) -> bool {
        // 根据错误码判断是否可重试
        match error_code {
            500..=599 => true,  // 服务器错误，可重试
            408 => true,        // 请求超时，可重试
            _ => false,         // 其他错误，不可重试
        }
    }
    
    async fn start_retry_task(&self) {
        use std::sync::atomic::{AtomicBool, Ordering};
        static RETRY_TASK_STARTED: AtomicBool = AtomicBool::new(false);
        
        if RETRY_TASK_STARTED.swap(true, Ordering::Acquire) {
            return;
        }
        
        let handler = MessageCommandHandlerRef {
            fsm: self.fsm.clone(),
            event_store: self.event_store.clone(),
            read_store: self.read_store.clone(),
            message_sender: self.message_sender.clone(),
            media_cache: self.media_cache.clone(),
            retry_queue: self.retry_queue.clone(),
        };
        
        let handler_arc = Arc::new(handler);
        let retry_queue = self.retry_queue.clone();
        
        tokio::spawn(async move {
            let mut retry_interval = tokio::time::interval(Duration::from_secs(5));
            retry_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            
            loop {
                retry_interval.tick().await;
                
                let mut queue_guard = retry_queue.lock().await;
                let mut messages_to_retry = Vec::new();
                
                for (message_id, retry_msg) in queue_guard.iter() {
                    if retry_msg.can_retry() {
                        messages_to_retry.push((message_id.clone(), retry_msg.clone()));
                    }
                }
                
                drop(queue_guard);
                
                for (message_id, mut retry_msg) in messages_to_retry {
                    retry_msg.retry_count += 1;
                    let message = retry_msg.message.clone();
                    
                    match handler_arc.send_message_internal(message, true).await {
                        Ok(_) => {
                            let mut queue_guard = retry_queue.lock().await;
                            queue_guard.remove(&message_id);
                        }
                        Err(e) => {
                            let mut queue_guard = retry_queue.lock().await;
                            if retry_msg.can_retry() {
                                queue_guard.insert(message_id, retry_msg);
                            } else {
                                queue_guard.remove(&message_id);
                            }
                        }
                    }
                }
                
                let queue_guard = retry_queue.lock().await;
                if queue_guard.is_empty() {
                    RETRY_TASK_STARTED.store(false, Ordering::Release);
                    break;
                }
            }
        });
    }
}

/// 用于重试任务的内部引用结构
struct MessageCommandHandlerRef {
    fsm: Arc<FsmManager>,
    event_store: Arc<dyn EventStore>,
    read_store: Arc<dyn ReadStore>,
    message_sender: Arc<MessageSender>,
    media_cache: Arc<MediaCacheManager>,
    retry_queue: Arc<Mutex<HashMap<String, RetryMessage>>>,
}

impl MessageCommandHandlerRef {
    async fn send_message_internal(&self, mut message: Message, is_retry: bool) -> anyhow::Result<()> {
        self.fsm.message_start_sending(&mut message, is_retry).await?;
        
        let timeout = Duration::from_secs(30);
        let send_result = self.message_sender.send_message_and_wait_ack(&message, timeout).await;
        
        match send_result {
            Ok(result) => {
                use flare_proto::common::AckStatus;
                match result.status {
                    AckStatus::Success => {
                        self.fsm.message_send_success(&mut message, result.seq).await?;
                        Ok(())
                    }
                    _ => Err(anyhow::anyhow!("Send failed")),
                }
            }
            Err(e) => Err(e),
        }
    }
}

// 导入 message_events
use crate::domain::event::message_events;
