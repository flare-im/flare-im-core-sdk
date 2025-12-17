//! 重连策略
//!
//! 参考 Telegram 的设计，实现智能重连策略

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::Instant;

/// 网络质量评估
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkQuality {
    /// 优秀：延迟低，丢包率低
    Excellent,
    /// 良好：延迟中等，丢包率低
    Good,
    /// 一般：延迟较高或偶发丢包
    Fair,
    /// 差：延迟高或频繁丢包
    Poor,
}

impl NetworkQuality {
    /// 根据延迟和丢包率评估网络质量
    pub fn from_metrics(latency_ms: u64, packet_loss_rate: f64) -> Self {
        match (latency_ms, packet_loss_rate) {
            (latency, loss) if latency < 100 && loss < 0.01 => NetworkQuality::Excellent,
            (latency, loss) if latency < 200 && loss < 0.05 => NetworkQuality::Good,
            (latency, loss) if latency < 500 && loss < 0.10 => NetworkQuality::Fair,
            _ => NetworkQuality::Poor,
        }
    }
}

/// 重连策略（参考 Telegram 设计）
///
/// Telegram 使用指数退避策略，根据网络质量动态调整
pub struct ReconnectStrategy {
    /// 初始重连延迟
    initial_delay: Duration,

    /// 最大重连延迟
    max_delay: Duration,

    /// 退避倍数（默认 2.0，指数退避）
    backoff_multiplier: f64,

    /// 当前重连延迟
    current_delay: Arc<RwLock<Duration>>,

    /// 重连次数
    attempt_count: Arc<RwLock<u32>>,

    /// 网络质量
    network_quality: Arc<RwLock<NetworkQuality>>,

    /// 最后一次重连时间
    last_reconnect_time: Arc<RwLock<Option<Instant>>>,
}

impl ReconnectStrategy {
    /// 创建默认重连策略
    pub fn default() -> Self {
        Self::new(
            Duration::from_secs(1),  // 初始延迟 1 秒
            Duration::from_secs(60), // 最大延迟 60 秒
            2.0,                     // 指数退避倍数
        )
    }

    /// 创建新的重连策略
    pub fn new(initial_delay: Duration, max_delay: Duration, backoff_multiplier: f64) -> Self {
        Self {
            initial_delay,
            max_delay,
            backoff_multiplier,
            current_delay: Arc::new(RwLock::new(initial_delay)),
            attempt_count: Arc::new(RwLock::new(0)),
            network_quality: Arc::new(RwLock::new(NetworkQuality::Good)),
            last_reconnect_time: Arc::new(RwLock::new(None)),
        }
    }

    /// 获取下一次重连延迟（指数退避）
    ///
    /// 参考 Telegram：根据网络质量和重连次数动态调整延迟
    pub async fn next_delay(&self) -> Duration {
        let attempt = *self.attempt_count.read().await;
        let quality = *self.network_quality.read().await;

        // 根据网络质量调整基础延迟
        let base_delay = match quality {
            NetworkQuality::Excellent => self.initial_delay,
            NetworkQuality::Good => self.initial_delay * 2,
            NetworkQuality::Fair => self.initial_delay * 4,
            NetworkQuality::Poor => self.initial_delay * 8,
        };

        // 指数退避：delay = base_delay * (backoff_multiplier ^ attempt)
        let delay_ms = base_delay.as_millis() as f64 * self.backoff_multiplier.powi(attempt as i32);

        // 限制最大延迟
        let delay = Duration::from_millis(delay_ms as u64).min(self.max_delay);

        // 更新当前延迟
        *self.current_delay.write().await = delay;

        delay
    }

    /// 记录重连尝试
    pub async fn record_attempt(&self) {
        let mut count = self.attempt_count.write().await;
        *count += 1;

        let mut last_time = self.last_reconnect_time.write().await;
        *last_time = Some(Instant::now());
    }

    /// 重置重连策略（连接成功后调用）
    pub async fn reset(&self) {
        *self.attempt_count.write().await = 0;
        *self.current_delay.write().await = self.initial_delay;
        *self.last_reconnect_time.write().await = None;
    }

    /// 更新网络质量
    pub async fn update_network_quality(&self, quality: NetworkQuality) {
        *self.network_quality.write().await = quality;

        // 如果网络质量改善，可以适当减少延迟
        if matches!(quality, NetworkQuality::Excellent | NetworkQuality::Good) {
            let current = *self.current_delay.read().await;
            if current > self.initial_delay {
                let reduced = current / 2;
                *self.current_delay.write().await = reduced.max(self.initial_delay);
            }
        }
    }

    /// 获取当前重连次数
    pub async fn attempt_count(&self) -> u32 {
        *self.attempt_count.read().await
    }

    /// 获取当前延迟
    pub async fn current_delay(&self) -> Duration {
        *self.current_delay.read().await
    }

    /// 获取网络质量
    pub async fn network_quality(&self) -> NetworkQuality {
        *self.network_quality.read().await
    }

    /// 检查是否应该继续重连（基于最大重试次数）
    pub async fn should_continue(&self, max_attempts: u32) -> bool {
        if max_attempts == 0 {
            return true; // 0 表示无限制
        }
        *self.attempt_count.read().await < max_attempts
    }
}

impl Default for ReconnectStrategy {
    fn default() -> Self {
        Self::default()
    }
}
