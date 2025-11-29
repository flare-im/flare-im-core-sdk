//! 生命周期管理器
//!
//! 管理多个组件的生命周期，支持统一启动和关闭

use crate::lifecycle::{Lifecycle, LifecycleObserver};
use crate::error::{SDKResult, SDKError};
use crate::platform::{get_platform, Platform};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use std::time::Duration;

/// 生命周期管理器
/// 
/// 统一管理多个组件的生命周期
pub struct LifecycleManager {
    /// 组件列表（名称 -> 组件）
    components: Arc<RwLock<HashMap<String, Arc<RwLock<dyn Lifecycle>>>>>,
    
    /// 观察者列表
    observers: Arc<RwLock<Vec<Arc<dyn LifecycleObserver>>>>,
    
    /// 关闭超时时间
    shutdown_timeout: Duration,
}

impl LifecycleManager {
    /// 创建新的生命周期管理器
    /// 
    /// 根据平台自动调整关闭超时时间
    pub fn new(shutdown_timeout: Option<Duration>) -> Self {
        let timeout = shutdown_timeout.unwrap_or_else(|| {
            // 根据平台调整默认超时时间
            match get_platform() {
                Platform::Web => Duration::from_secs(5), // Web 端较短超时
                Platform::Desktop => Duration::from_secs(30), // 桌面端较长超时
                Platform::Android | Platform::IOS | Platform::HarmonyOS => Duration::from_secs(10), // 移动端中等超时
            }
        });
        
        Self {
            components: Arc::new(RwLock::new(HashMap::new())),
            observers: Arc::new(RwLock::new(Vec::new())),
            shutdown_timeout: timeout,
        }
    }
    
    /// 创建默认的生命周期管理器（使用平台特定的超时时间）
    pub fn default() -> Self {
        Self::new(None)
    }
    
    /// 注册组件
    pub async fn register(&self, name: String, component: Arc<RwLock<dyn Lifecycle>>) {
        self.components.write().await.insert(name, component);
    }
    
    /// 注册观察者
    pub async fn add_observer(&self, observer: Arc<dyn LifecycleObserver>) {
        self.observers.write().await.push(observer);
    }
    
    /// 初始化所有组件
    /// 
    /// 优化：并行初始化，提高启动速度
    pub async fn initialize_all(&self) -> SDKResult<()> {
        // 快速获取组件列表并释放锁
        let components: Vec<(String, Arc<RwLock<dyn Lifecycle>>)> = {
            let comps = self.components.read().await;
            comps.iter().map(|(k, v)| (k.clone(), Arc::clone(v))).collect()
        };
        
        // 并行初始化所有组件
        let mut init_tasks = Vec::new();
        for (name, component) in components {
            let name_clone = name.clone();
            let comp = Arc::clone(&component);
            init_tasks.push(tokio::spawn(async move {
                let mut c = comp.write().await;
                match c.initialize().await {
                    Ok(()) => Ok(name_clone),
                    Err(e) => Err(format!("{}: {}", name_clone, e)),
                }
            }));
        }
        
        // 等待所有初始化完成
        let mut errors = Vec::new();
        for task in init_tasks {
            match task.await {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => errors.push(e),
                Err(e) => errors.push(format!("Task join error: {}", e)),
            }
        }
        
        if errors.is_empty() {
            Ok(())
        } else {
            Err(SDKError::internal(format!("初始化失败: {}", errors.join(", "))))
        }
    }
    
    /// 启动所有组件
    /// 
    /// 优化：并行启动，提高启动速度
    pub async fn start_all(&self) -> SDKResult<()> {
        // 快速获取组件列表并释放锁
        let components: Vec<(String, Arc<RwLock<dyn Lifecycle>>)> = {
            let comps = self.components.read().await;
            comps.iter().map(|(k, v)| (k.clone(), Arc::clone(v))).collect()
        };
        
        // 并行启动所有组件
        let mut start_tasks = Vec::new();
        for (name, component) in components {
            let name_clone = name.clone();
            let comp = Arc::clone(&component);
            start_tasks.push(tokio::spawn(async move {
                let mut c = comp.write().await;
                match c.start().await {
                    Ok(()) => Ok(name_clone),
                    Err(e) => Err(format!("{}: {}", name_clone, e)),
                }
            }));
        }
        
        // 等待所有启动完成
        let mut errors = Vec::new();
        for task in start_tasks {
            match task.await {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => errors.push(e),
                Err(e) => errors.push(format!("Task join error: {}", e)),
            }
        }
        
        if errors.is_empty() {
            Ok(())
        } else {
            Err(crate::error::SDKError::internal(format!("启动失败: {}", errors.join(", "))))
        }
    }
    
    /// 优雅关闭所有组件
    pub async fn shutdown_all(&self) -> SDKResult<()> {
        let components = self.components.read().await;
        let mut errors = Vec::new();
        
        // 按逆序关闭（后注册的先关闭）
        let mut names: Vec<String> = components.keys().cloned().collect();
        names.reverse();
        
        for name in names {
            if let Some(component) = components.get(&name) {
                let mut comp = component.write().await;
                if let Err(e) = comp.shutdown(Some(self.shutdown_timeout)).await {
                    errors.push(format!("{}: {}", name, e));
                }
            }
        }
        
        if errors.is_empty() {
            Ok(())
        } else {
            Err(crate::error::SDKError::internal(format!("关闭失败: {}", errors.join(", "))))
        }
    }
    
    /// 健康检查
    pub async fn health_check_all(&self) -> HashMap<String, bool> {
        let components = self.components.read().await;
        let mut results = HashMap::new();
        
        for (name, component) in components.iter() {
            let comp = component.read().await;
            let is_healthy = comp.health_check().await.unwrap_or(false);
            results.insert(name.clone(), is_healthy);
        }
        
        results
    }
}

