//! 消息服务
//!
//! 提供消息发送、接收、本地存储等核心功能

use crate::connection::ConnectionManager;
use crate::event::{Event, EventBus, MessageEvent};
use crate::model::{Message, ExtendedMessage};
#[cfg(feature = "extensions")]
use crate::extension::ExtensionInfoManager as ExtensionManager;
#[cfg(not(feature = "extensions"))]
type ExtensionManager = ();
use crate::protocol::FrameBuilder;
use crate::storage::{StorageBackend, MessageState};
use anyhow::{Context, Result};
use flare_core::common::protocol::{
    MessageCommand, SystemCommand, Reliability,
};
use flare_core::common::protocol::flare::core::commands::{
    message_command::Type as MessageCommandType,
    system_command::Type as SystemCommandType,
};
use flare_proto::{
    MessageType, MessageStatus, MessageSource,
    MessageContent as ProtoMessageContent,
};
use flare_proto::flare::common::v1::message_content::Content as ProtoContent;
use prost_types::Timestamp;
use prost::Message as ProstMessage;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
#[cfg(target_arch = "wasm32")]
use tokio::task::spawn_local as tokio_spawn;
#[cfg(not(target_arch = "wasm32"))]
use tokio::spawn as tokio_spawn;
use tracing::{debug, error, info};
#[cfg(not(target_arch = "wasm32"))]
use uuid::Uuid;
use crate::service::crypto::{CryptoService, NoopCrypto};
use tokio::sync::RwLock as TokioRwLock;
use crate::service::message::queue::{MessageQueue, MessageQueueConfig, MessageBatchProcessor};
use arc_swap::ArcSwap;

// MessageHandler 已完全移除，统一使用 MessageObserver
// 所有消息处理都通过 MessageObserver 接口

/// 消息服务
/// 
/// 负责消息的发送、接收、本地存储和分发
pub struct MessageService {
    /// 连接管理器（通过长连接发送消息）
    connection: Arc<ConnectionManager>,
    
    /// 本地存储
    storage: Arc<dyn StorageBackend>,
    
    /// 事件总线
    event_bus: Arc<EventBus>,
    
    /// 消息观察者列表（统一的消息处理接口）
    /// 使用 ArcSwap 实现无锁读取，提升性能
    observers: Arc<ArcSwap<Vec<crate::observer::ArcMessageObserver>>>,
    
    /// 当前用户 ID
    user_id: Arc<RwLock<String>>,
    
    /// 会话服务（用于更新会话信息）
    session_service: Arc<crate::service::SessionService>,
    
    /// 消息队列（优先级队列，支持批处理和去重）
    message_queue: Arc<MessageQueue>,
    
    /// 批处理器（用于批量发送）
    batch_processor: Arc<MessageBatchProcessor>,
    
    crypto: Arc<TokioRwLock<Arc<dyn CryptoService>>>,
    
    /// 扩展管理器（用于填充扩展信息）
    /// 如果启用了 extensions feature，则必需；否则使用空实现
    #[cfg(feature = "extensions")]
    extension_manager: Arc<ExtensionManager>,
    
    /// 错误恢复器（用于自动重试）
    error_recovery: Arc<crate::error::ErrorRecovery>,
    
    /// 熔断器（用于防止雪崩）
    circuit_breaker: Arc<crate::error::CircuitBreaker>,
}

#[derive(Clone)]
pub struct SendOptions {
    pub reliability: Reliability,
    pub priority: Option<i32>,
}

impl Default for SendOptions {
    fn default() -> Self {
        Self { reliability: Reliability::AtLeastOnce, priority: None }
    }
}

impl MessageService {
    /// 创建新的消息服务实例
    /// 
    /// 使用优先级队列、批处理、错误恢复和熔断器（所有功能默认启用）
    pub fn new(
        connection: Arc<ConnectionManager>,
        storage: Arc<dyn StorageBackend>,
        event_bus: Arc<EventBus>,
        user_id: Arc<RwLock<String>>,
    ) -> Self {
        // 默认配置：启用所有优化功能
        let queue_config = MessageQueueConfig::default();
        let (queue, mut queue_rx) = MessageQueue::new(queue_config.clone());
        let queue = Arc::new(queue);
        let batch_processor = Arc::new(MessageBatchProcessor::new(Arc::clone(&queue), queue_config));
        
        // 默认启用错误恢复和熔断器
        let error_recovery = Arc::new(crate::error::ErrorRecovery::new(
            crate::error::RetryStrategy::default()
        ));
        let circuit_breaker = Arc::new(crate::error::CircuitBreaker::default());
        
        let this = Self {
            connection: Arc::clone(&connection),
            storage: Arc::clone(&storage),
            event_bus: Arc::clone(&event_bus),
            observers: Arc::new(ArcSwap::from_pointee(Vec::new())),
            user_id: Arc::clone(&user_id),
            // session_service 将在 with_session_service 中设置
            session_service: Arc::new(crate::service::SessionService::new(
                Arc::clone(&connection),
                Arc::clone(&storage),
                Arc::new(crate::service::SyncService::new(
                    Arc::clone(&connection),
                    Arc::clone(&storage),
                    Arc::clone(&event_bus),
                    Arc::clone(&user_id),
                )),
                Arc::clone(&event_bus),
                Arc::clone(&user_id),
            )),
            message_queue: Arc::clone(&queue),
            batch_processor: Arc::clone(&batch_processor),
            crypto: Arc::new(TokioRwLock::new(Arc::new(NoopCrypto))),
            #[cfg(feature = "extensions")]
            extension_manager: Arc::new(ExtensionManager::new()),
            error_recovery,
            circuit_breaker,
        };
        
        // 启动批处理循环（优化：使用 Vec::with_capacity 预分配）
        let processor = Arc::clone(&batch_processor);
        let queue_for_cleanup = Arc::clone(&queue);
        let svc = this.clone_for_queue();
        
        tokio_spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(50));
            let mut cleanup_interval = tokio::time::interval(Duration::from_secs(300)); // 每5分钟清理一次
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // 定期处理批次
                        if let Err(e) = processor.process_batch(|items| {
                            let svc = svc.clone_for_queue();
                            async move {
                                // 优化：预分配 Vec 容量
                                let mut results = Vec::with_capacity(items.len());
                                for item in items {
                                    match svc.send_message_with_options(
                                        &item.session_id,
                                        item.content.clone(),
                                        item.options.clone(),
                                    ).await {
                                        Ok(_msg_id) => results.push((item.message_id, true)),
                                        Err(_) => results.push((item.message_id, false)),
                                    }
                                }
                                Ok(results)
                            }
                        }).await {
                            error!(error = %e, "Batch processing error");
                        }
                    }
                    _ = cleanup_interval.tick() => {
                        // 定期清理过期的去重记录（防止内存泄漏）
                        queue_for_cleanup.cleanup_dedup(Duration::from_secs(3600)).await;
                    }
                    _ = queue_rx.recv() => {
                        // 队列有新消息，触发批处理
                        if let Err(e) = processor.process_batch(|items| {
                            let svc = svc.clone_for_queue();
                            async move {
                                // 优化：预分配 Vec 容量
                                let mut results = Vec::with_capacity(items.len());
                                for item in items {
                                    match svc.send_message_with_options(
                                        &item.session_id,
                                        item.content.clone(),
                                        item.options.clone(),
                                    ).await {
                                        Ok(_msg_id) => results.push((item.message_id, true)),
                                        Err(_) => results.push((item.message_id, false)),
                                    }
                                }
                                Ok(results)
                            }
                        }).await {
                            error!(error = %e, "Batch processing error");
                        }
                    }
                }
            }
        });
        
        this
    }

    /// 设置会话服务（用于更新会话信息）
    pub fn with_session_service(mut self, session_service: Arc<crate::service::SessionService>) -> Self {
        self.session_service = session_service;
        self
    }
    
    /// 设置扩展管理器（用于填充扩展信息）
    #[cfg(feature = "extensions")]
    pub fn with_extension_manager(mut self, extension_manager: Arc<ExtensionManager>) -> Self {
        self.extension_manager = extension_manager;
        self
    }
    
    /// 设置错误恢复器（用于自动重试）
    pub fn with_error_recovery(mut self, error_recovery: Arc<crate::error::ErrorRecovery>) -> Self {
        self.error_recovery = error_recovery;
        self
    }
    
    /// 设置熔断器（用于防止雪崩）
    pub fn with_circuit_breaker(mut self, circuit_breaker: Arc<crate::error::CircuitBreaker>) -> Self {
        self.circuit_breaker = circuit_breaker;
        self
    }

    /// 发送消息
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `content`: 消息内容
    /// 
    /// # 返回
    /// - `Result<String>`: 消息 ID
    pub async fn send_message(
        &self,
        session_id: &str,
        content: ProtoMessageContent,
    ) -> Result<String> {
        self.send_message_with_options(session_id, content, SendOptions::default()).await
    }

    pub async fn reply_message(
        &self,
        session_id: &str,
        reply_to_message_id: &str,
        content: ProtoMessageContent,
    ) -> Result<String> {
        let message_id = new_message_id();
        
        // 构建完整的 Message 对象
        let mut message = self.build_complete_message(
            message_id.clone(),
            session_id,
            content,
            &SendOptions::default(),
        ).await?;
        
        // 添加回复属性
        message.attributes.insert("reply_to".to_string(), reply_to_message_id.to_string());

        let mut message_bytes = Vec::new();
        message.encode(&mut message_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to encode message: {}", e))?;
        let crypto = self.crypto.read().await.clone();
        let payload = crypto.encrypt(&message_bytes)?;

        let mut metadata = std::collections::HashMap::new();
        metadata.insert("session_id".to_string(), session_id.as_bytes().to_vec());

        let message_cmd = MessageCommand {
            r#type: MessageCommandType::Send as i32,
            message_id: message_id.clone(),
            payload,
            metadata,
            seq: 0,
        };

        let frame = FrameBuilder::new()
            .with_message_command(message_cmd)
            .with_message_id(message_id.clone())
            .with_reliability(Reliability::AtLeastOnce)
            .build();

        self.connection.send_frame(&frame).await
            .context("Failed to send reply message frame")?;

        message.status = MessageStatus::Created as i32;
        self.storage.save_message(&message).await
            .context("Failed to save reply message to local storage")?;

        self.event_bus.publish(Event::Message(MessageEvent::MessageSent {
            message_id: message_id.clone(),
            session_id: session_id.to_string(),
        }));

        info!(message_id = %message_id, session_id = %session_id, "Reply message sent");
        Ok(message_id)
    }

    pub async fn add_thread_reply(
        &self,
        session_id: &str,
        thread_id: &str,
        content: ProtoMessageContent,
    ) -> Result<String> {
        let message_id = new_message_id();
        
        // 构建完整的 Message 对象
        let mut message = self.build_complete_message(
            message_id.clone(),
            session_id,
            content,
            &SendOptions::default(),
        ).await?;
        
        // 添加线程回复属性
        message.attributes.insert("thread_id".to_string(), thread_id.to_string());

        let mut message_bytes = Vec::new();
        message.encode(&mut message_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to encode message: {}", e))?;
        let crypto = self.crypto.read().await.clone();
        let payload = crypto.encrypt(&message_bytes)?;

        let mut metadata = std::collections::HashMap::new();
        metadata.insert("session_id".to_string(), session_id.as_bytes().to_vec());

        let message_cmd = MessageCommand {
            r#type: MessageCommandType::Send as i32,
            message_id: message_id.clone(),
            payload,
            metadata,
            seq: 0,
        };

        let frame = FrameBuilder::new()
            .with_message_command(message_cmd)
            .with_message_id(message_id.clone())
            .with_reliability(Reliability::AtLeastOnce)
            .build();

        self.connection.send_frame(&frame).await
            .context("Failed to send thread reply frame")?;

        message.status = MessageStatus::Created as i32;
        self.storage.save_message(&message).await
            .context("Failed to save thread reply to local storage")?;

        self.event_bus.publish(Event::Message(MessageEvent::MessageSent {
            message_id: message_id.clone(),
            session_id: session_id.to_string(),
        }));

        info!(message_id = %message_id, session_id = %session_id, "Thread reply sent");
        Ok(message_id)
    }

    /// 构建完整的 Message 对象（内部辅助方法）
    /// 
    /// 确保所有必要字段都被正确设置，包括：
    /// - 基本字段：id、session_id、sender_id、message_type、status、source、content、timestamp
    /// - 业务字段：business_type、session_type、receiver_id（如果是单聊）
    /// - 扩展字段：priority（如果提供）
    async fn build_complete_message(
        &self,
        message_id: String,
        session_id: &str,
        content: ProtoMessageContent,
        options: &SendOptions,
    ) -> Result<Message> {
        let user_id = {
            let guard = self.user_id.read().await;
            guard.clone()
        };
        
        let mut message = Message::default();
        // 基本字段
        message.id = message_id;
        message.session_id = session_id.to_string();
        message.sender_id = user_id;
        message.message_type = Self::message_type_from_content(&content);
        message.status = MessageStatus::Created as i32;
        message.source = MessageSource::User as i32;
        message.content = Some(content);
        message.timestamp = Some(Timestamp {
            seconds: chrono::Utc::now().timestamp(),
            nanos: 0,
        });
        
        // 业务字段：默认值
        message.business_type = "chat".to_string();
        
        // 从会话信息中获取 session_type 和 receiver_id（如果是单聊）
        if let Ok(session) = self.session_service.get_session(session_id).await {
            // 使用 session_type 判断会话类型
            match session.session_type.as_str() {
                "single" => {
                    // 单聊：从 metadata 获取 peer_id
                    if let Some(peer_id) = session.metadata.get("peer_id") {
                        message.session_type = "single".to_string();
                        message.receiver_id = peer_id.clone();
                        message.receiver_ids = Vec::new();
                        message.group_id = String::new();
                    }
                }
                "group" | "channel" => {
                    // 群聊：session_id 通常就是 group_id，或者从 metadata 获取
                    message.session_type = session.session_type.clone();
                    message.group_id = session.metadata.get("group_id")
                        .cloned()
                        .unwrap_or_else(|| session_id.to_string());
                    message.receiver_id = String::new();
                    message.receiver_ids = Vec::new();
                }
                _ => {
                    // 其他类型：保持默认值
                    message.session_type = session.session_type.clone();
                }
            }
        }
        
        // 扩展字段
        if let Some(priority) = options.priority {
            message.extra.insert("priority".to_string(), priority.to_string());
        }
        
        Ok(message)
    }

    pub async fn send_message_with_options(
        &self,
        session_id: &str,
        content: ProtoMessageContent,
        options: SendOptions,
    ) -> Result<String> {
        // 1. 生成消息 ID
        let message_id = new_message_id();
        
        // 2. 构建完整的 Message 对象
        let message = self.build_complete_message(
            message_id.clone(),
            session_id,
            content,
            &options,
        ).await?;
        
        let mut message_bytes = Vec::new();
        message.encode(&mut message_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to encode message: {}", e))?;
        let crypto = self.crypto.read().await.clone();
        let payload = crypto.encrypt(&message_bytes)?;
        
        // 4. 构建 Frame（优化：减少克隆，使用引用，预分配容量）
        let mut metadata = std::collections::HashMap::with_capacity(2);
        metadata.insert("session_id".to_string(), session_id.as_bytes().to_vec());
        // 若会话是单聊且保存了对端ID，则附带目标用户用于定向推送
        if let Ok(s) = self.session_service.get_session(session_id).await {
            if let Some(peer) = s.metadata.get("peer_id") {
                metadata.insert("target_user_id".to_string(), peer.as_bytes().to_vec());
            }
        }
        
        let message_cmd = MessageCommand {
            r#type: MessageCommandType::Send as i32,
            message_id: message_id.clone(),
            payload,
            metadata,
            seq: 0, // seq 由服务端生成
        };
        
        let frame = FrameBuilder::new()
            .with_message_command(message_cmd)
            .with_message_id(message_id.clone())
            .with_reliability(options.reliability)
            .build();
        
        // 5. 并行执行：发送消息和保存到本地存储（优化：减少等待时间）
        // 优化：使用错误恢复机制和熔断器（如果已配置）
        let storage_clone = Arc::clone(&self.storage);
        let message_for_storage = message.clone();
        
        // 检查熔断器
        if self.circuit_breaker.is_open().await {
            return Err(anyhow::anyhow!("Circuit breaker is open, message sending is temporarily disabled"));
        }
        
        // 发送消息（使用错误恢复机制）
        let connection_clone = Arc::clone(&self.connection);
        let frame_clone = frame.clone();
        let circuit_breaker_clone = Arc::clone(&self.circuit_breaker);
        
        self.error_recovery.execute_with_retry(|| {
            let conn = Arc::clone(&connection_clone);
            let f = frame_clone.clone();
            let cb = Arc::clone(&circuit_breaker_clone);
            async move {
                let result = conn.send_frame(&f).await;
                match result {
                    Ok(_) => {
                        cb.record_success().await;
                        Ok(())
                    }
                    Err(e) => {
                        cb.record_failure().await;
                        Err(crate::error::SDKError::connection(
                            flare_core::common::error::code::ErrorCode::NetworkError,
                            format!("Failed to send message frame: {}", e)
                        ))
                    }
                }
            }
        }).await
        .map_err(|e| anyhow::anyhow!("{}", e))?;
        
        // 保存到本地存储（并行执行）
        let save_result = storage_clone.save_message(&message_for_storage).await;
        
        save_result.context("Failed to save message to local storage")?;
        
        // 6. 更新消息状态为 Sent（发送成功）
        // 注意：这里更新为 Sent 而不是 Created，因为消息已经成功发送到服务器
        self.update_message_status(&message_id, MessageStatus::Sent).await
            .context("Failed to update message status to Sent")?;
        
        // 7. 发布消息发送事件（优化：使用引用避免额外克隆）
        self.event_bus.publish(Event::Message(MessageEvent::MessageSent {
            message_id: message_id.clone(),
            session_id: session_id.to_string(),
        }));
        
        info!(
            message_id = %message_id,
            session_id = %session_id,
            "Message sent"
        );
        
        Ok(message_id)
    }

    pub async fn enqueue_message(&self, session_id: String, content: ProtoMessageContent, options: SendOptions) -> Result<()> {
        // 使用优先级队列
        let message_id = new_message_id();
        self.message_queue.enqueue(message_id, session_id, content, options).await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(())
    }

    fn clone_for_queue(&self) -> Self {
        Self {
            connection: Arc::clone(&self.connection),
            storage: Arc::clone(&self.storage),
            event_bus: Arc::clone(&self.event_bus),
            observers: Arc::clone(&self.observers),
            user_id: Arc::clone(&self.user_id),
            session_service: Arc::clone(&self.session_service),
            message_queue: Arc::clone(&self.message_queue),
            batch_processor: Arc::clone(&self.batch_processor),
            crypto: Arc::clone(&self.crypto),
            #[cfg(feature = "extensions")]
            extension_manager: Arc::clone(&self.extension_manager),
            error_recovery: Arc::clone(&self.error_recovery),
            circuit_breaker: Arc::clone(&self.circuit_breaker),
        }
    }

    pub async fn set_crypto(&self, crypto: Arc<dyn CryptoService>) {
        let mut guard = self.crypto.write().await;
        *guard = crypto;
    }

    pub async fn decrypt_payload(&self, data: &[u8]) -> Result<Vec<u8>> {
        // 优化：减少锁持有时间，直接使用引用
        let crypto = self.crypto.read().await;
        crypto.decrypt(data)
    }

    /// 接收消息（从连接层）
    /// 
    /// # 参数
    /// - `message`: 接收到的消息
    pub async fn on_message_received(&self, message: Message) -> Result<()> {
        // 优化：使用 Arc<Message> 减少克隆
        let message_arc = Arc::new(message);
        
        eprintln!("[MessageService] on_message_received: message_id={}, session_id={}, sender_id={}", 
            message_arc.id, message_arc.session_id, message_arc.sender_id);
        
        info!(
            message_id = %message_arc.id,
            session_id = %message_arc.session_id,
            sender_id = %message_arc.sender_id,
            "Message received"
        );
        
        // 确保会话存在（对方先发消息的情况）
        if self.storage.get_session(&message_arc.session_id).await?.is_none() {
            let display = Some(message_arc.sender_id.clone());
            let _ = self.session_service.create_session(
                Some(message_arc.session_id.clone()),
                "single".to_string(),
                "chat".to_string(),
                display,
            ).await;
        }
        
        // 1. 确保收到的消息状态为 Delivered（已送达），而不是 Created（发送中）
        let mut message_to_save = (*message_arc).clone();
        if message_to_save.status == MessageStatus::Created as i32 {
            message_to_save.status = MessageStatus::Delivered as i32;
        }
        
        // 2. 保存到本地存储
        self.storage.save_message(&message_to_save).await
            .context("Failed to save received message to local storage")?;
        
        // 3. 更新消息状态（已接收）
        let state = MessageState::new();
        let user_id = {
            let guard = self.user_id.read().await;
            guard.clone()
        };
        self.storage.save_message_state(
            &user_id,
            &message_arc.id,
            state,
        ).await
        .context("Failed to save message state")?;
        
        // 3. 分发给消息观察者（统一的消息处理接口）
        // 优化：使用 ArcSwap 无锁读取，使用 Arc<Message> 减少克隆
        let observers = self.observers.load();
        for observer in observers.iter() {
            // 检查是否支持此消息类型
            let supported_types = observer.supported_message_types();
            if supported_types.is_empty() {
                // 空列表表示支持所有类型
                if let Err(e) = observer.on_message(message_arc.as_ref()).await {
                    error!(
                        observer = observer.name(),
                        error = %e,
                        message_id = %message_arc.id,
                        "Observer failed to handle message"
                    );
                }
            } else {
                // 检查消息类型是否在支持列表中
                // 支持两种匹配方式：
                // 1. 字符串匹配（如 "1" 匹配 MessageType::Text）
                // 2. 枚举名称匹配（如 "Text" 匹配 MessageType::Text）
                let message_type_str = format!("{}", message_arc.message_type);
                let message_type_name = Self::message_type_to_name(message_arc.message_type);
                
                let should_handle = supported_types.iter().any(|t| {
                    t == &message_type_str || t == &message_type_name
                });
                
                if should_handle {
                    if let Err(e) = observer.on_message(message_arc.as_ref()).await {
                        error!(
                            observer = observer.name(),
                            error = %e,
                            message_id = %message_arc.id,
                            "Observer failed to handle message"
                        );
                    }
                }
            }
        }
        
        // 使用 Arc 中的消息 ID 和 session_id
        let message_id = message_arc.id.clone();
        let session_id = message_arc.session_id.clone();
        
        // 4. 发布消息接收事件
        eprintln!("[MessageService] 发布 MessageReceived 事件: message_id={}, session_id={}", message_id, session_id);
        self.event_bus.publish(Event::Message(MessageEvent::MessageReceived {
            message_id,
            session_id,
        }));
        eprintln!("[MessageService] MessageReceived 事件已发布");
        
        Ok(())
    }

    pub async fn set_user_id(&self, user_id: String) {
        let mut guard = self.user_id.write().await;
        *guard = user_id;
    }

    /// 注册消息观察者（统一的消息处理接口）
    /// 
    /// # 参数
    /// - `observer`: 消息观察者
    /// 
    /// # 示例
    /// ```rust,no_run
    /// struct MyObserver;
    /// 
    /// #[async_trait::async_trait]
    /// impl MessageObserver for MyObserver {
    ///     async fn on_message(&self, message: &Message) -> Result<bool> {
    ///         println!("收到消息: {}", message.id);
    ///         Ok(false) // 继续传递给其他观察者
    ///     }
    ///     
    ///     fn name(&self) -> &str {
    ///         "MyObserver"
    ///     }
    /// }
    /// 
    /// let observer = Arc::new(MyObserver);
    /// message_service.register_observer(observer).await;
    /// ```
    pub async fn register_observer(&self, observer: crate::observer::ArcMessageObserver) {
        // 优化：使用 ArcSwap 原子更新，不影响读取
        // 先获取当前值，修改后原子替换
        let current = self.observers.load();
        let mut new = (**current).clone();
        new.push(observer);
        // 按优先级排序（优先级小的在前）
        new.sort_by_key(|o| o.priority());
        self.observers.store(Arc::new(new));
        debug!("Message observer registered");
    }

    /// 移除消息观察者
    /// 
    /// # 参数
    /// - `observer`: 要移除的观察者
    pub async fn unregister_observer(&self, observer: &crate::observer::ArcMessageObserver) {
        // 优化：使用 ArcSwap 原子更新
        let current = self.observers.load();
        let mut new = (**current).clone();
        new.retain(|o| !std::sync::Arc::ptr_eq(o, observer));
        self.observers.store(Arc::new(new));
        debug!("Message observer unregistered");
    }

    /// 获取所有注册的观察者数量
    pub async fn observer_count(&self) -> usize {
        // 优化：无锁读取
        self.observers.load().len()
    }

    /// 清空所有消息观察者
    pub async fn clear_observers(&self) {
        // 优化：使用 ArcSwap 原子更新
        self.observers.store(Arc::new(Vec::<crate::observer::ArcMessageObserver>::new()));
        debug!("All message observers cleared");
    }

    /// 获取本地消息（返回基础 Message）
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `limit`: 返回的最大消息数量
    /// - `cursor`: 可选游标，用于分页
    /// 
    /// # 返回
    /// - `Result<Vec<Message>>`: 消息列表
    pub async fn get_local_messages(
        &self,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<Message>> {
        self.storage.get_messages(session_id, limit, cursor).await
            .context("Failed to get local messages")
    }
    
    /// 获取本地消息（返回带扩展信息的 ExtendedMessage）
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `limit`: 返回的最大消息数量
    /// - `cursor`: 可选游标，用于分页
    /// 
    /// # 返回
    /// - `Result<Vec<ExtendedMessage>>`: 带扩展信息的消息列表
    pub async fn get_local_messages_extended(
        &self,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<ExtendedMessage>> {
        // 1. 获取基础消息
        let messages = self.storage.get_messages(session_id, limit, cursor).await
            .context("Failed to get local messages")?;
        
        // 2. 转换为 ExtendedMessage，并加载扩展信息
        let mut extended_messages: Vec<ExtendedMessage> = Vec::with_capacity(messages.len());
        for msg in messages {
            // 尝试从存储加载扩展信息
            let extension = self.storage.get_message_extension(&msg.id).await
                .unwrap_or_else(|_| None)
                .unwrap_or_default();
            extended_messages.push(ExtendedMessage::new(msg, extension));
        }
        
        // 3. 批量填充扩展信息
        #[cfg(feature = "extensions")]
        {
            self.extension_manager.batch_enrich_messages(&mut extended_messages).await
                .context("Failed to enrich messages with extension info")?;
            
            // 4. 保存填充后的扩展信息到存储（优化：并行保存，不阻塞）
            let storage_clone = Arc::clone(&self.storage);
            for msg in extended_messages.iter() {
                let msg_id = msg.message.id.clone();
                let ext = msg.extension.clone();
                let storage = Arc::clone(&storage_clone);
                tokio_spawn(async move {
                    if let Err(e) = storage.save_message_extension(&msg_id, &ext).await {
                        tracing::warn!(error = %e, message_id = %msg_id, "Failed to save message extension");
                    }
                });
            }
        }
        
        Ok(extended_messages)
    }

    /// 删除消息（本地）
    /// 
    /// # 参数
    /// - `message_id`: 消息 ID
    pub async fn delete_message(&self, message_id: &str) -> Result<()> {
        // 1. 标记消息状态为已删除
        let mut state = MessageState::new();
        state = state.mark_as_deleted();
        self.storage.save_message_state(
            &self.user_id.read().await,
            message_id,
            state,
        ).await
        .context("Failed to mark message as deleted")?;
        
        // 2. 从本地存储删除（软删除）
        self.storage.delete_message(message_id).await
            .context("Failed to delete message from local storage")?;
        
        // 3. 发布消息删除事件（MessageEvent 中没有 Deleted，暂时跳过）
        // self.event_bus.publish(Event::Message(MessageEvent::Deleted {
        //     message_id: message_id.to_string(),
        // }));
        
        info!(message_id = %message_id, "Message deleted");
        Ok(())
    }

    /// 撤回消息
    /// 
    /// # 参数
    /// - `message_id`: 消息 ID
    pub async fn recall_message(&self, message_id: &str) -> Result<()> {
        // 1. 获取消息
        let message = self.storage.get_message(message_id).await?
            .context("Message not found")?;
        
        // 2. 验证消息发送者
        if message.sender_id != *self.user_id.read().await {
            return Err(anyhow::anyhow!("Only message sender can recall message"));
        }
        
        // 3. 构建撤回命令 Frame
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("session_id".to_string(), message.session_id.as_bytes().to_vec());
        
        let system_cmd = SystemCommand {
            r#type: SystemCommandType::Event as i32, // 使用 EVENT 类型
            format: 0,
            message: "recall".to_string(),
            metadata,
            data: message_id.as_bytes().to_vec(),
            compression: String::new(), // 仅在 CONNECT_ACK 时使用
            encryption: String::new(), // 仅在 CONNECT_ACK 时使用
        };
        
        let frame = FrameBuilder::new()
            .with_system_command(system_cmd)
            .with_message_id(message_id.to_string())
            .with_reliability(Reliability::AtLeastOnce)
            .build();
        
        // 4. 通过连接管理器发送撤回命令
        self.connection.send_frame(&frame).await
            .context("Failed to send recall command")?;
        
        // 5. 更新本地消息状态（标记为已撤回）
        let mut message = message;
        message.status = MessageStatus::Recalled as i32;
        self.storage.save_message(&message).await
            .context("Failed to update message status")?;
        
        // 6. 发布消息撤回事件
        self.event_bus.publish(Event::Message(MessageEvent::MessageRecalled {
            message_id: message_id.to_string(),
            session_id: message.session_id,
        }));
        
        info!(message_id = %message_id, "Message recalled");
        Ok(())
    }

    pub async fn add_reaction(&self, session_id: &str, message_id: &str, emoji: &str) -> Result<()> {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("session_id".to_string(), session_id.as_bytes().to_vec());
        metadata.insert("emoji".to_string(), emoji.as_bytes().to_vec());
        let system_cmd = SystemCommand {
            r#type: SystemCommandType::Event as i32,
            format: 0,
            message: "reaction_add".to_string(),
            metadata,
            data: message_id.as_bytes().to_vec(),
            compression: String::new(), // 仅在 CONNECT_ACK 时使用
            encryption: String::new(), // 仅在 CONNECT_ACK 时使用
        };
        let frame = FrameBuilder::new().with_system_command(system_cmd).with_message_id(message_id.to_string()).with_reliability(Reliability::AtLeastOnce).build();
        self.connection.send_frame(&frame).await?;
        if let Some(mut msg) = self.storage.get_message(message_id).await? {
            let mut count = 0i32;
            let key = format!("reaction::{}", emoji);
            if let Some(v) = msg.attributes.get(&key) { if let Ok(n) = v.parse::<i32>() { count = n; } }
            msg.attributes.insert(key, (count.saturating_add(1)).to_string());
            self.storage.save_message(&msg).await?;
        }
        Ok(())
    }

    pub async fn remove_reaction(&self, session_id: &str, message_id: &str, emoji: &str) -> Result<()> {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("session_id".to_string(), session_id.as_bytes().to_vec());
        metadata.insert("emoji".to_string(), emoji.as_bytes().to_vec());
        let system_cmd = SystemCommand {
            r#type: SystemCommandType::Event as i32,
            format: 0,
            message: "reaction_remove".to_string(),
            metadata,
            data: message_id.as_bytes().to_vec(),
            compression: String::new(), // 仅在 CONNECT_ACK 时使用
            encryption: String::new(), // 仅在 CONNECT_ACK 时使用
        };
        let frame = FrameBuilder::new().with_system_command(system_cmd).with_message_id(message_id.to_string()).with_reliability(Reliability::AtLeastOnce).build();
        self.connection.send_frame(&frame).await?;
        if let Some(mut msg) = self.storage.get_message(message_id).await? {
            let mut count = 0i32;
            let key = format!("reaction::{}", emoji);
            if let Some(v) = msg.attributes.get(&key) { if let Ok(n) = v.parse::<i32>() { count = n; } }
            let new_count = count.saturating_sub(1);
            msg.attributes.insert(key, new_count.to_string());
            self.storage.save_message(&msg).await?;
        }
        Ok(())
    }

    pub async fn edit_message(&self, session_id: &str, message_id: &str, content: ProtoMessageContent, attributes: Option<std::collections::HashMap<String, String>>) -> Result<()> {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("session_id".to_string(), session_id.as_bytes().to_vec());
        let mut message = self.storage.get_message(message_id).await?.ok_or_else(|| anyhow::anyhow!("Message not found"))?;
        message.content = Some(content);
        if let Some(attrs) = attributes { for (k, v) in attrs { message.attributes.insert(k, v); } }
        self.storage.save_message(&message).await?;
        let mut payload_buf = Vec::new();
        message.encode(&mut payload_buf).map_err(|e| anyhow::anyhow!("{}", e))?;
        let crypto = self.crypto.read().await.clone();
        let payload = crypto.encrypt(&payload_buf)?;
        let system_cmd = SystemCommand {
            r#type: SystemCommandType::Event as i32,
            format: 0,
            message: "edit".to_string(),
            metadata,
            data: payload,
            compression: String::new(), // 仅在 CONNECT_ACK 时使用
            encryption: String::new(), // 仅在 CONNECT_ACK 时使用
        };
        let frame = FrameBuilder::new().with_system_command(system_cmd).with_message_id(message_id.to_string()).with_reliability(Reliability::AtLeastOnce).build();
        self.connection.send_frame(&frame).await?;
        Ok(())
    }

    pub async fn send_read_receipt(&self, session_id: &str, message_id: &str, seq: Option<i64>) -> Result<()> {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("session_id".to_string(), session_id.as_bytes().to_vec());
        if let Some(s) = seq { metadata.insert("seq".to_string(), s.to_string().into_bytes()); }
        let system_cmd = SystemCommand {
            r#type: SystemCommandType::Event as i32,
            format: 0,
            message: "read".to_string(),
            metadata,
            data: message_id.as_bytes().to_vec(),
            compression: String::new(), // 仅在 CONNECT_ACK 时使用
            encryption: String::new(), // 仅在 CONNECT_ACK 时使用
        };
        let frame = FrameBuilder::new().with_system_command(system_cmd).with_message_id(message_id.to_string()).with_reliability(Reliability::AtLeastOnce).build();
        self.connection.send_frame(&frame).await?;
        Ok(())
    }

    pub async fn save_read_state(&self, user_id: &str, message_id: &str, state: MessageState) -> Result<()> {
        self.storage.save_message_state(user_id, message_id, state).await
    }

    /// 更新消息状态（当收到 ACK 时）
    /// 
    /// # 参数
    /// - `message_id`: 消息 ID
    /// - `status`: 新状态
    pub async fn update_message_status(
        &self,
        message_id: &str,
        status: MessageStatus,
    ) -> Result<()> {
        if let Some(mut message) = self.storage.get_message(message_id).await? {
            let old_status = message.status;
            message.status = status as i32;
            self.storage.save_message(&message).await
                .context("Failed to update message status")?;
            
            // 发布消息状态更新事件，通知 UI 更新
            if old_status != status as i32 {
                self.event_bus.publish(Event::Message(MessageEvent::MessageStatusUpdated {
                    message_id: message_id.to_string(),
                    session_id: message.session_id.clone(),
                    status: status as i32,
                }));
                
                debug!(
                    message_id = %message_id,
                    session_id = %message.session_id,
                    old_status = old_status,
                    new_status = status as i32,
                    "Message status updated"
                );
            }
        }
        
        Ok(())
    }

    /// 将消息类型枚举值转换为名称
    pub(crate) fn message_type_to_name(message_type: i32) -> String {
        match message_type {
            x if x == MessageType::Text as i32 => "Text".to_string(),
            x if x == MessageType::Image as i32 => "Image".to_string(),
            x if x == MessageType::Video as i32 => "Video".to_string(),
            x if x == MessageType::Audio as i32 => "Audio".to_string(),
            x if x == MessageType::File as i32 => "File".to_string(),
            x if x == MessageType::Location as i32 => "Location".to_string(),
            x if x == MessageType::Card as i32 => "Card".to_string(),
            x if x == MessageType::Custom as i32 => "Custom".to_string(),
            x if x == MessageType::Notification as i32 => "Notification".to_string(),
            x if x == MessageType::Typing as i32 => "Typing".to_string(),
            x if x == MessageType::Recall as i32 => "Recall".to_string(),
            x if x == MessageType::Read as i32 => "Read".to_string(),
            x if x == MessageType::Forward as i32 => "Forward".to_string(),
            x if x == MessageType::Vote as i32 => "Vote".to_string(),
            x if x == MessageType::Task as i32 => "Task".to_string(),
            x if x == MessageType::Schedule as i32 => "Schedule".to_string(),
            x if x == MessageType::Announcement as i32 => "Announcement".to_string(),
            _ => format!("Unknown({})", message_type),
        }
    }

    /// 从消息内容推断消息类型
    /// 
    /// 这是一个辅助函数，用于消除代码重复
    fn message_type_from_content(content: &ProtoMessageContent) -> i32 {
        match content.content.as_ref() {
            Some(ProtoContent::Text(_)) => MessageType::Text as i32,
            Some(ProtoContent::Image(_)) => MessageType::Image as i32,
            Some(ProtoContent::Video(_)) => MessageType::Video as i32,
            Some(ProtoContent::Audio(_)) => MessageType::Audio as i32,
            Some(ProtoContent::File(_)) => MessageType::File as i32,
            Some(ProtoContent::Location(_)) => MessageType::Location as i32,
            Some(ProtoContent::Card(_)) => MessageType::Card as i32,
            Some(ProtoContent::Notification(_)) => MessageType::Notification as i32,
            Some(ProtoContent::Custom(_)) => MessageType::Custom as i32,
            Some(ProtoContent::Forward(_)) => MessageType::Forward as i32,
            Some(ProtoContent::Typing(_)) => MessageType::Typing as i32,
            Some(ProtoContent::Vote(_)) => MessageType::Vote as i32,
            Some(ProtoContent::Task(_)) => MessageType::Task as i32,
            Some(ProtoContent::Schedule(_)) => MessageType::Schedule as i32,
            Some(ProtoContent::Announcement(_)) => MessageType::Announcement as i32,
            Some(ProtoContent::SystemEvent(_)) => MessageType::Notification as i32,
            None => MessageType::Text as i32,
        }
    }
}

// 所有消息处理都通过 MessageObserver 接口实现
// 用户可以直接实现 MessageObserver trait，提供最大的灵活性和扩展性

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::storage_trait::StorageBackend;
    use async_trait::async_trait;
    
    // Mock storage for testing
    struct MockStorage;
    impl crate::storage::storage_trait::StorageSyncBounds for MockStorage {}
    
    #[async_trait]
    impl StorageBackend for MockStorage {
        async fn save_message(&self, _message: &Message) -> Result<()> {
            Ok(())
        }
        
        async fn get_message(&self, _message_id: &str) -> Result<Option<Message>> {
            Ok(None)
        }
        
        async fn get_messages(
            &self,
            _session_id: &str,
            _limit: usize,
            _cursor: Option<String>,
        ) -> Result<Vec<Message>> {
            Ok(Vec::new())
        }
        
        async fn get_messages_by_seq(
            &self,
            _session_id: &str,
            _after_seq: i64,
            _limit: usize,
        ) -> Result<Vec<Message>> {
            Ok(Vec::new())
        }
        
        async fn get_max_seq(&self, _session_id: &str) -> Result<Option<i64>> {
            Ok(None)
        }
        
        async fn delete_message(&self, _message_id: &str) -> Result<()> {
            Ok(())
        }
        
        async fn save_session(&self, _session: &crate::model::SessionSummary) -> Result<()> {
            Ok(())
        }
        
        async fn get_session(&self, _session_id: &str) -> Result<Option<crate::model::SessionSummary>> {
            Ok(None)
        }
        
        async fn get_sessions(&self, _filter: crate::storage::storage_trait::SessionFilter) -> Result<Vec<crate::model::SessionSummary>> {
            Ok(Vec::new())
        }
        
        async fn update_session(
            &self,
            _session_id: &str,
            _updates: crate::storage::storage_trait::SessionUpdate,
        ) -> Result<()> {
            Ok(())
        }
        
        async fn delete_session(&self, _session_id: &str) -> Result<()> {
            Ok(())
        }
        
        async fn save_sync_cursor(&self, _session_id: &str, _cursor: &crate::model::SyncCursor) -> Result<()> {
            Ok(())
        }
        
        async fn get_sync_cursor(&self, _session_id: &str) -> Result<Option<crate::model::SyncCursor>> {
            Ok(None)
        }
        
        async fn get_all_sync_cursors(&self) -> Result<Vec<crate::model::SyncCursor>> {
            Ok(Vec::new())
        }
        
        async fn save_message_state(
            &self,
            _user_id: &str,
            _message_id: &str,
            _state: MessageState,
        ) -> Result<()> {
            Ok(())
        }
        
        async fn get_message_state(
            &self,
            _user_id: &str,
            _message_id: &str,
        ) -> Result<Option<MessageState>> {
            Ok(None)
        }
        
        async fn batch_check_deleted(
            &self,
            _user_id: &str,
            _message_ids: &[String],
        ) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
        
        // 扩展信息方法
        async fn save_message_extension(
            &self,
            _message_id: &str,
            _extension: &crate::model::MessageExtension,
        ) -> Result<()> {
            Ok(())
        }
        
        async fn get_message_extension(
            &self,
            _message_id: &str,
        ) -> Result<Option<crate::model::MessageExtension>> {
            Ok(None)
        }
        
        async fn save_session_extension(
            &self,
            _session_id: &str,
            _extension: &crate::model::SessionExtension,
        ) -> Result<()> {
            Ok(())
        }
        
        async fn get_session_extension(
            &self,
            _session_id: &str,
        ) -> Result<Option<crate::model::SessionExtension>> {
            Ok(None)
        }
        
        async fn batch_get_message_extensions(
            &self,
            _message_ids: &[String],
        ) -> Result<Vec<(String, crate::model::MessageExtension)>> {
            Ok(Vec::new())
        }
        
        async fn batch_get_session_extensions(
            &self,
            _session_ids: &[String],
        ) -> Result<Vec<(String, crate::model::SessionExtension)>> {
            Ok(Vec::new())
        }
    }
    
    #[tokio::test]
    async fn test_message_service_creation() {
        let bus = EventBus::new();
        let config = crate::config::ClientConfig::builder()
            .server_url("wss://example.com")
            .user_id("u1")
            .device_id("d1")
            .build()
            .unwrap();
        let connection = Arc::new(crate::connection::ConnectionManager::new(Arc::new(tokio::sync::RwLock::new(config)), Arc::new(bus)));
        let storage: Arc<dyn StorageBackend> = Arc::new(MockStorage);
        let service = MessageService::new(Arc::clone(&connection), Arc::clone(&storage), Arc::new(EventBus::new()), Arc::new(tokio::sync::RwLock::new(String::new())));
        service.set_user_id("user-123".to_string()).await;
        assert_eq!(service.user_id.read().await.as_str(), "user-123");
    }
    
    #[test]
    fn test_message_type_to_name() {
        assert_eq!(
            MessageService::message_type_to_name(MessageType::Text as i32),
            "Text"
        );
        assert_eq!(
            MessageService::message_type_to_name(MessageType::Image as i32),
            "Image"
        );
        assert_eq!(
            MessageService::message_type_to_name(MessageType::Custom as i32),
            "Custom"
        );
        assert_eq!(
            MessageService::message_type_to_name(999),
            "Unknown(999)"
        );
    }
}
#[cfg(target_arch = "wasm32")]
fn new_message_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    // 使用 chrono 获取时间戳，避免 unwrap
    let ts = chrono::Utc::now().timestamp_millis();
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("msg-{}-{}", ts, c)
}
#[cfg(not(target_arch = "wasm32"))]
fn new_message_id() -> String { Uuid::new_v4().to_string() }
