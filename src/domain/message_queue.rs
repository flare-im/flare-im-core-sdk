//! 消息队列模块
//!
//! 职责：接收消息 -> 本地队列 -> 业务处理
//! 提高 SDK 稳定性和可靠性

use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use crate::domain::message::Message;
use tracing::{error, info, warn};
use std::collections::HashMap;
use chrono::Utc;

/// 消息队列
///
/// 用于缓冲接收到的消息，提高 SDK 稳定性
pub struct MessageQueue {
    /// 消息接收通道（发送端）
    sender: mpsc::UnboundedSender<QueuedMessage>,
    
    /// 消息处理通道（接收端）
    receiver: Arc<RwLock<Option<mpsc::UnboundedReceiver<QueuedMessage>>>>,
    
    /// 消息去重映射（防止重复处理）
    seen_messages: Arc<RwLock<HashMap<String, chrono::DateTime<Utc>>>>,
    
    /// 队列统计
    stats: Arc<RwLock<QueueStats>>,
}

/// 队列中的消息
#[derive(Debug, Clone)]
pub struct QueuedMessage {
    /// 消息
    pub message: Message,
    
    /// 接收时间
    pub received_at: chrono::DateTime<Utc>,
    
    /// 重试次数
    pub retry_count: u32,
    
    /// 优先级（数字越大优先级越高）
    pub priority: u8,
}

/// 队列统计信息
#[derive(Debug, Clone, Default)]
pub struct QueueStats {
    /// 总接收消息数
    pub total_received: u64,
    
    /// 总处理消息数
    pub total_processed: u64,
    
    /// 总丢弃消息数（队列满）
    pub total_dropped: u64,
    
    /// 总重复消息数
    pub total_duplicates: u64,
    
    /// 当前队列长度
    pub current_queue_size: usize,
}

impl MessageQueue {
    /// 创建新的消息队列
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        
        Self {
            sender,
            receiver: Arc::new(RwLock::new(Some(receiver))),
            seen_messages: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(QueueStats::default())),
        }
    }
    
    /// 将消息加入队列
    ///
    /// 返回 true 表示成功加入队列，false 表示队列已满或消息重复
    pub async fn enqueue(&self, message: Message, priority: u8) -> bool {
        // 性能优化：快速路径检查（避免不必要的锁）
        if let Some(ref server_id) = message.server_id {
            if self.is_duplicate(server_id).await {
                let mut stats = self.stats.write().await;
                stats.total_duplicates += 1;
                return false;
            }
        } else {
            // 如果没有 server_id，使用 client_msg_id 检查
            if self.is_duplicate(&message.client_msg_id).await {
                let mut stats = self.stats.write().await;
                stats.total_duplicates += 1;
                return false;
            }
        }
        
        // 记录消息 ID（用于去重）
        if let Some(ref server_id) = message.server_id {
            self.mark_seen(server_id).await;
        }
        
        // 创建队列消息
        let queued_message = QueuedMessage {
            message,
            received_at: Utc::now(),
            retry_count: 0,
            priority,
        };
        
        // 发送到队列（非阻塞操作）
        match self.sender.send(queued_message) {
            Ok(_) => {
                let mut stats = self.stats.write().await;
                stats.total_received += 1;
                stats.current_queue_size += 1;
                true
            }
            Err(_) => {
                // 队列已满或接收端已关闭
                let mut stats = self.stats.write().await;
                stats.total_dropped += 1;
                false
            }
        }
    }
    
    /// 批量将消息加入队列（性能优化）
    ///
    /// 返回成功入队的消息数量
    pub async fn enqueue_batch(&self, messages: Vec<(Message, u8)>) -> usize {
        if messages.is_empty() {
            return 0;
        }
        
        let message_count = messages.len();
        let now = Utc::now();
        
        // 性能优化：批量去重检查（一次性获取写锁）
        let mut seen = self.seen_messages.write().await;
        let window = chrono::Duration::seconds(crate::domain::constants::message::MESSAGE_DEDUP_WINDOW_SECONDS);
        let cutoff = now - window;
        seen.retain(|_, &mut timestamp| timestamp > cutoff);
        
        let mut enqueued = 0;
        let mut duplicates = 0;
        
        // 性能优化：预分配容量，减少重新分配
        let mut queued_messages = Vec::with_capacity(message_count);
        
        for (message, priority) in messages {
            // 快速去重检查
            if let Some(ref server_id) = message.server_id {
                if seen.contains_key(server_id) {
                    duplicates += 1;
                    continue;
                }
            }
            
            // 标记已见（避免克隆，直接移动）
            if let Some(server_id) = message.server_id.clone() {
                seen.insert(server_id, now);
            }
            
            // 创建队列消息
            queued_messages.push(QueuedMessage {
                message,
                received_at: now,
                retry_count: 0,
                priority,
            });
        }
        
        // 批量发送到队列（减少锁竞争）
        for queued_message in queued_messages {
            if self.sender.send(queued_message).is_ok() {
                enqueued += 1;
            }
        }
        
        // 更新统计（一次性更新）
        let mut stats = self.stats.write().await;
        stats.total_received += enqueued as u64;
        stats.current_queue_size += enqueued;
        stats.total_duplicates += duplicates as u64;
        
        enqueued
    }
    
    /// 从队列接收消息（非阻塞）
    /// 
    /// 性能优化：减少锁持有时间
    pub async fn try_receive(&self) -> Option<QueuedMessage> {
        let msg_opt = {
            let mut receiver_guard = self.receiver.write().await;
            if let Some(ref mut receiver) = *receiver_guard {
                match receiver.try_recv() {
                    Ok(msg) => Some(msg),
                    Err(mpsc::error::TryRecvError::Empty) => return None,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        error!("Message queue receiver disconnected");
                        return None;
                    }
                }
            } else {
                return None;
            }
        };
        
        let msg = match msg_opt {
            Some(msg) => msg,
            None => return None,
        };
        
        // 更新统计（在锁外）
        let mut stats = self.stats.write().await;
        stats.total_processed += 1;
        stats.current_queue_size = stats.current_queue_size.saturating_sub(1);
        Some(msg)
    }
    
    /// 从队列接收消息（阻塞）
    /// 
    /// 性能优化：减少锁持有时间
    pub async fn receive(&self) -> Option<QueuedMessage> {
        let msg_opt = {
            let mut receiver_guard = self.receiver.write().await;
            if let Some(ref mut receiver) = *receiver_guard {
                receiver.recv().await
            } else {
                return None;
            }
        };
        
        let msg = match msg_opt {
            Some(msg) => msg,
            None => {
                warn!("Message queue receiver closed");
                return None;
            }
        };
        
        // 更新统计（在锁外）
        let mut stats = self.stats.write().await;
        stats.total_processed += 1;
        stats.current_queue_size = stats.current_queue_size.saturating_sub(1);
        Some(msg)
    }
    
    /// 检查消息是否重复（性能优化：快速路径）
    pub async fn is_duplicate(&self, message_id: &str) -> bool {
        let seen = self.seen_messages.read().await;
        seen.contains_key(message_id)
    }
    
    /// 标记消息已见过（性能优化：批量清理）
    async fn mark_seen(&self, message_id: &str) {
        let mut seen = self.seen_messages.write().await;
        seen.insert(message_id.to_string(), Utc::now());
        
        // 性能优化：每 100 次插入才清理一次，减少锁竞争
        if seen.len() % 100 == 0 {
            let window = chrono::Duration::seconds(crate::domain::constants::message::MESSAGE_DEDUP_WINDOW_SECONDS);
            let cutoff = Utc::now() - window;
            seen.retain(|_, &mut timestamp| timestamp > cutoff);
        }
    }
    
    /// 获取队列统计信息
    pub async fn stats(&self) -> QueueStats {
        self.stats.read().await.clone()
    }
    
    /// 获取当前队列长度
    pub async fn len(&self) -> usize {
        self.stats.read().await.current_queue_size
    }
    
    /// 检查队列是否为空
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
}

impl Default for MessageQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// 消息队列处理器
///
/// 负责从队列中取出消息并处理
pub struct MessageQueueProcessor {
    queue: Arc<MessageQueue>,
    handler: Arc<dyn MessageHandler + Send + Sync>,
}

/// 消息处理器 trait
#[async_trait::async_trait]
pub trait MessageHandler: Send + Sync {
    /// 处理消息
    async fn handle_message(&self, message: &Message) -> anyhow::Result<()>;
    
    /// 处理消息失败时的回调
    async fn handle_error(&self, message: &Message, error: &anyhow::Error) {
        error!("Failed to process message {}: {}", message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("<none>"), error);
    }
}

impl MessageQueueProcessor {
    /// 创建新的消息队列处理器
    pub fn new(queue: Arc<MessageQueue>, handler: Arc<dyn MessageHandler + Send + Sync>) -> Self {
        Self { queue, handler }
    }
    
    /// 启动消息处理循环
    pub async fn start(&self) {
        info!("Message queue processor started");
        
        loop {
            // 从队列接收消息
            match self.queue.receive().await {
                Some(queued_message) => {
                    info!(
                        message_id = %queued_message.message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("<none>"),
                        conversation_id = %queued_message.message.conversation_id.as_ref().map(|s| s.as_str()).unwrap_or("<none>"),
                        sender_id = %queued_message.message.sender_id,
                        priority = queued_message.priority,
                        "Processing queued message"
                    );
                    
                    // 处理消息
                    match self.handler.handle_message(&queued_message.message).await {
                        Ok(_) => {
                            info!(
                                message_id = %queued_message.message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("<none>"),
                                "Message processed successfully by handler"
                            );
                        }
                        Err(e) => {
                            // 处理失败
                            self.handler.handle_error(&queued_message.message, &e).await;
                            
                            // 如果重试次数未超过限制，可以重新入队
                            if queued_message.retry_count < crate::domain::constants::message::MAX_MESSAGE_RETRY_COUNT {
                                let mut retry_message = queued_message.clone();
                                retry_message.retry_count += 1;
                                
                                // 重新入队（降低优先级）
                                if !self.queue.enqueue(retry_message.message, queued_message.priority.saturating_sub(1)).await {
                                    warn!("Failed to retry message: {}", queued_message.message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("<none>"));
                                }
                            } else {
                    error!(
                        message_id = %queued_message.message.server_id.as_ref().map(|s| s.as_str()).unwrap_or("<none>"),
                        retry_count = queued_message.retry_count,
                        "Message processing failed after max retries"
                    );
                            }
                        }
                    }
                }
                None => {
                    // 队列关闭，退出循环
                    info!("Message queue closed, processor stopping");
                    break;
                }
            }
        }
    }
}
