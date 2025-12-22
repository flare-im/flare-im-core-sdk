//! 指标监控模块
//!
//! 提供消息发送、接收、ACK 等关键操作的指标监控
//! 对标微信、Telegram、飞书的监控体系

use std::sync::Arc;
use std::time::Instant;
use once_cell::sync::Lazy;

/// 消息发送指标
pub struct MessageMetrics {
    /// 发送总数
    pub sent_total: u64,
    /// 发送成功数
    pub sent_success: u64,
    /// 发送失败数
    pub sent_failed: u64,
    /// ACK 超时数
    pub ack_timeout: u64,
    /// 平均发送延迟（毫秒）
    pub avg_send_latency_ms: u64,
}

impl MessageMetrics {
    pub fn new() -> Self {
        Self {
            sent_total: 0,
            sent_success: 0,
            sent_failed: 0,
            ack_timeout: 0,
            avg_send_latency_ms: 0,
        }
    }
    
    /// 记录消息发送
    pub fn record_send(&mut self, success: bool, latency_ms: u64) {
        self.sent_total += 1;
        if success {
            self.sent_success += 1;
        } else {
            self.sent_failed += 1;
        }
        
        // 更新平均延迟（简单移动平均）
        if self.sent_total > 0 {
            self.avg_send_latency_ms = 
                (self.avg_send_latency_ms * (self.sent_total - 1) + latency_ms) / self.sent_total;
        }
    }
    
    /// 记录 ACK 超时
    pub fn record_ack_timeout(&mut self) {
        self.ack_timeout += 1;
    }
    
    /// 获取指标快照
    pub fn snapshot(&self) -> MessageMetricsSnapshot {
        MessageMetricsSnapshot {
            sent_total: self.sent_total,
            sent_success: self.sent_success,
            sent_failed: self.sent_failed,
            ack_timeout: self.ack_timeout,
            avg_send_latency_ms: self.avg_send_latency_ms,
            success_rate: if self.sent_total > 0 {
                (self.sent_success as f64 / self.sent_total as f64) * 100.0
            } else {
                0.0
            },
        }
    }
}

/// 指标快照
#[derive(Debug, Clone)]
pub struct MessageMetricsSnapshot {
    pub sent_total: u64,
    pub sent_success: u64,
    pub sent_failed: u64,
    pub ack_timeout: u64,
    pub avg_send_latency_ms: u64,
    pub success_rate: f64,
}

/// 全局指标实例（线程安全）
static GLOBAL_METRICS: Lazy<Arc<tokio::sync::Mutex<MessageMetrics>>> = 
    Lazy::new(|| Arc::new(tokio::sync::Mutex::new(MessageMetrics::new())));

/// 记录消息发送
pub async fn record_message_send(success: bool, latency_ms: u64) {
    let mut metrics = GLOBAL_METRICS.lock().await;
    metrics.record_send(success, latency_ms);
}

/// 记录 ACK 超时
pub async fn record_ack_timeout() {
    let mut metrics = GLOBAL_METRICS.lock().await;
    metrics.record_ack_timeout();
}

/// 获取指标快照
pub async fn get_metrics_snapshot() -> MessageMetricsSnapshot {
    let metrics = GLOBAL_METRICS.lock().await;
    metrics.snapshot()
}
