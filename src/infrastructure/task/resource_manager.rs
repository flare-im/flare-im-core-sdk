//! 资源管理器
//!
//! 统一管理 SDK 中的资源清理
//!
//! 参考顶级 IM SDK 设计：
//! - 统一资源清理接口
//! - 优雅关闭机制
//! - 资源泄漏检测

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// 资源类型
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResourceType {
    /// 连接资源
    Connection,
    /// 存储资源
    Storage,
    /// 事件总线
    EventBus,
    /// 请求管理器
    RequestManager,
    /// 其他资源
    Other(String),
}

/// 资源清理器 trait
#[async_trait::async_trait]
pub trait ResourceCleaner: Send + Sync {
    /// 清理资源
    async fn cleanup(&self) -> anyhow::Result<()>;

    /// 获取资源类型
    fn resource_type(&self) -> ResourceType;

    /// 获取资源名称（用于日志）
    fn resource_name(&self) -> &str;
}

/// 资源管理器
///
/// 统一管理所有需要清理的资源
pub struct ResourceManager {
    /// 资源列表
    resources: Arc<RwLock<Vec<Arc<dyn ResourceCleaner>>>>,

    /// 是否正在关闭
    shutting_down: Arc<RwLock<bool>>,
}

impl ResourceManager {
    /// 创建新的资源管理器
    pub fn new() -> Self {
        Self {
            resources: Arc::new(RwLock::new(Vec::new())),
            shutting_down: Arc::new(RwLock::new(false)),
        }
    }

    /// 注册资源
    pub async fn register(&self, resource: Arc<dyn ResourceCleaner>) {
        if *self.shutting_down.read().await {
            warn!(
                resource_type = ?resource.resource_type(),
                resource_name = resource.resource_name(),
                "Attempting to register resource during shutdown"
            );
            return;
        }

        let mut resources = self.resources.write().await;
        resources.push(resource);
        debug!("Resource registered");
    }

    /// 清理所有资源
    pub async fn cleanup_all(&self) -> anyhow::Result<()> {
        *self.shutting_down.write().await = true;
        info!("Resource manager cleaning up...");

        let resources: Vec<Arc<dyn ResourceCleaner>> = {
            let resources_guard = self.resources.read().await;
            resources_guard.clone()
        };

        if resources.is_empty() {
            info!("No resources to cleanup");
            return Ok(());
        }

        info!(resource_count = resources.len(), "Cleaning up resources...");

        let mut errors = Vec::new();
        for resource in resources.iter() {
            let resource_type = resource.resource_type();
            let resource_name = resource.resource_name();

            match resource.cleanup().await {
                Ok(_) => {
                    debug!(
                        resource_type = ?resource_type,
                        resource_name = %resource_name,
                        "Resource cleaned up successfully"
                    );
                }
                Err(e) => {
                    warn!(
                        resource_type = ?resource_type,
                        resource_name = %resource_name,
                        error = %e,
                        "Failed to cleanup resource"
                    );
                    errors.push(format!("{}: {}", resource_name, e));
                }
            }
        }

        // 清空资源列表
        self.resources.write().await.clear();

        if errors.is_empty() {
            info!("All resources cleaned up successfully");
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Some resources failed to cleanup: {}",
                errors.join(", ")
            ))
        }
    }

    /// 获取资源数量
    pub async fn resource_count(&self) -> usize {
        self.resources.read().await.len()
    }

    /// 检查是否正在关闭
    pub async fn is_shutting_down(&self) -> bool {
        *self.shutting_down.read().await
    }
}

impl Default for ResourceManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 资源使用统计
pub struct ResourceStats {
    /// 资源创建时间
    created_at: Instant,

    /// 资源使用次数
    use_count: Arc<RwLock<u64>>,
}

impl ResourceStats {
    pub fn new() -> Self {
        Self {
            created_at: Instant::now(),
            use_count: Arc::new(RwLock::new(0)),
        }
    }

    pub async fn increment(&self) {
        *self.use_count.write().await += 1;
    }

    pub async fn get_use_count(&self) -> u64 {
        *self.use_count.read().await
    }

    pub fn age(&self) -> std::time::Duration {
        self.created_at.elapsed()
    }
}

impl Default for ResourceStats {
    fn default() -> Self {
        Self::new()
    }
}
