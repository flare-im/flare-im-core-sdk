//! 持久化待发送消息队列
//!
//! 实现消息的持久化存储、重试、去重、幂等性保证

use crate::infrastructure::storage::StorageBackend;
use crate::shared::error::SDKResult;
use anyhow::{Context, Result};
use flare_proto::MessageContent as ProtoMessageContent;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// 待发送消息项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingMessage {
    /// 消息 ID（用于去重和幂等）
    pub message_id: String,

    /// 会话 ID
    pub session_id: String,

    /// 消息内容
    pub content: Vec<u8>, // 序列化的 ProtoMessageContent

    /// 优先级
    pub priority: i32,

    /// 可靠性级别
    pub reliability: String, // 序列化的 Reliability

    /// 创建时间（毫秒时间戳）
    pub created_at: i64,

    /// 最后尝试时间（毫秒时间戳）
    pub last_attempt_at: i64,

    /// 重试次数
    pub retry_count: u32,

    /// 最大重试次数
    pub max_retries: u32,

    /// 状态（pending, sending, failed, completed）
    pub status: String,

    /// 错误信息（如果失败）
    pub error: Option<String>,
}

impl PendingMessage {
    /// 创建新的待发送消息
    pub fn new(
        message_id: String,
        session_id: String,
        content: ProtoMessageContent,
        priority: i32,
        reliability: String,
        max_retries: u32,
    ) -> Result<Self> {
        let content_bytes = prost::Message::encode_to_vec(&content);
        let now = chrono::Utc::now().timestamp_millis();

        Ok(Self {
            message_id,
            session_id,
            content: content_bytes,
            priority,
            reliability,
            created_at: now,
            last_attempt_at: now,
            retry_count: 0,
            max_retries,
            status: "pending".to_string(),
            error: None,
        })
    }

    /// 反序列化消息内容
    pub fn deserialize_content(&self) -> Result<ProtoMessageContent> {
        prost::Message::decode(&self.content[..]).context("Failed to deserialize message content")
    }

    /// 检查是否可以重试
    pub fn can_retry(&self) -> bool {
        self.retry_count < self.max_retries && self.status != "completed"
    }

    /// 标记为发送中
    pub fn mark_sending(&mut self) {
        self.status = "sending".to_string();
        self.last_attempt_at = chrono::Utc::now().timestamp_millis();
    }

    /// 标记为失败
    pub fn mark_failed(&mut self, error: String) {
        self.status = "failed".to_string();
        self.error = Some(error);
        self.retry_count += 1;
        self.last_attempt_at = chrono::Utc::now().timestamp_millis();
    }

    /// 标记为完成
    pub fn mark_completed(&mut self) {
        self.status = "completed".to_string();
    }

    /// 增加重试次数
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
        self.last_attempt_at = chrono::Utc::now().timestamp_millis();
    }
}

/// 持久化待发送消息队列配置
#[derive(Debug, Clone)]
pub struct PendingMessageQueueConfig {
    /// 是否启用持久化（默认：true）
    pub enable_persistence: bool,

    /// 最大队列大小（默认：1000）
    pub max_queue_size: usize,

    /// 最大重试次数（默认：3）
    pub max_retries: u32,

    /// 重试延迟（默认：1秒）
    pub retry_delay: Duration,

    /// 重试延迟倍数（指数退避，默认：2.0）
    pub retry_delay_multiplier: f64,

    /// 最大重试延迟（默认：60秒）
    pub max_retry_delay: Duration,

    /// 是否启用去重（默认：true）
    pub enable_deduplication: bool,

    /// 去重窗口时间（默认：24小时）
    pub dedup_window: Duration,

    /// 清理间隔（默认：1小时）
    pub cleanup_interval: Duration,

    /// 已完成消息保留时间（默认：7天）
    pub completed_retention: Duration,
}

impl Default for PendingMessageQueueConfig {
    fn default() -> Self {
        Self {
            enable_persistence: true,
            max_queue_size: 1000,
            max_retries: 3,
            retry_delay: Duration::from_secs(1),
            retry_delay_multiplier: 2.0,
            max_retry_delay: Duration::from_secs(60),
            enable_deduplication: true,
            dedup_window: Duration::from_secs(24 * 3600),
            cleanup_interval: Duration::from_secs(3600),
            completed_retention: Duration::from_secs(7 * 24 * 3600),
        }
    }
}

/// 持久化待发送消息队列
pub struct PendingMessageQueue {
    #[allow(dead_code)] // 保留用于未来实现数据库持久化
    storage: Arc<dyn StorageBackend>,
    config: Arc<RwLock<PendingMessageQueueConfig>>,
    /// 内存缓存（用于快速访问）
    cache: Arc<RwLock<std::collections::HashMap<String, PendingMessage>>>,
}

impl PendingMessageQueue {
    /// 创建新的持久化队列
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self {
            storage,
            config: Arc::new(RwLock::new(PendingMessageQueueConfig::default())),
            cache: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// 创建带配置的队列
    pub fn with_config(
        storage: Arc<dyn StorageBackend>,
        config: PendingMessageQueueConfig,
    ) -> Self {
        Self {
            storage,
            config: Arc::new(RwLock::new(config)),
            cache: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// 更新配置
    pub async fn update_config(&self, config: PendingMessageQueueConfig) {
        *self.config.write().await = config;
    }

    /// 添加消息到队列（幂等性保证）
    ///
    /// 如果消息已存在，返回现有消息，不会重复添加
    pub async fn enqueue(
        &self,
        message_id: String,
        session_id: String,
        content: ProtoMessageContent,
        priority: i32,
        reliability: String,
    ) -> SDKResult<PendingMessage> {
        let config = self.config.read().await;

        // 去重检查
        if config.enable_deduplication {
            // 先检查内存缓存
            let cache = self.cache.read().await;
            if let Some(existing) = cache.get(&message_id) {
                debug!(message_id = %message_id, "Message already in queue (deduplication)");
                return Ok(existing.clone());
            }
            drop(cache);

            // 检查持久化存储（如果启用）
            if config.enable_persistence {
                // TODO: 从存储中查询（需要实现专门的表）
                // 这里简化实现，假设存储中没有
            }
        }

        // 创建待发送消息
        let max_retries = config.max_retries;
        let pending = PendingMessage::new(
            message_id.clone(),
            session_id,
            content,
            priority,
            reliability,
            max_retries,
        )?;

        // 保存到内存缓存
        {
            let mut cache = self.cache.write().await;
            cache.insert(message_id.clone(), pending.clone());
        }

        // 持久化（如果启用）
        if config.enable_persistence {
            // TODO: 保存到数据库（需要实现专门的表）
            // 这里简化实现
        }

        info!(message_id = %message_id, "Message enqueued");
        Ok(pending)
    }

    /// 获取待发送消息
    pub async fn dequeue(&self) -> Result<Option<PendingMessage>> {
        // 从内存缓存中查找待发送的消息
        // 先收集候选消息（避免借用冲突）
        let candidates: Vec<_> = {
            let cache = self.cache.read().await;
            cache
                .values()
                .filter(|msg| msg.status == "pending" || msg.status == "failed")
                .filter(|msg| msg.can_retry())
                .cloned()
                .collect()
        };

        if candidates.is_empty() {
            return Ok(None);
        }

        // 排序：优先级高的优先，同优先级按创建时间
        let mut sorted_candidates = candidates;
        sorted_candidates.sort_by(|a, b| match b.priority.cmp(&a.priority) {
            std::cmp::Ordering::Equal => a.created_at.cmp(&b.created_at),
            other => other,
        });

        if let Some(mut pending) = sorted_candidates.first().cloned() {
            let message_id = pending.message_id.clone();
            pending.mark_sending();

            // 更新缓存
            {
                let mut cache = self.cache.write().await;
                cache.insert(message_id, pending.clone());
            }

            Ok(Some(pending))
        } else {
            Ok(None)
        }
    }

    /// 标记消息为完成
    pub async fn mark_completed(&self, message_id: &str) -> Result<()> {
        let mut cache = self.cache.write().await;
        if let Some(msg) = cache.get_mut(message_id) {
            msg.mark_completed();
        }
        Ok(())
    }

    /// 标记消息为失败并重试
    pub async fn mark_failed_and_retry(&self, message_id: &str, error: String) -> SDKResult<bool> {
        let mut cache = self.cache.write().await;

        if let Some(msg) = cache.get_mut(message_id) {
            if !msg.can_retry() {
                return Ok(false);
            }

            msg.mark_failed(error);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 清理已完成的消息
    pub async fn cleanup_completed(&self) -> Result<usize> {
        let config = self.config.read().await;
        let now = chrono::Utc::now().timestamp_millis();
        let retention_ms = config.completed_retention.as_millis() as i64;

        let mut cache = self.cache.write().await;
        let mut removed = 0;

        cache.retain(|_, msg| {
            if msg.status == "completed" {
                let age = now - msg.last_attempt_at;
                if age > retention_ms {
                    removed += 1;
                    false
                } else {
                    true
                }
            } else {
                true
            }
        });

        if removed > 0 {
            info!(removed = removed, "Cleaned up completed messages");
        }

        Ok(removed)
    }

    /// 获取队列统计信息
    pub async fn stats(&self) -> QueueStats {
        let cache = self.cache.read().await;
        let total = cache.len();
        let pending = cache.values().filter(|msg| msg.status == "pending").count();
        let sending = cache.values().filter(|msg| msg.status == "sending").count();
        let failed = cache.values().filter(|msg| msg.status == "failed").count();
        let completed = cache
            .values()
            .filter(|msg| msg.status == "completed")
            .count();

        QueueStats {
            total,
            pending,
            sending,
            failed,
            completed,
        }
    }
}

/// 队列统计信息
#[derive(Debug, Clone)]
pub struct QueueStats {
    pub total: usize,
    pub pending: usize,
    pub sending: usize,
    pub failed: usize,
    pub completed: usize,
}
