//! 关键事件补偿任务（Init）：基于 storage.QueryEvents 回放撤回/编辑/删除等关键事件。

use std::pin::Pin;
use std::sync::Arc;

use tracing::info;

use super::super::handlers::SyncHandler;
use crate::core::{SyncContext, SyncMode, SyncResult, SyncTask, SyncTaskResult};

pub struct KeyEventsSyncTask(pub(crate) Arc<SyncHandler>);

impl KeyEventsSyncTask {
    pub fn new(handler: Arc<SyncHandler>) -> Self {
        Self(handler)
    }
}

impl SyncTask for KeyEventsSyncTask {
    fn id(&self) -> &'static str {
        "key_events"
    }

    fn mode(&self) -> SyncMode {
        SyncMode::Init
    }

    fn weight(&self) -> u32 {
        15
    }

    fn execute(
        &self,
        ctx: SyncContext,
    ) -> Pin<Box<dyn std::future::Future<Output = SyncResult<SyncTaskResult>> + Send>> {
        let handler = self.0.clone();
        Box::pin(async move {
            info!(task = "key_events", "sync phase: key events start");
            ctx.report_progress("syncing critical events");
            handler.sync_critical_events().await?;
            info!(task = "key_events", "sync phase: key events done");
            Ok(SyncTaskResult::ok())
        })
    }
}
