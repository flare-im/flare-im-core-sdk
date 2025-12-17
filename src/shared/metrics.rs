//! 性能指标收集
//!
//! 提供 SDK 性能监控和指标收集功能

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

/// 性能指标收集器
///
/// 使用原子操作实现无锁的性能指标收集
pub struct Metrics {
    /// 已发送消息数
    messages_sent: Arc<AtomicU64>,

    /// 已接收消息数
    messages_received: Arc<AtomicU64>,

    /// 消息发送总延迟（毫秒）
    message_send_latency_ms: Arc<AtomicU64>,

    /// 消息接收总延迟（毫秒）
    message_receive_latency_ms: Arc<AtomicU64>,

    /// 当前连接数
    connection_count: Arc<AtomicU32>,

    /// 存储操作总数
    storage_operations: Arc<AtomicU64>,

    /// 存储操作总耗时（毫秒）
    storage_latency_ms: Arc<AtomicU64>,

    /// 同步操作总数
    sync_operations: Arc<AtomicU64>,

    /// 同步操作总耗时（毫秒）
    sync_latency_ms: Arc<AtomicU64>,

    /// 错误总数
    error_count: Arc<AtomicU64>,

    /// 重试总数
    retry_count: Arc<AtomicU64>,
}

impl Metrics {
    /// 创建新的指标收集器
    pub fn new() -> Self {
        Self {
            messages_sent: Arc::new(AtomicU64::new(0)),
            messages_received: Arc::new(AtomicU64::new(0)),
            message_send_latency_ms: Arc::new(AtomicU64::new(0)),
            message_receive_latency_ms: Arc::new(AtomicU64::new(0)),
            connection_count: Arc::new(AtomicU32::new(0)),
            storage_operations: Arc::new(AtomicU64::new(0)),
            storage_latency_ms: Arc::new(AtomicU64::new(0)),
            sync_operations: Arc::new(AtomicU64::new(0)),
            sync_latency_ms: Arc::new(AtomicU64::new(0)),
            error_count: Arc::new(AtomicU64::new(0)),
            retry_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 记录消息发送
    pub fn record_message_sent(&self, latency: Duration) {
        self.messages_sent.fetch_add(1, Ordering::Relaxed);
        self.message_send_latency_ms
            .fetch_add(latency.as_millis() as u64, Ordering::Relaxed);
    }

    /// 记录消息接收
    pub fn record_message_received(&self, latency: Duration) {
        self.messages_received.fetch_add(1, Ordering::Relaxed);
        self.message_receive_latency_ms
            .fetch_add(latency.as_millis() as u64, Ordering::Relaxed);
    }

    /// 更新连接数
    pub fn set_connection_count(&self, count: u32) {
        self.connection_count.store(count, Ordering::Relaxed);
    }

    /// 记录存储操作
    pub fn record_storage_operation(&self, latency: Duration) {
        self.storage_operations.fetch_add(1, Ordering::Relaxed);
        self.storage_latency_ms
            .fetch_add(latency.as_millis() as u64, Ordering::Relaxed);
    }

    /// 记录同步操作
    pub fn record_sync_operation(&self, latency: Duration) {
        self.sync_operations.fetch_add(1, Ordering::Relaxed);
        self.sync_latency_ms
            .fetch_add(latency.as_millis() as u64, Ordering::Relaxed);
    }

    /// 记录错误
    pub fn record_error(&self) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录重试
    pub fn record_retry(&self) {
        self.retry_count.fetch_add(1, Ordering::Relaxed);
    }

    /// 获取指标快照
    pub fn snapshot(&self) -> MetricsSnapshot {
        let messages_sent = self.messages_sent.load(Ordering::Relaxed);
        let messages_received = self.messages_received.load(Ordering::Relaxed);
        let storage_ops = self.storage_operations.load(Ordering::Relaxed);
        let sync_ops = self.sync_operations.load(Ordering::Relaxed);

        MetricsSnapshot {
            messages_sent,
            messages_received,
            avg_send_latency_ms: if messages_sent > 0 {
                self.message_send_latency_ms.load(Ordering::Relaxed) / messages_sent
            } else {
                0
            },
            avg_receive_latency_ms: if messages_received > 0 {
                self.message_receive_latency_ms.load(Ordering::Relaxed) / messages_received
            } else {
                0
            },
            connection_count: self.connection_count.load(Ordering::Relaxed),
            storage_operations: storage_ops,
            avg_storage_latency_ms: if storage_ops > 0 {
                self.storage_latency_ms.load(Ordering::Relaxed) / storage_ops
            } else {
                0
            },
            sync_operations: sync_ops,
            avg_sync_latency_ms: if sync_ops > 0 {
                self.sync_latency_ms.load(Ordering::Relaxed) / sync_ops
            } else {
                0
            },
            error_count: self.error_count.load(Ordering::Relaxed),
            retry_count: self.retry_count.load(Ordering::Relaxed),
        }
    }

    /// 重置所有指标
    pub fn reset(&self) {
        self.messages_sent.store(0, Ordering::Relaxed);
        self.messages_received.store(0, Ordering::Relaxed);
        self.message_send_latency_ms.store(0, Ordering::Relaxed);
        self.message_receive_latency_ms.store(0, Ordering::Relaxed);
        self.storage_operations.store(0, Ordering::Relaxed);
        self.storage_latency_ms.store(0, Ordering::Relaxed);
        self.sync_operations.store(0, Ordering::Relaxed);
        self.sync_latency_ms.store(0, Ordering::Relaxed);
        self.error_count.store(0, Ordering::Relaxed);
        self.retry_count.store(0, Ordering::Relaxed);
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// 指标快照
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    /// 已发送消息数
    pub messages_sent: u64,

    /// 已接收消息数
    pub messages_received: u64,

    /// 平均发送延迟（毫秒）
    pub avg_send_latency_ms: u64,

    /// 平均接收延迟（毫秒）
    pub avg_receive_latency_ms: u64,

    /// 当前连接数
    pub connection_count: u32,

    /// 存储操作总数
    pub storage_operations: u64,

    /// 平均存储操作延迟（毫秒）
    pub avg_storage_latency_ms: u64,

    /// 同步操作总数
    pub sync_operations: u64,

    /// 平均同步操作延迟（毫秒）
    pub avg_sync_latency_ms: u64,

    /// 错误总数
    pub error_count: u64,

    /// 重试总数
    pub retry_count: u64,
}

impl MetricsSnapshot {
    /// 计算错误率
    pub fn error_rate(&self) -> f64 {
        let total_operations = self.messages_sent
            + self.messages_received
            + self.storage_operations
            + self.sync_operations;
        if total_operations > 0 {
            self.error_count as f64 / total_operations as f64
        } else {
            0.0
        }
    }

    /// 计算重试率
    pub fn retry_rate(&self) -> f64 {
        let total_operations = self.messages_sent
            + self.messages_received
            + self.storage_operations
            + self.sync_operations;
        if total_operations > 0 {
            self.retry_count as f64 / total_operations as f64
        } else {
            0.0
        }
    }
}
