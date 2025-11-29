//! 生命周期 Trait 定义
//!
//! 定义组件的生命周期接口

use crate::error::SDKResult;
use async_trait::async_trait;
use std::time::Duration;

/// 组件生命周期状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleState {
    /// 未初始化
    Uninitialized,
    
    /// 初始化中
    Initializing,
    
    /// 已初始化（运行中）
    Initialized,
    
    /// 关闭中
    ShuttingDown,
    
    /// 已关闭
    ShutDown,
    
    /// 错误状态
    Error(String),
}

/// 生命周期管理 Trait
/// 
/// 所有需要生命周期管理的组件都应实现此接口
/// 
/// # 生命周期流程
/// 
/// ```
/// Uninitialized -> Initializing -> Initialized -> ShuttingDown -> ShutDown
///                                                      |
///                                                      v
///                                                   Error
/// ```
#[async_trait]
pub trait Lifecycle: Send + Sync {
    /// 获取当前状态
    fn state(&self) -> LifecycleState;
    
    /// 初始化组件
    /// 
    /// # 返回
    /// - `SDKResult<()>`: 初始化结果
    async fn initialize(&mut self) -> SDKResult<()>;
    
    /// 启动组件
    /// 
    /// 在初始化后调用，开始提供服务
    /// 
    /// # 返回
    /// - `SDKResult<()>`: 启动结果
    async fn start(&mut self) -> SDKResult<()>;
    
    /// 停止组件
    /// 
    /// 停止服务，但保留资源
    /// 
    /// # 返回
    /// - `SDKResult<()>`: 停止结果
    async fn stop(&mut self) -> SDKResult<()>;
    
    /// 优雅关闭
    /// 
    /// 等待所有任务完成，然后清理资源
    /// 
    /// # 参数
    /// - `timeout`: 关闭超时时间
    /// 
    /// # 返回
    /// - `SDKResult<()>`: 关闭结果
    async fn shutdown(&mut self, timeout: Option<Duration>) -> SDKResult<()>;
    
    /// 健康检查
    /// 
    /// 检查组件是否健康运行
    /// 
    /// # 返回
    /// - `SDKResult<bool>`: true 表示健康，false 表示不健康
    async fn health_check(&self) -> SDKResult<bool>;
}

/// 生命周期事件
#[derive(Debug, Clone)]
pub enum LifecycleEvent {
    /// 状态变化
    StateChanged {
        from: LifecycleState,
        to: LifecycleState,
    },
    
    /// 初始化完成
    Initialized,
    
    /// 启动完成
    Started,
    
    /// 停止完成
    Stopped,
    
    /// 关闭完成
    ShutDown,
    
    /// 错误发生
    Error(String),
}

/// 生命周期观察者
/// 
/// 监听生命周期事件
#[async_trait]
pub trait LifecycleObserver: Send + Sync {
    /// 处理生命周期事件
    async fn on_event(&self, event: LifecycleEvent);
}

