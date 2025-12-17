//! 任务实例定义
//!
//! 定义任务运行时实例和状态管理

use crate::infrastructure::task::executor::{SyncContext, SyncTaskExecutor};
use crate::infrastructure::task::standard::{TaskResult, TaskStatus};
use anyhow::Result;
use std::sync::Arc;

/// 同步任务实例
///
/// 包含任务执行器和运行时信息
#[derive(Clone)]
pub struct SyncTask {
    /// 任务 ID
    pub task_id: String,

    /// 任务执行器
    executor: Arc<dyn SyncTaskExecutor>,

    /// 任务状态
    pub status: TaskStatus,

    /// 进度（0-100）
    pub progress: u8,

    /// 错误信息（如果失败）
    pub error: Option<String>,

    /// 任务结果
    pub result: Option<TaskResult>,
}

impl SyncTask {
    /// 创建新任务
    pub fn new(task_id: String, executor: Arc<dyn SyncTaskExecutor>) -> Self {
        Self {
            task_id,
            executor,
            status: TaskStatus::Pending,
            progress: 0,
            error: None,
            result: None,
        }
    }

    /// 获取任务名称
    pub fn name(&self) -> &str {
        self.executor.name()
    }

    /// 获取任务描述
    pub fn description(&self) -> &str {
        self.executor.description()
    }

    /// 是否为强制加载任务（Blocking Task）
    #[inline]
    pub fn is_blocking(&self) -> bool {
        matches!(
            self.executor.task_type(),
            crate::infrastructure::task::standard::TaskType::Blocking
        )
    }

    /// 是否为后台慢加载任务（Background Task）
    #[inline]
    pub fn is_background(&self) -> bool {
        matches!(
            self.executor.task_type(),
            crate::infrastructure::task::standard::TaskType::Background
        )
    }

    /// 获取任务类型
    pub fn task_type(&self) -> crate::infrastructure::task::standard::TaskType {
        self.executor.task_type()
    }

    /// 获取任务优先级
    pub fn priority(&self) -> u32 {
        self.executor.priority()
    }

    /// 获取执行模式
    pub fn execution_mode(&self) -> crate::infrastructure::task::standard::TaskExecutionMode {
        self.executor.execution_mode()
    }

    /// 获取任务执行器（用于调度器）
    pub fn get_executor(&self) -> Arc<dyn SyncTaskExecutor> {
        Arc::clone(&self.executor)
    }

    /// 执行任务
    pub async fn execute(&mut self, context: &SyncContext) -> Result<()> {
        self.status = TaskStatus::Running;
        self.progress = 0;
        self.error = None;

        match self.executor.execute(context).await {
            Ok(result) => {
                self.status = TaskStatus::Completed;
                self.progress = 100;
                self.result = Some(result);
                Ok(())
            }
            Err(e) => {
                self.status = TaskStatus::Failed;
                self.error = Some(e.to_string());
                Err(e)
            }
        }
    }
}
