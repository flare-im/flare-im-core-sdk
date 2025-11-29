//! 连接健康检查
//!
//! 提供连接稳定性检查和健康监控

use crate::connection::ConnectionManager;
use crate::error::{SDKError, SDKResult};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant, Interval};
use tracing::{debug, info, warn};

/// 连接健康检查器
pub struct ConnectionHealthChecker {
    connection: Arc<ConnectionManager>,
    last_heartbeat: Arc<RwLock<Option<Instant>>>,
    check_interval: Duration,
    heartbeat_timeout: Duration,
}

impl ConnectionHealthChecker {
    pub fn new(
        connection: Arc<ConnectionManager>,
        check_interval: Duration,
        heartbeat_timeout: Duration,
    ) -> Self {
        Self {
            connection,
            last_heartbeat: Arc::new(RwLock::new(None)),
            check_interval,
            heartbeat_timeout,
        }
    }

    /// 记录心跳
    pub async fn record_heartbeat(&self) {
        *self.last_heartbeat.write().await = Some(Instant::now());
    }

    /// 检查连接健康状态
    pub async fn check_health(&self) -> SDKResult<bool> {
        // 1. 检查连接状态
        let is_connected = self.connection.is_connected().await;
        if !is_connected {
            return Ok(false);
        }

        // 2. 检查心跳超时
        let last_heartbeat = self.last_heartbeat.read().await;
        if let Some(last) = *last_heartbeat {
            if last.elapsed() > self.heartbeat_timeout {
                warn!(
                    elapsed_ms = last.elapsed().as_millis(),
                    timeout_ms = self.heartbeat_timeout.as_millis(),
                    "Heartbeat timeout"
                );
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// 启动健康检查任务
    pub fn start_health_check(&self) -> tokio::task::JoinHandle<()> {
        let connection = Arc::clone(&self.connection);
        let last_heartbeat = Arc::clone(&self.last_heartbeat);
        let check_interval = self.check_interval;
        let heartbeat_timeout = self.heartbeat_timeout;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(check_interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                // 检查连接状态
                let is_connected = connection.is_connected().await;
                if !is_connected {
                    debug!("Connection health check: disconnected");
                    continue;
                }

                // 检查心跳超时
                let last_heartbeat_guard = last_heartbeat.read().await;
                if let Some(last) = *last_heartbeat_guard {
                    if last.elapsed() > heartbeat_timeout {
                        warn!(
                            elapsed_ms = last.elapsed().as_millis(),
                            "Connection health check failed: heartbeat timeout"
                        );
                        // 可以在这里触发重连或通知
                    }
                }
            }
        })
    }
}

/// 连接稳定性检查
pub struct ConnectionStabilityChecker {
    connection: Arc<ConnectionManager>,
    min_connection_duration: Duration,
    connection_start_time: Arc<RwLock<Option<Instant>>>,
}

impl ConnectionStabilityChecker {
    pub fn new(
        connection: Arc<ConnectionManager>,
        min_connection_duration: Duration,
    ) -> Self {
        Self {
            connection,
            min_connection_duration,
            connection_start_time: Arc::new(RwLock::new(None)),
        }
    }

    /// 记录连接开始时间
    pub async fn record_connection_start(&self) {
        *self.connection_start_time.write().await = Some(Instant::now());
    }

    /// 检查连接是否稳定
    pub async fn is_stable(&self) -> bool {
        let start_time = self.connection_start_time.read().await;
        if let Some(start) = *start_time {
            let elapsed = start.elapsed();
            if elapsed >= self.min_connection_duration {
                return true;
            }
        }
        false
    }

    /// 检查连接是否过早断开
    pub async fn check_premature_disconnect(&self) -> Option<Duration> {
        let start_time = self.connection_start_time.read().await;
        if let Some(start) = *start_time {
            let elapsed = start.elapsed();
            if elapsed < self.min_connection_duration {
                return Some(elapsed);
            }
        }
        None
    }
}

