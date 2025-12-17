//! 工具方法 API 实现
//!
//! 提供性能指标、存储访问等工具方法

use crate::api::FlareIMClient;
use crate::api::traits::UtilityApi;
use anyhow::Result;
use std::sync::Arc;

impl UtilityApi for FlareIMClient {
    async fn user_id(&self) -> Result<String> {
        Ok(self.user_id.read().await.clone())
    }

    fn metrics_snapshot(&self) -> crate::shared::metrics::MetricsSnapshot {
        self.metrics.snapshot()
    }

    fn reset_metrics(&self) {
        self.metrics.reset();
    }

    fn task_manager(&self) -> Arc<crate::infrastructure::task::TaskManager> {
        Arc::clone(&self.task_manager)
    }

    fn task_scheduler(&self) -> Arc<crate::infrastructure::task::TaskScheduler> {
        Arc::clone(&self.task_scheduler)
    }

    fn storage(&self) -> Arc<dyn crate::infrastructure::storage::StorageBackend> {
        Arc::clone(&self.storage)
    }

    fn message_command_handler(&self) -> Arc<crate::application::MessageCommandHandler> {
        Arc::clone(&self.message_command_handler)
    }

    fn message_query_handler(&self) -> Arc<crate::application::MessageQueryHandler> {
        Arc::clone(&self.message_query_handler)
    }

    #[cfg(debug_assertions)]
    fn leak_detector(&self) -> Arc<crate::shared::memory_leak_detector::MemoryLeakDetector> {
        Arc::clone(&self.leak_detector)
    }
}
