//! 任务调度 API 实现
//!
//! 提供任务注册、调度、状态查询等功能

use crate::api::FlareIMClient;
use crate::api::traits::TaskApi;
use anyhow::Result;
use std::sync::Arc;

impl TaskApi for FlareIMClient {
    async fn register_task(
        &self,
        executor: Arc<dyn crate::infrastructure::task::executor::SyncTaskExecutor>,
    ) {
        self.task_scheduler.register_task(executor).await;
    }

    async fn unregister_task(&self, name: &str) -> bool {
        self.task_scheduler.unregister_task(name).await
    }

    async fn get_registered_tasks(&self) -> Vec<String> {
        self.task_scheduler.get_registered_tasks().await
    }

    async fn schedule_task_by_name(
        &self,
        task_name: &str,
        task_id: Option<String>,
    ) -> Result<String> {
        self.task_scheduler
            .schedule_task_by_name(task_name, task_id)
            .await
    }

    async fn get_task_status(
        &self,
        task_id: &str,
    ) -> Option<crate::infrastructure::task::standard::TaskStatus> {
        self.task_scheduler.get_task_status(task_id).await
    }

    async fn cancel_task(&self, task_id: &str) -> bool {
        self.task_scheduler.cancel_task(task_id).await
    }

    async fn get_task_scheduler_stats(&self) -> crate::infrastructure::task::TaskSchedulerStats {
        self.task_scheduler.get_stats().await
    }

    /// 获取任务调度器性能快照（新增：用于性能监控）
    async fn get_task_scheduler_performance(
        &self,
    ) -> crate::infrastructure::task::TaskSchedulerPerformanceSnapshot {
        self.task_scheduler.get_performance_snapshot().await
    }
}
