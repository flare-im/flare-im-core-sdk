//! 连接池管理器
//!
//! 提供动态连接池配置、监控和调整功能

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::Instant;
use tracing::{debug, info, warn};

/// 连接池配置
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// 最小连接数
    pub min_connections: u32,
    
    /// 最大连接数
    pub max_connections: u32,
    
    /// 连接获取超时时间
    pub acquire_timeout: Duration,
    
    /// 连接空闲超时时间
    pub idle_timeout: Duration,
    
    /// 连接最大生存时间
    pub max_lifetime: Duration,
    
    /// 是否启用动态调整
    pub enable_dynamic_adjustment: bool,
}

impl Default for PoolConfig {
    fn default() -> Self {
        use crate::shared::platform::{get_platform, Platform};
        let platform = get_platform();
        
        let (min, max) = match platform {
            Platform::Web => (1, 5),
            Platform::Desktop => (5, 20),
            Platform::Android | Platform::IOS | Platform::HarmonyOS => (2, 10),
        };
        
        Self {
            min_connections: min,
            max_connections: max,
            acquire_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(300),
            max_lifetime: Duration::from_secs(3600),
            enable_dynamic_adjustment: true,
        }
    }
}

/// 连接池统计信息
#[derive(Debug, Clone)]
pub struct PoolStats {
    /// 当前活跃连接数
    pub active_connections: u32,
    
    /// 当前空闲连接数
    pub idle_connections: u32,
    
    /// 等待连接的请求数
    pub waiting_requests: u32,
    
    /// 连接获取总次数
    pub total_acquires: u64,
    
    /// 连接获取失败次数
    pub failed_acquires: u64,
    
    /// 平均获取延迟（毫秒）
    pub avg_acquire_latency_ms: u64,
    
    /// 最后更新时间
    pub last_update: Instant,
}

impl Default for PoolStats {
    fn default() -> Self {
        Self {
            active_connections: 0,
            idle_connections: 0,
            waiting_requests: 0,
            total_acquires: 0,
            failed_acquires: 0,
            avg_acquire_latency_ms: 0,
            last_update: Instant::now(),
        }
    }
}

/// 连接池管理器
/// 
/// 监控连接池状态，并根据负载动态调整配置
pub struct PoolManager {
    config: Arc<RwLock<PoolConfig>>,
    stats: Arc<RwLock<PoolStats>>,
}

impl PoolManager {
    /// 创建新的连接池管理器
    pub fn new(config: PoolConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            stats: Arc::new(RwLock::new(PoolStats::default())),
        }
    }
    
    /// 创建默认的连接池管理器
    pub fn default() -> Self {
        Self::new(PoolConfig::default())
    }
    
    /// 获取当前配置
    pub async fn get_config(&self) -> PoolConfig {
        self.config.read().await.clone()
    }
    
    /// 更新配置
    pub async fn update_config(&self, config: PoolConfig) {
        *self.config.write().await = config;
        info!("Pool configuration updated: min={}, max={}", 
              config.min_connections, config.max_connections);
    }
    
    /// 记录连接获取
    pub async fn record_acquire(&self, success: bool, latency: Duration) {
        let mut stats = self.stats.write().await;
        stats.total_acquires += 1;
        if !success {
            stats.failed_acquires += 1;
        }
        
        // 更新平均延迟（简单移动平均）
        let total = stats.total_acquires;
        let current_avg = stats.avg_acquire_latency_ms;
        let new_latency_ms = latency.as_millis() as u64;
        stats.avg_acquire_latency_ms = 
            (current_avg * (total - 1) + new_latency_ms) / total;
        stats.last_update = Instant::now();
    }
    
    /// 更新连接池状态
    pub async fn update_stats(
        &self,
        active: u32,
        idle: u32,
        waiting: u32,
    ) {
        let mut stats = self.stats.write().await;
        stats.active_connections = active;
        stats.idle_connections = idle;
        stats.waiting_requests = waiting;
        stats.last_update = Instant::now();
    }
    
    /// 获取统计信息
    pub async fn get_stats(&self) -> PoolStats {
        self.stats.read().await.clone()
    }
    
    /// 检查是否需要调整连接池大小
    /// 
    /// 根据当前负载和统计信息，建议新的连接池配置
    pub async fn should_adjust(&self) -> Option<PoolConfig> {
        let config = self.config.read().await;
        if !config.enable_dynamic_adjustment {
            return None;
        }
        
        let stats = self.stats.read().await;
        
        // 如果失败率过高，可能需要增加连接数
        let failure_rate = if stats.total_acquires > 0 {
            stats.failed_acquires as f64 / stats.total_acquires as f64
        } else {
            0.0
        };
        
        // 如果平均延迟过高，可能需要增加连接数
        let high_latency = stats.avg_acquire_latency_ms > 100; // 100ms
        
        // 如果有等待请求，可能需要增加连接数
        let has_waiting = stats.waiting_requests > 0;
        
        // 如果连接使用率过高（>80%），可能需要增加连接数
        let usage_rate = if config.max_connections > 0 {
            stats.active_connections as f64 / config.max_connections as f64
        } else {
            0.0
        };
        
        let mut new_config = config.clone();
        let mut adjusted = false;
        
        // 需要增加连接数的情况
        if failure_rate > 0.1 || high_latency || has_waiting || usage_rate > 0.8 {
            let new_max = (config.max_connections as f64 * 1.2).ceil() as u32;
            if new_max > config.max_connections && new_max <= 100 {
                new_config.max_connections = new_max;
                adjusted = true;
                debug!(
                    "Suggesting pool size increase: {} -> {} (failure_rate={:.2}, latency={}ms, waiting={}, usage={:.2})",
                    config.max_connections, new_max, failure_rate, stats.avg_acquire_latency_ms,
                    stats.waiting_requests, usage_rate
                );
            }
        }
        
        // 需要减少连接数的情况（连接使用率过低且空闲连接过多）
        if usage_rate < 0.3 && stats.idle_connections > config.min_connections {
            let new_max = (config.max_connections as f64 * 0.9).floor() as u32;
            if new_max >= config.min_connections && new_max < config.max_connections {
                new_config.max_connections = new_max;
                adjusted = true;
                debug!(
                    "Suggesting pool size decrease: {} -> {} (usage={:.2}, idle={})",
                    config.max_connections, new_max, usage_rate, stats.idle_connections
                );
            }
        }
        
        if adjusted {
            Some(new_config)
        } else {
            None
        }
    }
    
    /// 重置统计信息
    pub async fn reset_stats(&self) {
        *self.stats.write().await = PoolStats::default();
    }
}

