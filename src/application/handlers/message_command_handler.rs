use std::sync::Arc;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use crate::application::fsm::FsmManager;
use crate::domain::repository::{EventStore, MessageRepository};
use crate::domain::message::{Message, MessageType};
use crate::domain::service::MessageDomainService;
use crate::infrastructure::messaging::MessageSender;
use crate::infrastructure::storage::media_cache::MediaCacheManager;
use crate::infrastructure::event_bus::EventBus;
use crate::infrastructure::metrics;
use crate::application::commands::*;

#[derive(Debug, Clone)]
struct RetryMessage {
    message: Message,
    retry_count: u32,
    max_retries: u32,
}

impl RetryMessage {
    fn new(message: Message, max_retries: u32) -> Self {
        Self {
            message,
            retry_count: 0,
            max_retries,
        }
    }
    
    fn can_retry(&self) -> bool {
        self.retry_count < self.max_retries
    }
}

pub struct MessageCommandHandler {
    #[allow(dead_code)]
    fsm: Arc<FsmManager>,
    #[allow(dead_code)]
    event_store: Arc<dyn EventStore>,
    message_repository: Arc<dyn MessageRepository>,
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
        message_repository: Arc<dyn MessageRepository>,
        message_sender: Arc<MessageSender>,
        media_cache: Arc<MediaCacheManager>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            fsm,
            event_store,
            message_repository,
            message_sender,
            media_cache,
            event_bus,
            domain_service: MessageDomainService::new(),
            retry_queue: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    /// 验证并加载消息（用于操作消息处理前）
    ///
    /// 这是一个统一的验证方法，所有需要对现有消息进行操作的方法都应该使用它。
    /// 它处理以下逻辑：
    /// 1. 解析 client_msg_id 到 server_msg_id（通过 resolve_message_id）
    /// 2. 加载消息（通过 load_message）
    /// 3. 验证 server_id 是否存在（必须收到 ACK 后才能操作）
    ///
    /// # 参数
    /// * `client_msg_id` - 客户端消息 ID（可能是 client_msg_id 或 server_msg_id）
    ///
    /// # 返回
    /// * `Ok((message, server_msg_id))` - 验证通过，返回消息和最终的服务器消息 ID
    /// * `Err` - 如果消息不存在或 server_id 不可用
    ///
    /// # 错误情况
    /// * `MessageNotFound` - 消息不存在（通过 client_msg_id 或 server_msg_id 都找不到）
    /// * `ServerIdNotAvailable` - 消息的 server_id 不可用（消息还没有收到服务端 ACK）
    ///
    /// # 使用场景
    /// 所有对现有消息进行操作的方法都应该使用这个方法：
    /// - `handle_add_reaction`
    /// - `handle_remove_reaction`
    /// - `handle_recall`
    /// - `handle_edit`
    /// - `handle_delete`
    /// - `handle_pin`
    /// - `handle_unpin`
    /// - `handle_mark`
    /// - 等等
    ///
    /// # 示例
    /// ```rust
    /// let (mut message, server_msg_id) = self.validate_and_load_message_for_operation(&cmd.message_id).await?;
    /// // 现在可以安全地使用 message 和 server_msg_id 进行操作
    /// ```
    async fn validate_and_load_message_for_operation(
        &self,
        client_msg_id: &str,
    ) -> anyhow::Result<(Message, String)> {
        // 1. 解析消息ID（将 client_msg_id 转换为 server_msg_id）
        let resolved_msg_id = self.resolve_message_id(client_msg_id).await;
        
        // 2. 加载消息
        let message = self.load_message(&resolved_msg_id).await?
            .ok_or_else(|| anyhow::anyhow!(
                "Message not found: {} (original_id: {})",
                resolved_msg_id,
                client_msg_id
            ))?;
        
        // 3. 验证 server_id 是否存在
        // **关键限制**：操作消息处理前必须验证 server_id 是否存在
        // 如果 message.server_id 为空或等于 client_msg_id，说明消息还没有收到服务端 ACK
        // 此时不允许操作，必须等待 ACK 返回后才能操作
        let server_id = message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("");
        if server_id.is_empty() || server_id == client_msg_id {
            return Err(anyhow::anyhow!(
                "Message server_id is not available. Please wait for ACK before performing operation. \
                 client_msg_id: {}, resolved_msg_id: {}, server_id: {}",
                client_msg_id,
                resolved_msg_id,
                server_id
            ));
        }
        
        // 4. 确保使用服务端返回的 server_id（这是操作必须使用的 ID）
        let server_msg_id = message.server_id.clone().unwrap_or_default();
        
        tracing::debug!(
            client_msg_id = %client_msg_id,
            resolved_msg_id = %resolved_msg_id,
            server_msg_id = %server_msg_id,
            "消息验证成功，可以执行操作"
        );
        
        Ok((message, server_msg_id))
    }
    
    pub async fn handle(&self, cmd: SendMessageCommand) -> anyhow::Result<()> {
        self.send_message_internal(cmd.message, false).await
    }
    
    pub(crate) async fn send_message_internal(&self, mut message: Message, is_retry: bool) -> anyhow::Result<()> {
        if self.is_media_message(&message) {
            self.prepare_media_message(&mut message).await?;
        }
        
        self.save_message(&message).await?;
        self.publish_message_created_event(&message).await?;
        self.fsm.message_start_sending(&mut message, is_retry).await?;
        
        let send_start = Instant::now();
        let timeout = Duration::from_secs(30);
        let send_result = self.message_sender.send_message_and_wait_ack(&message, timeout).await;
        
        match send_result {
            Ok(result) => {
                let latency_ms = send_start.elapsed().as_millis() as u64;
                
                if result.success {
                    metrics::record_message_send(true, latency_ms).await;
                    let server_msg_id = result.server_msg_id.clone();
                    self.fsm.message_send_success(&mut message, result.seq, server_msg_id.clone()).await?;
                    self.save_message(&message).await?;
                    tracing::info!(
                        message_id = %server_msg_id,
                        client_msg_id = %message.client_msg_id,
                        seq = result.seq,
                        latency_ms = latency_ms,
                        "✅ 消息发送成功，已注册ID映射"
                    );
                    self.publish_message_sent_event(&message, is_retry).await?;
                    Ok(())
                } else {
                    metrics::record_message_send(false, latency_ms).await;
                    let error_msg = if !result.error_message.is_empty() {
                        result.error_message
                    } else {
                        format!("服务器返回错误码: {}", result.error_code)
                    };
                    self.fsm.message_send_failed(&mut message, error_msg.clone()).await?;
                    self.save_message(&message).await?;
                    self.publish_message_send_failed_event(&message, &error_msg).await?;
                    self.handle_send_failure(message, result.error_code, error_msg, latency_ms).await
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
    
    pub async fn handle_recall(&self, cmd: RecallMessageCommand) -> anyhow::Result<()> {
        let recaller_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 1. 验证并加载消息（统一验证逻辑）
        let (mut message, server_msg_id) = self.validate_and_load_message_for_operation(&cmd.client_msg_id).await?;
        
        // 2. 调用领域服务执行操作（业务逻辑在领域层）
        let mut operation = self.domain_service.execute_recall_operation(
            &mut message,
            recaller_id,
            cmd.reason.clone(),
            Some(120),
        )?;
        
        // 3. 确保 operation.target_message_id 使用正确的 server_msg_id
        if operation.target_message_id != server_msg_id {
            tracing::debug!(
                original_target = %operation.target_message_id,
                corrected_target = %server_msg_id,
                client_msg_id = %cmd.client_msg_id,
                "修复 operation.target_message_id，使用正确的 server_msg_id"
            );
            operation.target_message_id = server_msg_id.clone();
        }
        
        // 4. 保存消息（基础设施层职责）
        self.save_message(&message).await?;
        
        // 5. 发送操作到服务端（基础设施层职责）
        if let Err(e) = self.send_operation_to_server(&message, operation).await {
            tracing::warn!(
                error = %e,
                message_id = %server_msg_id,
                "Failed to send recall operation to server, but local state updated"
            );
        }
        
        // 6. 发布领域事件（应用层职责）
        self.publish_message_event(message_events::RECALLED, message, None).await?;
        Ok(())
    }
    
    /// 处理编辑消息命令
    ///
    /// IM 最佳实践：乐观更新（Optimistic Update）
    /// 1. 立即更新本地消息内容（content + edit_history）
    /// 2. 立即保存到本地数据库
    /// 3. 立即发布 MessageEdited 事件，UI 立即渲染（给用户即时反馈）
    /// 4. 后台异步发送编辑操作到服务端
    /// 5. 服务端处理完成后，通过同步机制推送更新后的消息，确保最终一致性
    ///
    /// 关键点：
    /// - 编辑操作不应该创建新消息，应该更新原消息
    /// - 操作消息（Operation Message）不应该被保存为普通消息
    /// - 编辑历史应该正确保存到本地和服务端
    pub async fn handle_edit(&self, cmd: EditMessageCommand) -> anyhow::Result<()> {
        let editor_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 1. 验证并加载消息（统一验证逻辑）
        let (mut message, server_msg_id) = self.validate_and_load_message_for_operation(&cmd.client_msg_id).await?;
        tracing::info!(
            message_id = %server_msg_id,
            client_msg_id = %cmd.client_msg_id,
            editor_id = %editor_id,
            "收到编辑消息（本地应用）"
        );
        
        // 2. 执行编辑操作（领域层：更新消息内容和编辑历史）
        let mut operation = self.domain_service.execute_edit_operation(
            &mut message,
            editor_id,
            cmd.new_content.clone(),
            cmd.reason.clone(),
        )?;
        
        // 3. 确保 operation.target_message_id 使用正确的 server_msg_id
        if operation.target_message_id != server_msg_id {
            operation.target_message_id = server_msg_id.clone();
        }
        
        // 4. 【乐观更新】立即保存更新后的消息到本地数据库
        // 这确保了用户立即看到编辑后的内容，不等待服务端确认
        self.save_message(&message).await?;
        
        // 5. 【乐观更新】立即发布 MessageEdited 事件，UI 立即渲染
        // 给用户即时反馈，提升用户体验
        self.publish_message_event(message_events::EDITED, message.clone(), None).await?;
        
        // 6. 【后台同步】异步发送编辑操作到服务端（不阻塞 UI）
        // 操作消息不应该被保存为普通消息，只是用来通知服务端
        if let Err(e) = self.send_operation_to_server(&message, operation).await {
            tracing::warn!(
                error = %e,
                message_id = %server_msg_id,
                "Failed to send edit operation to server, but local state updated"
            );
            // 注意：即使服务端发送失败，本地状态已经更新
            // 后续可以通过同步机制从服务端获取最新状态
        }
        
        Ok(())
    }
    
    /// 处理删除消息命令
    pub async fn handle_delete(&self, cmd: DeleteMessageCommand) -> anyhow::Result<()> {
        // 解析消息ID（将client_msg_id转换为server_msg_id）
        let resolved_msg_id = self.resolve_message_id(&cmd.client_msg_id).await;
        
        // 尝试加载消息，如果消息不存在，检查是否已经被删除
        let mut message = match self.load_message(&resolved_msg_id).await? {
            Some(msg) => msg,
            None => {
                // 消息不存在，可能是已经被删除或者是硬删除
                // 对于硬删除，直接返回成功（幂等性）
                if cmd.delete_type == crate::domain::message::DeleteType::Hard {
                    tracing::warn!(
                        message_id = %resolved_msg_id,
                        client_msg_id = %cmd.client_msg_id,
                        "Message not found, but hard delete is idempotent, returning success"
                    );
                    return Ok(());
                }
                // 对于软删除，如果消息不存在，返回错误
                return Err(anyhow::anyhow!("Message not found: {} (client_msg_id: {})", resolved_msg_id, cmd.client_msg_id));
            }
        };
        
        // **关键限制**：操作消息处理前必须验证 server_id 是否存在
        // 对于删除操作，我们也需要验证 server_id，但删除操作比较特殊（可能删除未ACK的消息）
        // 这里我们仍然要求 server_id 存在，确保操作能够正确同步到服务端
        let server_id = message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("");
        if server_id.is_empty() || server_id == cmd.client_msg_id {
            return Err(anyhow::anyhow!(
                "Message server_id is not available. Please wait for ACK before deleting message. client_msg_id: {}, server_id: {}",
                cmd.client_msg_id,
                server_id
            ));
        }
        
        let server_msg_id = message.server_id.clone().unwrap_or_default();
        
        // 检查消息是否已经被删除
        if let Some(deleted) = message.extra.get("deleted") {
            if deleted == "true" || deleted == "hard" {
                // 消息已经被删除，返回成功（幂等性）
                tracing::info!(
                    message_id = %message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("<none>"),
                    client_msg_id = %cmd.client_msg_id,
                    "Message already deleted, returning success (idempotent)"
                );
                return Ok(());
            }
        }
        
        let operator_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 使用领域服务验证是否可以删除
        let can_delete = self.domain_service.can_delete(&message, &operator_id, cmd.delete_type)?;
        if !can_delete {
            return Err(anyhow::anyhow!("Message cannot be deleted"));
        }
        
        // 使用领域服务处理删除（通过 apply_operation，本地状态更新）
        use crate::domain::message::{MessageOperation, OperationType, OperationData};
        let operation = MessageOperation {
            operation_type: OperationType::Delete,
            target_message_id: server_msg_id.clone(), // 使用已验证的 server_msg_id
            operator_id,
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
        self.domain_service.apply_operation(operation.clone(), &mut message)?;
        
        // 保存消息（本地状态）
        self.save_message(&message).await?;
        
        // 发送操作到服务端
        if let Err(e) = self.send_operation_to_server(&message, operation).await {
            tracing::warn!(
                error = %e,
                message_id = %server_msg_id,
                "Failed to send delete operation to server, but local state updated"
            );
            // 即使发送失败，本地状态已更新，继续执行
        }
        
        self.publish_message_event(message_events::DELETED, message, None).await?;
        
        Ok(())
    }
    
    /// 发送消息操作到服务端（通过 send_event 上行 Event，等待 OperationAck）
    async fn send_operation_to_server(
        &self,
        original_message: &Message,
        operation: crate::domain::message::MessageOperation,
    ) -> anyhow::Result<()> {
        let conversation_id = original_message
            .conversation_id
            .as_deref()
            .unwrap_or("");
        if conversation_id.is_empty() {
            return Err(anyhow::anyhow!("conversation_id required for send_operation"));
        }
        let event = crate::infrastructure::operation_event_builder::operation_to_event(&operation, conversation_id)?;
        let timeout = Duration::from_secs(30);
        let ack = self.message_sender.send_event_and_wait_ack(event, timeout).await?;
        if ack.success {
            tracing::info!(
                operation_type = ?operation.operation_type,
                target_message_id = %operation.target_message_id,
                "✅ 消息操作已发送到服务端"
            );
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Server returned error: {} - {}",
                ack.error_code,
                ack.error_message
            ))
        }
    }
    
    /// 处理标记消息已读命令
    pub async fn handle_mark_read(&self, cmd: MarkMessagesReadCommand) -> anyhow::Result<()> {
        let user_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        use crate::domain::message::{MessageOperation, OperationType, OperationData};
        use chrono::Utc;
        
        // 批量处理已读操作
        let mut operations = Vec::new();
        
        for message_id in &cmd.message_ids {
            if let Some(mut message) = self.load_message(message_id).await? {
                // 使用 Message 的 mark_read 方法（本地状态更新）
                message.mark_read(user_id.clone())?;
                
                // 如果是阅后即焚，设置销毁时间
                if cmd.burn_after_read {
                    // 使用领域服务计算过期时间
                    let burn_seconds = message.burn_after_seconds.unwrap_or(60); // 默认 60 秒
                    let expire_at = self.domain_service.calculate_expire_at(&message, burn_seconds);
                    // 在消息的 extra 中记录过期时间
                    message.extra.insert("burn_expire_at".to_string(), expire_at.to_rfc3339());
                }
                
                // 保存消息（本地状态）
                self.save_message(&message).await?;
                
                // 构建已读操作（批量发送）
                operations.push((message, message_id.clone()));
            }
        }
        
        // 批量发送已读操作到服务端
        if !operations.is_empty() {
            let operation = MessageOperation {
                operation_type: OperationType::Read,
                target_message_id: cmd.message_ids[0].clone(), // 使用第一条消息ID作为目标
                operator_id: user_id,
                timestamp: Utc::now(),
                show_notice: false,
                notice_text: None,
                target_user_id: None,
                operation_data: OperationData::Read {
                    message_ids: cmd.message_ids.clone(),
                    read_at: Some(Utc::now()),
                    burn_after_read: cmd.burn_after_read,
                },
                metadata: std::collections::HashMap::new(),
            };
            
            // 使用第一条消息作为原始消息发送操作
            if let Some((original_message, _)) = operations.first() {
                if let Err(e) = self.send_operation_to_server(original_message, operation).await {
                    tracing::warn!(
                        error = %e,
                        "Failed to send read operation to server, but local state updated"
                    );
                    // 即使发送失败，本地状态已更新，继续执行
                }
            }
        }
        
        Ok(())
    }
    
    /// 处理回复消息命令
    pub async fn handle_reply(&self, cmd: ReplyMessageCommand) -> anyhow::Result<String> {
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 解析被引用的消息ID（将client_msg_id转换为server_msg_id）
        let server_quoted_id = self.resolve_message_id(&cmd.quoted_message_id).await;
        
        // 加载被引用的消息（用于获取 receiver_id 和引用信息）
        let original_message = self.load_message(&server_quoted_id).await?
            .ok_or_else(|| anyhow::anyhow!("Original message not found: {} (client_msg_id: {})", server_quoted_id, cmd.quoted_message_id))?;
        
        // 生成引用预览文本（如果未提供）
        let quoted_text_preview = cmd.quoted_text_preview
            .unwrap_or_else(|| self.domain_service.generate_preview(&original_message));
        
        // 获取被引用消息的发送者ID（如果未提供）
        let quoted_sender_id = cmd.quoted_sender_id
            .unwrap_or_else(|| original_message.sender_id.clone());
        
        // 使用 build_reply_message 创建回复消息（使用 quote 字段）
        use crate::domain::message::build_reply_message;
        let reply_message = build_reply_message(
            Some(cmd.conversation_id),  // conversation_id 现在是 Option
            sender_id,
            server_quoted_id.clone(),
            Some(quoted_sender_id),
            Some(quoted_text_preview),
            cmd.reply_content,
        )?;
        
        // 设置 receiver_id（单聊时必需）
        let mut reply_message = reply_message;
        reply_message.receiver_id = original_message.receiver_id.clone();
        
        // 发送回复消息（这会创建新消息并发送到服务端）
        self.send_message_internal(reply_message.clone(), false).await?;
        
        Ok(reply_message.server_id.clone().unwrap_or_else(|| reply_message.client_msg_id.clone()))
    }
    
    /// 处理转发消息命令
    pub async fn handle_forward(&self, cmd: ForwardMessagesCommand) -> anyhow::Result<Vec<String>> {
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        if cmd.message_ids.is_empty() {
            return Err(anyhow::anyhow!("No messages to forward"));
        }
        
        // 解析消息ID（将client_msg_id转换为server_msg_id）
        let mut server_message_ids = Vec::new();
        for message_id in &cmd.message_ids {
            let server_id = self.resolve_message_id(message_id).await;
            server_message_ids.push(server_id);
        }
        
        // 使用领域服务创建转发消息
        let forwarded_message = if cmd.merge_forward {
            // 合并转发：创建一条包含多条消息的转发消息
            self.domain_service.create_forward_message(
                Some(cmd.target_conversation_id.clone()),
                sender_id.clone(),
                server_message_ids.clone(),
                None, // forward_reason
            )?
        } else {
            // 逐条转发：为每条消息创建一条转发消息
            let mut forwarded_ids = Vec::new();
            for message_id in &server_message_ids {
                let forwarded_message = self.domain_service.create_forward_message(
                    Some(cmd.target_conversation_id.clone()),
                    sender_id.clone(),
                    vec![message_id.clone()],
                    None,
                )?;
                
                // 发送转发消息
                self.send_message_internal(forwarded_message.clone(), false).await?;
                let msg_id = forwarded_message.server_id.clone().unwrap_or_else(|| forwarded_message.client_msg_id.clone());
                forwarded_ids.push(msg_id);
            }
            
            return Ok(forwarded_ids);
        };
        
        // 发送转发消息
        self.send_message_internal(forwarded_message.clone(), false).await?;
        
        Ok(vec![forwarded_message.server_id.clone().unwrap_or_else(|| forwarded_message.client_msg_id.clone())])
    }
    
    /// 处理添加反应命令（简化版：只负责编排）
    pub async fn handle_add_reaction(&self, cmd: AddReactionCommand) -> anyhow::Result<()> {
        let user_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 1. 验证并加载消息（统一验证逻辑）
        let (mut message, server_msg_id) = self.validate_and_load_message_for_operation(&cmd.message_id).await?;
        
        // 2. 调用领域服务执行操作（业务逻辑在领域层）
        let mut operation = self.domain_service.execute_add_reaction_operation(
            &mut message,
            user_id,
            cmd.emoji.clone(),
        )?;
        
        // 3. 确保 operation.target_message_id 使用正确的 server_msg_id
        if operation.target_message_id != server_msg_id {
            tracing::debug!(
                original_target = %operation.target_message_id,
                corrected_target = %server_msg_id,
                client_msg_id = %cmd.message_id,
                "修复 operation.target_message_id，使用正确的 server_msg_id"
            );
            operation.target_message_id = server_msg_id.clone();
        }
        
        // 4. 保存消息（基础设施层职责）
        self.save_message(&message).await?;
        
        // 5. 发送操作到服务端（基础设施层职责）
        if let Err(e) = self.send_operation_to_server(&message, operation).await {
            tracing::warn!(
                error = %e,
                message_id = %server_msg_id,
                client_msg_id = %cmd.message_id,
                "Failed to send reaction operation to server, but local state updated"
            );
        }
        
        // 6. 发布领域事件（应用层职责）
        self.publish_message_event(message_events::REACTION_ADDED, message, None).await?;
        
        Ok(())
    }
    
    /// 处理移除反应命令（简化版：只负责编排）
    pub async fn handle_remove_reaction(&self, cmd: RemoveReactionCommand) -> anyhow::Result<()> {
        let user_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 1. 验证并加载消息（统一验证逻辑）
        let (mut message, server_msg_id) = self.validate_and_load_message_for_operation(&cmd.message_id).await?;
        
        // 2. 调用领域服务执行操作（业务逻辑在领域层）
        let mut operation = self.domain_service.execute_remove_reaction_operation(
            &mut message,
            user_id,
            cmd.emoji.clone(),
        )?;
        
        // 3. 确保 operation.target_message_id 使用正确的 server_msg_id
        if operation.target_message_id != server_msg_id {
            tracing::debug!(
                original_target = %operation.target_message_id,
                corrected_target = %server_msg_id,
                client_msg_id = %cmd.message_id,
                "修复 operation.target_message_id，使用正确的 server_msg_id"
            );
            operation.target_message_id = server_msg_id.clone();
        }
        
        // 4. 保存消息（基础设施层职责）
        self.save_message(&message).await?;
        
        // 5. 发送操作到服务端（基础设施层职责）
        if let Err(e) = self.send_operation_to_server(&message, operation).await {
            tracing::warn!(
                error = %e,
                message_id = %server_msg_id,
                client_msg_id = %cmd.message_id,
                "Failed to send reaction operation to server, but local state updated"
            );
        }
        
        // 6. 发布领域事件（应用层职责）
        self.publish_message_event(message_events::REACTION_REMOVED, message, None).await?;
        
        Ok(())
    }
    
    /// 处理置顶消息命令（简化版：只负责编排）
    pub async fn handle_pin(&self, cmd: PinMessageCommand) -> anyhow::Result<()> {
        let operator_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 1. 验证并加载消息（统一验证逻辑）
        let (mut message, server_msg_id) = self.validate_and_load_message_for_operation(&cmd.message_id).await?;
        
        // 2. 调用领域服务执行操作（业务逻辑在领域层）
        let mut operation = self.domain_service.execute_pin_operation(
            &mut message,
            operator_id,
            cmd.reason.clone(),
            cmd.expire_at,
        )?;
        
        // 3. 确保 operation.target_message_id 使用正确的 server_msg_id
        if operation.target_message_id != server_msg_id {
            tracing::debug!(
                original_target = %operation.target_message_id,
                corrected_target = %server_msg_id,
                client_msg_id = %cmd.message_id,
                "修复 operation.target_message_id，使用正确的 server_msg_id"
            );
            operation.target_message_id = server_msg_id.clone();
        }
        
        // 4. 保存消息（基础设施层职责）
        self.save_message(&message).await?;
        
        // 5. 发送操作到服务端（基础设施层职责）
        if let Err(e) = self.send_operation_to_server(&message, operation).await {
            tracing::warn!(
                error = %e,
                message_id = %server_msg_id,
                "Failed to send pin operation to server, but local state updated"
            );
        }
        
        // 6. 发布领域事件（应用层职责）
        self.publish_message_event(message_events::PINNED, message, None).await?;
        
        Ok(())
    }
    
    /// 处理取消置顶命令（简化版：只负责编排）
    pub async fn handle_unpin(&self, cmd: UnpinMessageCommand) -> anyhow::Result<()> {
        let operator_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 1. 验证并加载消息（统一验证逻辑）
        let (mut message, server_msg_id) = self.validate_and_load_message_for_operation(&cmd.message_id).await?;
        
        // 2. 调用领域服务执行操作（业务逻辑在领域层）
        let mut operation = self.domain_service.execute_unpin_operation(
            &mut message,
            operator_id,
        )?;
        
        // 3. 确保 operation.target_message_id 使用正确的 server_msg_id
        if operation.target_message_id != server_msg_id {
            tracing::debug!(
                original_target = %operation.target_message_id,
                corrected_target = %server_msg_id,
                client_msg_id = %cmd.message_id,
                "修复 operation.target_message_id，使用正确的 server_msg_id"
            );
            operation.target_message_id = server_msg_id.clone();
        }
        
        // 4. 保存消息（基础设施层职责）
        self.save_message(&message).await?;
        
        // 5. 发送操作到服务端（基础设施层职责）
        if let Err(e) = self.send_operation_to_server(&message, operation).await {
            tracing::warn!(
                error = %e,
                message_id = %server_msg_id,
                "Failed to send unpin operation to server, but local state updated"
            );
        }
        
        // 6. 发布领域事件（应用层职责）
        self.publish_message_event(message_events::UNPINNED, message, None).await?;
        
        Ok(())
    }
    
    /// 处理收藏消息命令
    pub async fn handle_favorite(&self, cmd: FavoriteMessageCommand) -> anyhow::Result<()> {
        let operator_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 解析消息ID（将client_msg_id转换为server_msg_id）
        let server_msg_id = self.resolve_message_id(&cmd.message_id).await;
        
        let mut message = self.load_message(&server_msg_id).await?
            .ok_or_else(|| anyhow::anyhow!("Message not found: {} (client_msg_id: {})", server_msg_id, cmd.message_id))?;
        
        // 使用领域服务验证是否可以收藏
        let can_favorite = self.domain_service.can_favorite(&message, &operator_id)?;
        if !can_favorite {
            return Err(anyhow::anyhow!("Cannot favorite this message"));
        }
        
        // 在 extra 字段中标记为收藏（本地状态更新）
        message.extra.insert("is_favorited".to_string(), "true".to_string());
        message.extra.insert("favorited_by".to_string(), operator_id);
        if !cmd.tags.is_empty() {
            message.extra.insert("favorite_tags".to_string(), cmd.tags.join(","));
        }
        if let Some(note) = &cmd.note {
            message.extra.insert("favorite_note".to_string(), note.clone());
        }
        message.version += 1;
        message.updated_at = chrono::Utc::now();
        
        // 保存消息（本地状态）
        self.save_message(&message).await?;
        
        // 收藏操作是用户偏好，不需要通过 MessageOperation 发送到服务端
        // 如果需要服务端持久化，可以通过其他方式实现（如用户偏好表）
        // 目前只更新本地状态
        
        self.publish_message_event(message_events::FAVORITED, message, None).await?;
        
        Ok(())
    }
    
    /// 处理取消收藏命令
    pub async fn handle_unfavorite(&self, cmd: UnfavoriteMessageCommand) -> anyhow::Result<()> {
        // 解析消息ID（将client_msg_id转换为server_msg_id）
        let server_msg_id = self.resolve_message_id(&cmd.message_id).await;
        
        let mut message = self.load_message(&server_msg_id).await?
            .ok_or_else(|| anyhow::anyhow!("Message not found: {} (client_msg_id: {})", server_msg_id, cmd.message_id))?;
        
        // 移除收藏标记（本地状态更新）
        message.extra.remove("is_favorited");
        message.extra.remove("favorited_by");
        message.extra.remove("favorite_tags");
        message.extra.remove("favorite_note");
        message.version += 1;
        message.updated_at = chrono::Utc::now();
        
        // 保存消息（本地状态）
        self.save_message(&message).await?;
        
        // 取消收藏操作是用户偏好，不需要通过 MessageOperation 发送到服务端
        // 如果需要服务端持久化，可以通过其他方式实现（如用户偏好表）
        // 目前只更新本地状态
        
        self.publish_message_event(message_events::UNFAVORITED, message, None).await?;
        
        Ok(())
    }
    
    /// 处理标记消息命令（简化版：只负责编排）
    pub async fn handle_mark(&self, cmd: MarkMessageCommand) -> anyhow::Result<()> {
        let operator_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 1. 验证并加载消息（统一验证逻辑）
        let (mut message, server_msg_id) = self.validate_and_load_message_for_operation(&cmd.message_id).await?;
        
        // 2. 调用领域服务执行操作（业务逻辑在领域层）
        let mut operation = self.domain_service.execute_mark_operation(
            &mut message,
            operator_id,
            cmd.mark_type,
            cmd.color.clone(),
        )?;
        
        // 3. 确保 operation.target_message_id 使用正确的 server_msg_id
        if operation.target_message_id != server_msg_id {
            tracing::debug!(
                original_target = %operation.target_message_id,
                corrected_target = %server_msg_id,
                client_msg_id = %cmd.message_id,
                "修复 operation.target_message_id，使用正确的 server_msg_id"
            );
            operation.target_message_id = server_msg_id.clone();
        }
        
        // 4. 保存消息（基础设施层职责）
        self.save_message(&message).await?;
        
        // 5. 发送操作到服务端（基础设施层职责）
        if let Err(e) = self.send_operation_to_server(&message, operation).await {
            tracing::warn!(
                error = %e,
                message_id = %server_msg_id,
                "Failed to send mark operation to server, but local state updated"
            );
        }
        
        // 6. 发布领域事件（应用层职责）
        self.publish_message_event(message_events::MARKED, message, None).await?;
        
        Ok(())
    }
    
    /// 处理线程回复命令
    pub async fn handle_thread_reply(&self, cmd: crate::application::commands::AddThreadReplyCommand) -> anyhow::Result<String> {
        let sender_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        // 解析线程ID（将client_msg_id转换为server_msg_id）
        let server_thread_id = self.resolve_message_id(&cmd.thread_id).await;
        
        // 加载线程首条消息
        let thread_message = self.load_message(&server_thread_id).await?
            .ok_or_else(|| anyhow::anyhow!("Thread message not found: {} (client_msg_id: {})", server_thread_id, cmd.thread_id))?;
        
        // 使用 build_text_message 创建回复消息
        use crate::domain::message::build_text_message;
        let mut reply_message = build_text_message(
            Some(cmd.conversation_id),  // conversation_id 现在是 Option
            sender_id,
            // 使用统一的解码方法从 protobuf 解码文本内容
            match flare_proto::decode_message_content(&cmd.reply_content) {
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
            },
            thread_message.receiver_id.clone(),
        )?;
        
        // 设置 thread_id 到消息的 attributes 字段（服务端期望的字段）
        reply_message.attributes.insert("thread_id".to_string(), server_thread_id.clone());
        
        // 发送回复消息（这会创建新消息并发送到服务端）
        self.send_message_internal(reply_message.clone(), false).await?;
        
        Ok(reply_message.server_id.clone().unwrap_or_else(|| reply_message.client_msg_id.clone()))
    }
    
    /// 处理批量标记已读命令
    pub async fn handle_batch_mark_read(&self, cmd: crate::application::commands::BatchMarkMessageReadCommand) -> anyhow::Result<()> {
        let user_id = self.fsm.current_user_id().await
            .ok_or_else(|| anyhow::anyhow!("User is not logged in"))?;
        
        use crate::domain::message::{MessageOperation, OperationType, OperationData};
        use chrono::Utc;
        
        let message_ids = if let Some(ids) = cmd.message_ids {
            ids
        } else {
            // 如果没有指定消息ID，标记会话中所有未读消息为已读
            // 注意：这是一个简化实现，实际生产环境应该从 ReadStore 查询未读消息列表
            // 目前暂时返回空列表，表示没有需要标记的消息
            // TODO: 实现从 ReadStore 查询未读消息的功能
            tracing::warn!(
                conversation_id = %cmd.conversation_id,
                "BatchMarkMessagesReadCommand received with empty message_ids. Marking all messages in conversation as read (simplified - no unread messages found)."
            );
            Vec::new()
        };
        
        if message_ids.is_empty() {
            return Ok(());
        }
        
        // 批量处理已读操作
        for message_id in &message_ids {
            if let Some(mut message) = self.load_message(message_id).await? {
                // 使用 Message 的 mark_read 方法（本地状态更新）
                message.mark_read(user_id.clone())?;
                
                // 如果是阅后即焚，设置销毁时间
                if cmd.burn_after_read {
                    let burn_seconds = message.burn_after_seconds.unwrap_or(60);
                    let expire_at = self.domain_service.calculate_expire_at(&message, burn_seconds);
                    message.extra.insert("burn_expire_at".to_string(), expire_at.to_rfc3339());
                }
                
                // 保存消息（本地状态）
                self.save_message(&message).await?;
            }
        }
        
        // 批量发送已读操作到服务端
        let operation = MessageOperation {
            operation_type: OperationType::Read,
            target_message_id: message_ids[0].clone(),
            operator_id: user_id,
            timestamp: Utc::now(),
            show_notice: false,
            notice_text: None,
            target_user_id: None,
            operation_data: OperationData::Read {
                message_ids: message_ids.clone(),
                read_at: Some(Utc::now()),
                burn_after_read: cmd.burn_after_read,
            },
            metadata: std::collections::HashMap::new(),
        };
        
        // 使用第一条消息作为原始消息发送操作
        if let Some(original_message) = self.load_message(&message_ids[0]).await? {
            if let Err(e) = self.send_operation_to_server(&original_message, operation).await {
                tracing::warn!(
                    error = %e,
                    "Failed to send batch read operation to server, but local state updated"
                );
            }
        }
        
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
    
    /// 解析消息ID（将 client_msg_id 转换为 server_msg_id）
    /// 
    /// 策略：
    /// 1. 如果 message_id 已经是 server_msg_id，直接返回
    /// 2. 如果 message_id 是 client_msg_id，从本地 ReadStore 查询
    /// 3. 如果本地找不到，尝试从服务端查询（通过 client_msg_id）
    async fn resolve_message_id(&self, message_id: &str) -> String {
        // 首先尝试从本地 ReadStore 查询
        if let Ok(Some(msg)) = self.load_message(message_id).await {
            // 如果找到消息，返回 server_msg_id
            return msg.server_id.clone().unwrap_or_else(|| msg.client_msg_id.clone());
        }
        
        // 如果本地找不到，尝试从服务端查询
        // 注意：这里假设 message_id 是 client_msg_id
        // 服务端应该支持通过 client_msg_id 查询消息
        if let Ok(Some(msg)) = self.query_message_from_server(message_id).await {
            // 保存到本地 ReadStore
            let _ = self.save_message(&msg).await;
            return msg.server_id.clone().unwrap_or_else(|| msg.client_msg_id.clone());
        }
        
        // 如果都找不到，返回原始 ID（可能是 server_msg_id）
        message_id.to_string()
    }
    
    /// 从服务端查询消息（通过 client_msg_id）
    /// 
    /// 注意：服务端应该支持通过 client_msg_id 查询消息
    async fn query_message_from_server(&self, client_msg_id: &str) -> anyhow::Result<Option<Message>> {
        // TODO: 实现从服务端查询消息的逻辑
        // 可以通过 gRPC 调用 GetMessage API，或者通过 WebSocket/QUIC 协议查询
        // 目前先返回 None，等待后续实现
        tracing::warn!(
            client_msg_id = %client_msg_id,
            "query_message_from_server not implemented yet, returning None"
        );
        Ok(None)
    }
    
    async fn load_message(&self, message_id: &str) -> anyhow::Result<Option<Message>> {
        // 先尝试通过 message_id 查找（可能是 server_id 或 client_msg_id）
        self.message_repository.find_by_id(message_id).await
    }
    
    /// 保存消息到本地数据库
    ///
    /// IM 最佳实践：操作消息不应该被保存为普通消息
    /// 操作消息只是用来通知服务端执行操作，不应该出现在消息列表中
    async fn save_message(&self, message: &Message) -> anyhow::Result<()> {
        // 检查是否是操作消息（不应该被保存）
        if message.message_type == crate::domain::message::MessageType::Operation {
            tracing::debug!(
                message_id = %message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("<none>"),
                "跳过保存操作消息（操作消息不应该被保存为普通消息）"
            );
            return Ok(());
        }
        
        // 检查是否有跳过保存的标记
        if message.extra.get("_skip_local_save") == Some(&"true".to_string()) {
            tracing::debug!(
                message_id = %message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("<none>"),
                "跳过保存消息（标记为 _skip_local_save）"
            );
            return Ok(());
        }
        
        self.message_repository.save(message).await
    }
    
    async fn publish_message_event(
        &self,
        event_type: &'static str,
        message: Message,
        additional_data: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        use crate::domain::event::DomainEvent;
        let mut data = serde_json::json!({
            "message_id": message.server_id.clone().unwrap_or_default(),
            "conversation_id": message.conversation_id.clone().unwrap_or_default(),
            "sender_id": message.sender_id,
        });
        
        if let Some(additional) = additional_data {
            if let Some(obj) = data.as_object_mut() {
                for (k, v) in additional.as_object().unwrap() {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }
        
        let aggregate_id = message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("");
        let event = DomainEvent::new(
            event_type,
            aggregate_id,
            message.version,
            data,
        );
        
        // 发布到 EventStore（持久化）
        self.event_store.append(event.clone()).await?;
        
        // 发布到 EventBus（实时通知 UI 层）
        // 关键：必须同时发布到 EventBus，否则 UI 不会收到事件
        self.event_bus.publish(event).await?;
        
        Ok(())
    }
    
    /// 发布消息创建事件
    async fn publish_message_created_event(&self, message: &Message) -> anyhow::Result<()> {
        use crate::domain::event::{DomainEvent, message_events, MessageCreated};
        use crate::domain::message::ContentType;
        
        // 对于文本消息，直接从 protobuf 解码提取文本字符串
        // 对于非文本消息，保留原始 protobuf 字节数组
        let content_value = if message.content_type == ContentType::PlainText {
            match flare_proto::decode_message_content(message.content.as_slice()) {
                Ok(mc) => {
                    if let Some(flare_proto::flare::common::v1::message_content::Content::Text(text_content)) = mc.content {
                        // 文本消息：直接使用字符串，serde_json::Value::String 是最小化包装
                        serde_json::Value::String(text_content.text)
                    } else {
                        // 非文本类型的 PlainText（理论上不应该发生），保留原始字节
                        serde_json::json!(message.content)
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        message_id = %message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("<none>"),
                        error = %e,
                        "Failed to decode MessageContent from protobuf, using raw bytes"
                    );
                    serde_json::json!(message.content)
                }
            }
        } else {
            // 非文本消息：保留原始 protobuf 字节数组
            serde_json::json!(message.content)
        };
        
        // 构建 MessageCreated 事件数据
        let message_created = MessageCreated {
            message_id: message.server_id.clone().unwrap_or_default(),
            conversation_id: message.conversation_id.clone(),
            sender_id: message.sender_id.clone(),
            content: content_value, // 使用提取的文本内容或原始字节数组
        };
        
        // 构建事件数据，包含 MessageCreated 和完整的 Message 对象（用于事件投影）
        let mut event_data = serde_json::to_value(&message_created)?;
        // 添加完整的消息对象到事件数据中，供事件投影器使用
        if let Some(obj) = event_data.as_object_mut() {
            // 将完整的消息对象序列化并添加到事件数据中
            let message_json = serde_json::to_value(message)?;
            obj.insert("message".to_string(), message_json);
        }
        
        // 发布到 EventStore（持久化）
        let aggregate_id = message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("");
        let event = DomainEvent::new(
            message_events::CREATED,
            aggregate_id,
            message.version,
            event_data.clone(),
        );
        self.event_store.append(event.clone()).await?;
        
        // 发布到 EventBus（实时通知 UI 层）
        self.event_bus.publish(event).await?;
        
        Ok(())
    }
    
    async fn publish_message_sent_event(&self, message: &Message, _is_retry: bool) -> anyhow::Result<()> {
        use crate::domain::event::{DomainEvent, message_events, MessageSent};
        
        // 构建 MessageSent 事件数据
        let message_sent = MessageSent {
            message_id: message.server_id.clone().unwrap_or_default(),
            seq: message.seq.unwrap_or(0),
        };
        
        let aggregate_id = message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("");
        let event = DomainEvent::new(
            message_events::SENT,
            aggregate_id,
            message.version,
            serde_json::to_value(&message_sent)?,
        );
        
        // 发布到 EventStore（持久化）
        self.event_store.append(event.clone()).await?;
        
        // 发布到 EventBus（实时通知 UI 层）
        self.event_bus.publish(event).await?;
        
        Ok(())
    }
    
    async fn publish_message_send_failed_event(&self, message: &Message, error: &str) -> anyhow::Result<()> {
        use crate::domain::event::{DomainEvent, message_events, MessageSendFailed};
        
        // 构建 MessageSendFailed 事件数据
        let message_send_failed = MessageSendFailed {
            message_id: message.server_id.clone().unwrap_or_default(),
            error: error.to_string(),
        };
        
        let aggregate_id = message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("");
        let event = DomainEvent::new(
            message_events::SEND_FAILED,
            aggregate_id,
            message.version,
            serde_json::to_value(&message_send_failed)?,
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
        _latency_ms: u64,
    ) -> anyhow::Result<()> {
        let is_retryable = self.is_retryable_error(error_code);
        
        if is_retryable {
            let mut retry_queue = self.retry_queue.lock().await;
            let retry_msg = RetryMessage::new(message.clone(), 3);
            let msg_id = message.server_id.clone().unwrap_or_else(|| message.client_msg_id.clone());
            retry_queue.insert(msg_id.clone(), retry_msg);
            
            tracing::warn!(
                message_id = %msg_id,
                error = %error_msg,
                "消息发送失败，已加入重试队列"
            );
            
            self.start_retry_task().await;
            Err(anyhow::anyhow!("消息发送失败，已加入重试队列: {}", error_msg))
        } else {
            tracing::error!(
                message_id = %message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("<none>"),
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
        _latency_ms: u64,
    ) -> anyhow::Result<()> {
        let mut retry_queue = self.retry_queue.lock().await;
        let retry_msg = RetryMessage::new(message.clone(), 3);
        let msg_id = message.server_id.clone().unwrap_or_else(|| message.client_msg_id.clone());
        retry_queue.insert(msg_id, retry_msg);
        
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
            message_sender: self.message_sender.clone(),
        };
        
        let handler_arc = Arc::new(handler);
        let retry_queue = self.retry_queue.clone();
        
        tokio::spawn(async move {
            let mut retry_interval = tokio::time::interval(Duration::from_secs(5));
            retry_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            
            loop {
                retry_interval.tick().await;
                
                let queue_guard = retry_queue.lock().await;
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
                        Err(_e) => {
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
    message_sender: Arc<MessageSender>,
}

impl MessageCommandHandlerRef {
    async fn send_message_internal(&self, mut message: Message, is_retry: bool) -> anyhow::Result<()> {
        self.fsm.message_start_sending(&mut message, is_retry).await?;
        
        let timeout = Duration::from_secs(30);
        let send_result = self.message_sender.send_message_and_wait_ack(&message, timeout).await;
        
        match send_result {
            Ok(result) => {
                if result.success {
                    self.fsm.message_send_success(&mut message, result.seq, result.server_msg_id.clone()).await?;
                    Ok(())
                } else {
                    Err(anyhow::anyhow!("Send failed: {} - {}", result.error_code, result.error_message))
                }
            }
            Err(e) => Err(e),
        }
    }
}

// 导入 message_events
use crate::domain::event::message_events;
