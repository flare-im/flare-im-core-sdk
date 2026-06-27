//! 关键事件补偿任务（Init）：基于 storage.QueryEvents 回放撤回/编辑/删除等关键事件。

use std::pin::Pin;
use std::sync::Arc;

use tracing::debug;

use super::super::SyncProtocolAdapter;
use crate::kernel::{
    SyncContext, SyncFailurePolicy, SyncMode, SyncResult, SyncTask, SyncTaskResult,
};

pub struct KeyEventsSyncTask(pub(crate) Arc<SyncProtocolAdapter>);

impl KeyEventsSyncTask {
    pub fn new(handler: Arc<SyncProtocolAdapter>) -> Self {
        Self(handler)
    }
}

impl SyncTask for KeyEventsSyncTask {
    fn id(&self) -> &'static str {
        "key_events"
    }

    fn mode(&self) -> SyncMode {
        // 关键事件回放可能触发 QueryEvents 远程拉取（默认 30s 超时），
        // 放在 Background 可避免阻塞登录首屏可用性。
        SyncMode::Background
    }

    fn weight(&self) -> u32 {
        15
    }

    fn failure_policy(&self) -> SyncFailurePolicy {
        SyncFailurePolicy::Continue
    }

    fn execute(
        &self,
        ctx: SyncContext,
    ) -> Pin<Box<dyn std::future::Future<Output = SyncResult<SyncTaskResult>> + Send>> {
        let handler = self.0.clone();
        Box::pin(async move {
            debug!(task = "key_events", "sync phase: key events start");
            ctx.report_progress("syncing critical events");
            handler.sync_critical_events().await?;
            debug!(task = "key_events", "sync phase: key events done");
            Ok(SyncTaskResult::ok())
        })
    }
}
