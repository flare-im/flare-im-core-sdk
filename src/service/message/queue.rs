//! 消息队列和批处理
//!
//! 实现消息优先级队列、批处理和去重机制

use crate::error::{SDKError, SDKResult};
use super::service::SendOptions;
use flare_proto::MessageContent as ProtoMessageContent;
use std::collections::BinaryHeap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc, Mutex};
use std::cmp::Ordering;
use std::time::{Duration, Instant};

/// 消息优先级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessagePriority {
    /// 最低优先级（后台消息）
    Lowest = 0,
    /// 低优先级（普通消息）
    Low = 1,
    /// 普通优先级（默认）
    Normal = 2,
    /// 高优先级（重要消息）
    High = 3,
    /// 最高优先级（紧急消息）
    Highest = 4,
}

impl Default for MessagePriority {
    fn default() -> Self {
        Self::Normal
    }
}

impl From<i32> for MessagePriority {
    fn from(priority: i32) -> Self {
        match priority {
            p if p <= 0 => MessagePriority::Lowest,
            p if p <= 2 => MessagePriority::Low,
            p if p <= 5 => MessagePriority::Normal,
            p if p <= 8 => MessagePriority::High,
            _ => MessagePriority::Highest,
        }
    }
}

/// 队列中的消息项
#[derive(Clone)]
pub struct QueuedMessageItem {
    /// 消息ID（用于去重）
    pub message_id: String,
    
    /// 会话ID
    pub session_id: String,
    
    /// 消息内容
    pub content: ProtoMessageContent,
    
    /// 发送选项
    pub options: SendOptions,
    
    /// 优先级
    pub priority: MessagePriority,
    
    /// 入队时间
    pub enqueue_time: Instant,
    
    /// 重试次数
    pub retry_count: u32,
}

impl PartialEq for QueuedMessageItem {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.enqueue_time == other.enqueue_time
    }
}

impl Eq for QueuedMessageItem {}

impl PartialOrd for QueuedMessageItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QueuedMessageItem {
    fn cmp(&self, other: &Self) -> Ordering {
        // 优先级高的先出队
        match self.priority.cmp(&other.priority) {
            Ordering::Equal => {
                // 优先级相同时，早入队的先出队
                other.enqueue_time.cmp(&self.enqueue_time)
            }
            other => other,
        }
    }
}

/// 消息队列配置
#[derive(Debug, Clone)]
pub struct MessageQueueConfig {
    /// 批处理大小（默认 10）
    pub batch_size: usize,
    
    /// 批处理超时时间（默认 100ms）
    pub batch_timeout: Duration,
    
    /// 最大队列长度（默认 1000）
    pub max_queue_size: usize,
    
    /// 最大重试次数（默认 3）
    pub max_retries: u32,
    
    /// 重试延迟（默认 1秒）
    pub retry_delay: Duration,
    
    /// 是否启用去重（默认 true）
    pub enable_deduplication: bool,
}

impl Default for MessageQueueConfig {
    fn default() -> Self {
        // 根据平台自动调整配置
        use crate::platform::{get_platform, Platform};
        let platform = get_platform();
        
        match platform {
            Platform::Web => {
                // Web 端：较小的批次和队列，更快的超时（考虑浏览器性能）
                Self {
                    batch_size: 5,
                    batch_timeout: Duration::from_millis(50),
                    max_queue_size: 500,
                    max_retries: 2,
                    retry_delay: Duration::from_millis(500),
                    enable_deduplication: true,
                }
            }
            Platform::Desktop => {
                // 桌面端：较大的批次和队列，标准超时
                Self {
                    batch_size: 20,
                    batch_timeout: Duration::from_millis(100),
                    max_queue_size: 2000,
                    max_retries: 3,
                    retry_delay: Duration::from_secs(1),
                    enable_deduplication: true,
                }
            }
            Platform::Android | Platform::IOS | Platform::HarmonyOS => {
                // 移动端：中等批次，考虑内存限制
                Self {
                    batch_size: 10,
                    batch_timeout: Duration::from_millis(100),
                    max_queue_size: 1000,
                    max_retries: 3,
                    retry_delay: Duration::from_millis(800),
                    enable_deduplication: true,
                }
            }
        }
    }
}

/// 消息队列
/// 
/// 支持优先级队列、批处理和去重
pub struct MessageQueue {
    /// 优先级队列
    queue: Arc<Mutex<BinaryHeap<QueuedMessageItem>>>,
    
    /// 去重集合（message_id -> enqueue_time）
    dedup_set: Arc<RwLock<std::collections::HashMap<String, Instant>>>,
    
    /// 配置
    config: MessageQueueConfig,
    
    /// 发送通道（用于通知发送器）
    sender_tx: mpsc::Sender<()>,
}

impl MessageQueue {
    /// 创建新的消息队列
    pub fn new(config: MessageQueueConfig) -> (Self, mpsc::Receiver<()>) {
        let (tx, rx) = mpsc::channel(1);
        let queue = Self {
            queue: Arc::new(Mutex::new(BinaryHeap::new())),
            dedup_set: Arc::new(RwLock::new(std::collections::HashMap::new())),
            config,
            sender_tx: tx,
        };
        (queue, rx)
    }
    
    /// 入队消息
    /// 
    /// 支持背压机制：当队列满时，如果消息优先级较低，会尝试丢弃最低优先级的消息
    pub async fn enqueue(
        &self,
        message_id: String,
        session_id: String,
        content: ProtoMessageContent,
        options: SendOptions,
    ) -> SDKResult<()> {
        // 去重检查（在锁外进行，减少锁持有时间）
        if self.config.enable_deduplication {
            let dedup = self.dedup_set.read().await;
            if dedup.contains_key(&message_id) {
                return Err(SDKError::message_error(
                    flare_core::common::error::code::ErrorCode::MessageRateLimitExceeded,
                    format!("消息已存在: {}", message_id),
                ));
            }
            drop(dedup);
        }
        
        // 确定优先级
        let priority = options.priority
            .map(MessagePriority::from)
            .unwrap_or(MessagePriority::Normal);
        
        // 创建队列项
        let item = QueuedMessageItem {
            message_id: message_id.clone(),
            session_id,
            content,
            options,
            priority,
            enqueue_time: Instant::now(),
            retry_count: 0,
        };
        
        // 检查队列大小并处理背压
        let mut queue = self.queue.lock().await;
        if queue.len() >= self.config.max_queue_size {
            // 队列已满，尝试背压处理
            // 如果当前消息优先级高于最低优先级，尝试丢弃最低优先级的消息
            if priority > MessagePriority::Lowest {
                // BinaryHeap 是最大堆，需要找到最低优先级的消息
                // 策略：将所有消息取出，找到最低优先级的，丢弃它
                let mut temp_items = Vec::new();
                let mut lowest_item: Option<QueuedMessageItem> = None;
                
                // 取出所有消息，找到最低优先级的
                while let Some(item) = queue.pop() {
                    if let Some(ref lowest) = lowest_item {
                        if item.priority < lowest.priority {
                            // 如果之前有最低优先级的，先保存它
                            temp_items.push(lowest.clone());
                            lowest_item = Some(item);
                        } else {
                            temp_items.push(item);
                        }
                    } else {
                        lowest_item = Some(item);
                    }
                }
                
                // 如果找到了最低优先级的消息，丢弃它
                if let Some(lowest) = lowest_item {
                    let lowest_priority = lowest.priority;
                    let lowest_message_id = lowest.message_id.clone();
                    
                    if lowest_priority < priority {
                        // 当前消息优先级更高，丢弃最低优先级的
                        if self.config.enable_deduplication {
                            drop(queue);
                            let mut dedup = self.dedup_set.write().await;
                            dedup.remove(&lowest_message_id);
                            drop(dedup);
                            queue = self.queue.lock().await;
                        }
                        
                        // 将剩余消息放回队列
                        for item in temp_items {
                            queue.push(item);
                        }
                        
                        // 入队新消息
                        queue.push(item);
                        drop(queue);
                        
                        // 更新去重集合
                        if self.config.enable_deduplication {
                            let mut dedup = self.dedup_set.write().await;
                            dedup.insert(message_id, Instant::now());
                        }
                        
                        // 通知发送器
                        let _ = self.sender_tx.send(()).await;
                        return Ok(());
                    } else {
                        // 当前消息优先级不够高，无法替换
                        // 将包括最低优先级在内的所有消息放回
                        temp_items.push(lowest);
                        for item in temp_items {
                            queue.push(item);
                        }
                    }
                }
            }
            
            // 无法处理背压，返回错误
            drop(queue);
            return Err(SDKError::message_error(
                flare_core::common::error::code::ErrorCode::MessageRateLimitExceeded,
                format!("消息队列已满: {}/{}", self.config.max_queue_size, self.config.max_queue_size),
            ));
        }
        
        // 队列未满，直接入队
        queue.push(item);
        drop(queue);
        
        // 更新去重集合
        if self.config.enable_deduplication {
            let mut dedup = self.dedup_set.write().await;
            dedup.insert(message_id, Instant::now());
        }
        
        // 通知发送器
        let _ = self.sender_tx.send(()).await;
        
        Ok(())
    }
    
    /// 批量出队
    /// 
    /// 返回一批消息，最多 batch_size 条
    pub async fn dequeue_batch(&self) -> Vec<QueuedMessageItem> {
        let mut queue = self.queue.lock().await;
        let mut batch = Vec::new();
        let batch_size = self.config.batch_size;
        
        while batch.len() < batch_size && !queue.is_empty() {
            if let Some(item) = queue.pop() {
                batch.push(item);
            }
        }
        
        batch
    }
    
    /// 重新入队（用于重试）
    pub async fn re_enqueue(&self, mut item: QueuedMessageItem) -> SDKResult<()> {
        if item.retry_count >= self.config.max_retries {
            // 超过最大重试次数，从去重集合中移除
            if self.config.enable_deduplication {
                let mut dedup = self.dedup_set.write().await;
                dedup.remove(&item.message_id);
            }
            return Err(SDKError::message_error(
                flare_core::common::error::code::ErrorCode::MessageSendFailed,
                format!("消息发送失败，已重试 {} 次", self.config.max_retries),
            ));
        }
        
        item.retry_count += 1;
        item.enqueue_time = Instant::now();
        
        self.queue.lock().await.push(item);
        
        // 延迟后通知发送器
        let sender_tx = self.sender_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let _ = sender_tx.send(()).await;
        });
        
        Ok(())
    }
    
    /// 清理过期的去重记录
    pub async fn cleanup_dedup(&self, max_age: Duration) {
        let mut dedup = self.dedup_set.write().await;
        let now = Instant::now();
        dedup.retain(|_, time| now.duration_since(*time) < max_age);
    }
    
    /// 获取队列长度
    pub async fn len(&self) -> usize {
        self.queue.lock().await.len()
    }
    
    /// 检查队列是否为空
    pub async fn is_empty(&self) -> bool {
        self.queue.lock().await.is_empty()
    }
}

/// 消息批处理器
/// 
/// 负责批量发送消息，提高效率
pub struct MessageBatchProcessor {
    /// 消息队列
    queue: Arc<MessageQueue>,
    
    /// 批处理配置
    config: MessageQueueConfig,
    
    /// 当前批次
    current_batch: Arc<Mutex<Vec<QueuedMessageItem>>>,
    
    /// 最后批处理时间
    last_batch_time: Arc<Mutex<Instant>>,
}

impl MessageBatchProcessor {
    /// 创建新的批处理器
    pub fn new(queue: Arc<MessageQueue>, config: MessageQueueConfig) -> Self {
        Self {
            queue,
            config: config.clone(),
            current_batch: Arc::new(Mutex::new(Vec::with_capacity(config.batch_size))),
            last_batch_time: Arc::new(Mutex::new(Instant::now())),
        }
    }
    
    /// 处理批处理逻辑
    /// 
    /// 当达到批处理大小或超时时间时，触发批量发送
    /// 
    /// 优化：缩小锁粒度，在发送消息时释放锁
    pub async fn process_batch<F, Fut>(&self, sender: F) -> SDKResult<()>
    where
        F: Fn(Vec<QueuedMessageItem>) -> Fut,
        Fut: std::future::Future<Output = SDKResult<Vec<(String, bool)>>>,
    {
        // 快速检查是否需要处理（只读锁，快速释放）
        let should_check = {
            let batch = self.current_batch.lock().await;
            let last_time = self.last_batch_time.lock().await;
            batch.len() >= self.config.batch_size
                || last_time.elapsed() >= self.config.batch_timeout
        };
        
        if !should_check {
            return Ok(());
        }
        
        // 获取批次数据（缩小锁范围）
        let batch_to_send = {
            let batch = self.current_batch.lock().await;
            let last_time_guard = self.last_batch_time.lock().await;
            
            // 检查是否需要触发批处理
            let should_flush = batch.len() >= self.config.batch_size
                || last_time_guard.elapsed() >= self.config.batch_timeout;
            
            if !should_flush || batch.is_empty() {
                // 尝试从队列获取更多消息
                drop(batch);
                drop(last_time_guard);
                
                let mut items = self.queue.dequeue_batch().await;
                if items.is_empty() {
                    return Ok(());
                }
                
                // 重新获取锁，添加新消息
                let mut batch = self.current_batch.lock().await;
                batch.append(&mut items);
                
                // 再次检查是否需要发送
                let last_time_guard2 = self.last_batch_time.lock().await;
                let should_flush = batch.len() >= self.config.batch_size
                    || last_time_guard2.elapsed() >= self.config.batch_timeout;
                
                if !should_flush {
                    return Ok(());
                }
                
                batch.clone()
            } else {
                // 从队列中获取更多消息
                drop(batch);
                drop(last_time_guard);
                
                let mut items = self.queue.dequeue_batch().await;
                
                // 重新获取锁，合并批次
                let mut batch = self.current_batch.lock().await;
                batch.append(&mut items);
                
                batch.clone()
            }
        };
        
        // 执行批量发送（锁已释放）
        match sender(batch_to_send).await {
            Ok(results) => {
                    // 处理发送结果（重新获取锁）
                    let mut batch = self.current_batch.lock().await;
                    
                    // 统计成功和失败的数量
                    let success_count = results.iter().filter(|(_, success)| *success).count();
                    let fail_count = results.len() - success_count;
                    
                    if fail_count > 0 {
                        tracing::warn!(
                            success = success_count,
                            failed = fail_count,
                            "Some messages in batch failed to send"
                        );
                    }
                    
                    batch.clear();
                    *self.last_batch_time.lock().await = Instant::now();
                    Ok(())
                }
            Err(e) => {
                // 批量发送失败，重新入队（需要重新获取批次数据）
                // 注意：这里无法恢复原始批次，因为已经发送了
                // 实际场景中，发送失败的消息应该已经在发送过程中处理了
                tracing::error!(error = %e, "Batch send failed");
                Err(e)
            }
        }
    }
}

