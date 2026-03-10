use std::sync::atomic::{AtomicU64, Ordering};

/// SDK 全局 Metrics 计数器
pub static PACKETS_SENT: AtomicU64 = AtomicU64::new(0);
pub static PACKETS_RECEIVED: AtomicU64 = AtomicU64::new(0);
pub static MESSAGES_SENT: AtomicU64 = AtomicU64::new(0);
pub static MESSAGES_RECEIVED: AtomicU64 = AtomicU64::new(0);
pub static SYNC_COUNT: AtomicU64 = AtomicU64::new(0);

/// 日志 + Metrics 中间件
pub struct LoggingMiddleware;

impl LoggingMiddleware {
    pub fn snapshot() -> MetricsSnapshot {
        MetricsSnapshot {
            packets_sent: PACKETS_SENT.load(Ordering::Relaxed),
            packets_received: PACKETS_RECEIVED.load(Ordering::Relaxed),
            messages_sent: MESSAGES_SENT.load(Ordering::Relaxed),
            messages_received: MESSAGES_RECEIVED.load(Ordering::Relaxed),
            sync_count: SYNC_COUNT.load(Ordering::Relaxed),
        }
    }

    pub fn reset() {
        PACKETS_SENT.store(0, Ordering::Relaxed);
        PACKETS_RECEIVED.store(0, Ordering::Relaxed);
        MESSAGES_SENT.store(0, Ordering::Relaxed);
        MESSAGES_RECEIVED.store(0, Ordering::Relaxed);
        SYNC_COUNT.store(0, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub packets_sent: u64,
    pub packets_received: u64,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub sync_count: u64,
}
