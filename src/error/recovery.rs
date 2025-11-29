//! 错误恢复策略
//!
//! 实现自动重试、降级、熔断等错误恢复机制

use crate::error::SDKError;
use std::time::{Duration, Instant};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 重试策略
#[derive(Debug, Clone)]
pub enum RetryStrategy {
    /// 不重试
    NoRetry,
    
    /// 固定间隔重试
    Fixed {
        /// 重试次数
        max_attempts: u32,
        /// 重试间隔
        interval: Duration,
    },
    
    /// 指数退避重试
    ExponentialBackoff {
        /// 最大重试次数
        max_attempts: u32,
        /// 初始延迟
        initial_delay: Duration,
        /// 最大延迟
        max_delay: Duration,
        /// 退避倍数
        multiplier: f64,
    },
    
    /// 指数退避 + 抖动（Jitter）
    ExponentialBackoffWithJitter {
        /// 最大重试次数
        max_attempts: u32,
        /// 初始延迟
        initial_delay: Duration,
        /// 最大延迟
        max_delay: Duration,
        /// 退避倍数
        multiplier: f64,
        /// 抖动比例（0.0-1.0）
        jitter_ratio: f64,
    },
}

impl Default for RetryStrategy {
    fn default() -> Self {
        // 根据平台自动调整重试策略
        use crate::platform::{get_platform, Platform};
        let platform = get_platform();
        
        match platform {
            Platform::Web => {
                // Web 端：较少重试次数，较短延迟（考虑浏览器性能和用户体验）
                Self::ExponentialBackoffWithJitter {
                    max_attempts: 2,
                    initial_delay: Duration::from_millis(50),
                    max_delay: Duration::from_secs(10),
                    multiplier: 2.0,
                    jitter_ratio: 0.1,
                }
            }
            Platform::Desktop => {
                // 桌面端：标准重试策略
                Self::ExponentialBackoffWithJitter {
                    max_attempts: 3,
                    initial_delay: Duration::from_millis(100),
                    max_delay: Duration::from_secs(30),
                    multiplier: 2.0,
                    jitter_ratio: 0.1,
                }
            }
            Platform::Android | Platform::IOS | Platform::HarmonyOS => {
                // 移动端：考虑网络切换和电池优化，适中的重试策略
                Self::ExponentialBackoffWithJitter {
                    max_attempts: 3,
                    initial_delay: Duration::from_millis(200),
                    max_delay: Duration::from_secs(20),
                    multiplier: 2.0,
                    jitter_ratio: 0.15, // 更大的抖动，避免同时重试
                }
            }
        }
    }
}

impl RetryStrategy {
    /// 计算重试延迟
    pub fn calculate_delay(&self, attempt: u32) -> Option<Duration> {
        match self {
            RetryStrategy::NoRetry => None,
            RetryStrategy::Fixed { max_attempts, interval } => {
                if attempt < *max_attempts {
                    Some(*interval)
                } else {
                    None
                }
            }
            RetryStrategy::ExponentialBackoff {
                max_attempts,
                initial_delay,
                max_delay,
                multiplier,
            } => {
                if attempt < *max_attempts {
                    let delay_ms = (initial_delay.as_millis() as f64 * multiplier.powi(attempt as i32)) as u64;
                    Some(Duration::from_millis(delay_ms.min(max_delay.as_millis() as u64)))
                } else {
                    None
                }
            }
            RetryStrategy::ExponentialBackoffWithJitter {
                max_attempts,
                initial_delay,
                max_delay,
                multiplier,
                jitter_ratio,
            } => {
                if attempt < *max_attempts {
                    let base_delay_ms = (initial_delay.as_millis() as f64 * multiplier.powi(attempt as i32)) as u64;
                    let max_delay_ms = max_delay.as_millis() as u64;
                    let delay_ms = base_delay_ms.min(max_delay_ms);
                    
                    // 添加抖动
                    let jitter = (delay_ms as f64 * jitter_ratio) as u64;
                    let final_delay = delay_ms + jitter;
                    
                    Some(Duration::from_millis(final_delay.min(max_delay_ms)))
                } else {
                    None
                }
            }
        }
    }
    
    /// 判断是否应该重试
    pub fn should_retry(&self, error: &SDKError, attempt: u32) -> bool {
        if !error.is_retryable() {
            return false;
        }
        
        match self {
            RetryStrategy::NoRetry => false,
            RetryStrategy::Fixed { max_attempts, .. } => attempt < *max_attempts,
            RetryStrategy::ExponentialBackoff { max_attempts, .. } => attempt < *max_attempts,
            RetryStrategy::ExponentialBackoffWithJitter { max_attempts, .. } => attempt < *max_attempts,
        }
    }
}

/// 错误恢复器
/// 
/// 根据错误类型和策略自动执行恢复操作
pub struct ErrorRecovery {
    /// 重试策略
    retry_strategy: Arc<RwLock<RetryStrategy>>,
}

impl ErrorRecovery {
    pub fn new(strategy: RetryStrategy) -> Self {
        Self {
            retry_strategy: Arc::new(RwLock::new(strategy)),
        }
    }
    
    /// 执行带重试的操作
    pub async fn execute_with_retry<F, Fut, T>(&self, mut operation: F) -> Result<T, SDKError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, SDKError>>,
    {
        let mut attempt = 0;
        let strategy = self.retry_strategy.read().await;
        
        loop {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    attempt += 1;
                    
                    if strategy.should_retry(&e, attempt) {
                        if let Some(delay) = strategy.calculate_delay(attempt) {
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                    }
                    
                    return Err(e);
                }
            }
        }
    }
    
    /// 更新重试策略
    pub async fn update_strategy(&self, strategy: RetryStrategy) {
        *self.retry_strategy.write().await = strategy;
    }
}

/// 熔断器状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CircuitBreakerState {
    /// 关闭（正常）
    Closed,
    /// 打开（熔断）
    Open,
    /// 半开（尝试恢复）
    HalfOpen,
}

/// 熔断器
/// 
/// 当错误率超过阈值时，自动熔断，避免雪崩
pub struct CircuitBreaker {
    state: Arc<RwLock<CircuitBreakerState>>,
    failure_count: Arc<RwLock<u32>>,
    success_count: Arc<RwLock<u32>>,
    failure_threshold: u32,
    success_threshold: u32,
    timeout: Duration,
    last_failure_time: Arc<RwLock<Option<Instant>>>,
}

impl CircuitBreaker {
    /// 创建新的熔断器
    pub fn new(failure_threshold: u32, success_threshold: u32, timeout: Duration) -> Self {
        Self {
            state: Arc::new(RwLock::new(CircuitBreakerState::Closed)),
            failure_count: Arc::new(RwLock::new(0)),
            success_count: Arc::new(RwLock::new(0)),
            failure_threshold,
            success_threshold,
            timeout,
            last_failure_time: Arc::new(RwLock::new(None)),
        }
    }
    
    /// 创建默认的熔断器（根据平台自动调整参数）
    pub fn default() -> Self {
        use crate::platform::{get_platform, Platform};
        let platform = get_platform();
        
        match platform {
            Platform::Web => {
                // Web 端：更敏感的熔断（快速失败，快速恢复）
                Self::new(3, 2, Duration::from_secs(5))
            }
            Platform::Desktop => {
                // 桌面端：标准熔断策略
                Self::new(5, 3, Duration::from_secs(30))
            }
            Platform::Android | Platform::IOS | Platform::HarmonyOS => {
                // 移动端：考虑网络切换，适中的熔断策略
                Self::new(5, 3, Duration::from_secs(15))
            }
        }
    }
    
    /// 检查是否允许执行
    /// 
    /// 优化：快速检查，减少锁持有时间
    pub async fn is_open(&self) -> bool {
        // 快速检查状态
        let state = *self.state.read().await;
        match state {
            CircuitBreakerState::Closed => false,
            CircuitBreakerState::Open => {
                // 检查是否超时，可以尝试恢复
                if let Some(last_failure) = *self.last_failure_time.read().await {
                    if last_failure.elapsed() >= self.timeout {
                        // 切换到半开状态
                        *self.state.write().await = CircuitBreakerState::HalfOpen;
                        *self.success_count.write().await = 0;
                        return false;
                    }
                }
                true
            }
            CircuitBreakerState::HalfOpen => false,
        }
    }
    
    /// 记录成功
    pub async fn record_success(&self) {
        let mut state = self.state.write().await;
        match *state {
            CircuitBreakerState::Closed => {
                // 重置失败计数
                *self.failure_count.write().await = 0;
            }
            CircuitBreakerState::HalfOpen => {
                let mut success_count = self.success_count.write().await;
                *success_count += 1;
                if *success_count >= self.success_threshold {
                    // 恢复到关闭状态
                    *state = CircuitBreakerState::Closed;
                    *self.failure_count.write().await = 0;
                    *success_count = 0;
                }
            }
            CircuitBreakerState::Open => {
                // 不应该在打开状态记录成功
            }
        }
    }
    
    /// 记录失败
    /// 
    /// 优化：减少锁持有时间，避免死锁
    pub async fn record_failure(&self) {
        // 快速检查当前状态
        let current_state = *self.state.read().await;
        
        match current_state {
            CircuitBreakerState::Closed => {
                // 更新失败计数
                let should_open = {
                    let mut failure_count = self.failure_count.write().await;
                    *failure_count += 1;
                    *failure_count >= self.failure_threshold
                };
                
                if should_open {
                    // 切换到打开状态
                    *self.state.write().await = CircuitBreakerState::Open;
                    *self.last_failure_time.write().await = Some(Instant::now());
                }
            }
            CircuitBreakerState::HalfOpen => {
                // 半开状态下失败，立即打开
                *self.state.write().await = CircuitBreakerState::Open;
                *self.last_failure_time.write().await = Some(Instant::now());
                *self.success_count.write().await = 0;
            }
            CircuitBreakerState::Open => {
                // 更新失败时间（优化：只在需要时更新）
                let mut last_failure = self.last_failure_time.write().await;
                if last_failure.is_none() {
                    *last_failure = Some(Instant::now());
                }
            }
        }
    }
}

