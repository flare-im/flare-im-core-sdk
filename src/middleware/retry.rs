use std::time::Duration;

/// 重试中间件配置
pub struct RetryMiddleware {
    pub max_retries: u32,
    pub base_delay: Duration,
}

impl RetryMiddleware {
    pub fn new(max_retries: u32, base_delay: Duration) -> Self {
        Self { max_retries, base_delay }
    }

    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        self.base_delay * 2u32.pow(attempt.min(5))
    }
}

impl Default for RetryMiddleware {
    fn default() -> Self {
        Self::new(3, Duration::from_millis(500))
    }
}
