//! 任务执行器定义
//!
//! 定义任务执行器 trait 和上下文

use crate::connection::ConnectionManager;
use crate::event::EventBus;
use crate::protocol::RequestManager;
use crate::service::sync::SyncConfig;
use crate::storage::StorageBackend;
use crate::task::standard::{TaskResult, TaskExecutionMode, TaskType};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

/// 同步上下文
/// 
/// 提供给任务执行器的上下文信息
#[derive(Clone)]
pub struct SyncContext {
    /// 连接管理器
    pub connection: Arc<ConnectionManager>,
    
    /// 本地存储
    pub storage: Arc<dyn StorageBackend>,
    
    /// 事件总线
    pub event_bus: Arc<EventBus>,
    
    /// 请求管理器
    pub request_manager: Arc<RequestManager>,
    
    /// 同步配置
    pub config: SyncConfig,
    
    /// 当前用户 ID
    pub user_id: String,
}

/// 同步任务执行器 trait
/// 
/// 所有同步任务必须实现此接口，支持强制和可选两种模式
/// 
/// # 任务标准
/// 
/// 1. **优先级**：使用 `priority` 模块定义的常量
///    - `priority::CRITICAL` (0-9): 关键系统任务
///    - `priority::HIGH` (10-29): 重要同步任务（如全量同步）
///    - `priority::MEDIUM_HIGH` (30-49): 常规同步任务（如会话同步）
///    - `priority::MEDIUM` (50-69): 普通同步任务（如消息同步）
///    - `priority::LOW` (70-89): 后台同步任务
///    - `priority::LOWEST` (90+): 可延迟任务
/// 
/// 2. **任务类型**：
///    - `Mandatory`: 强制任务，必须执行
///    - `Optional`: 可选任务，由客户端决定是否执行
/// 
/// 3. **执行模式**：
///    - `Sync`: 同步执行，阻塞等待完成
///    - `Async`: 异步执行，在后台执行
/// 
/// # 示例
/// 
/// ```rust,no_run
/// use crate::task::executor::{SyncTaskExecutor, SyncContext};
/// use crate::task::standard::{priority, TaskResult};
/// use async_trait::async_trait;
/// 
/// struct MySyncTask;
/// 
/// #[async_trait]
/// impl SyncTaskExecutor for MySyncTask {
///     fn name(&self) -> &str {
///         "MySyncTask"
///     }
///     
///     fn description(&self) -> &str {
///         "我的自定义同步任务"
///     }
///     
///     fn task_type(&self) -> TaskType {
///         TaskType::Optional  // 可选任务
///     }
///     
///     fn priority(&self) -> u32 {
///         priority::MEDIUM  // 中等优先级
///     }
///     
///     fn execution_mode(&self) -> TaskExecutionMode {
///         TaskExecutionMode::Async  // 异步执行
///     }
///     
///     async fn execute(&self, context: &SyncContext) -> Result<TaskResult> {
///         // 执行同步逻辑
///         Ok(TaskResult::success(
///             "my-task".to_string(),
///             0,
///             0,
///             100,
///         ))
///     }
/// }
/// ```
#[async_trait]
pub trait SyncTaskExecutor: Send + Sync {
    /// 任务名称（用于日志和调试）
    fn name(&self) -> &str;
    
    /// 任务描述
    fn description(&self) -> &str {
        ""
    }
    
    /// 任务类型（强制或可选）
    fn task_type(&self) -> TaskType {
        TaskType::Optional
    }
    
    /// 任务优先级（数字越小优先级越高）
    /// 
    /// 默认使用 `priority::DEFAULT` (100)
    fn priority(&self) -> u32 {
        crate::task::standard::priority::DEFAULT
    }
    
    /// 执行模式（同步或异步）
    fn execution_mode(&self) -> TaskExecutionMode {
        TaskExecutionMode::Async
    }
    
    /// 执行任务
    /// 
    /// # 参数
    /// - `context`: 同步上下文，包含连接、存储等资源
    /// 
    /// # 返回
    /// - `Result<TaskResult>`: 任务执行结果
    async fn execute(&self, context: &SyncContext) -> Result<TaskResult>;
}

/// 任务执行器包装器
/// 
/// 用于在运行时获取任务执行器的信息
pub struct TaskExecutorWrapper {
    executor: Arc<dyn SyncTaskExecutor>,
}

impl TaskExecutorWrapper {
    pub fn new(executor: Arc<dyn SyncTaskExecutor>) -> Self {
        Self { executor }
    }
    
    pub fn name(&self) -> &str {
        self.executor.name()
    }
    
    pub fn description(&self) -> &str {
        self.executor.description()
    }
    
    pub fn task_type(&self) -> TaskType {
        self.executor.task_type()
    }
    
    pub fn priority(&self) -> u32 {
        self.executor.priority()
    }
    
    pub fn execution_mode(&self) -> TaskExecutionMode {
        self.executor.execution_mode()
    }
    
    pub async fn execute(&self, context: &SyncContext) -> Result<TaskResult> {
        self.executor.execute(context).await
    }
}

