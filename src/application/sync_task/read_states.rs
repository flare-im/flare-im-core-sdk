//! 已读状态同步任务（Background）：按会话上报本地已读位点（ReadAck）。
//! 构造时注入 [SyncProtocolAdapter]，在 execute 内直接调用。

use std::pin::Pin;
use std::sync::Arc;

use tracing::debug;

use super::super::SyncProtocolAdapter;
use crate::core::{SyncContext, SyncFailurePolicy, SyncMode, SyncResult, SyncTask, SyncTaskResult};

pub struct ReadStatesSyncTask(pub(crate) Arc<SyncProtocolAdapter>);

impl ReadStatesSyncTask {
    pub fn new(handler: Arc<SyncProtocolAdapter>) -> Self {
        Self(handler)
    }
}

impl SyncTask for ReadStatesSyncTask {
    fn id(&self) -> &'static str {
        "read_states"
    }
    fn mode(&self) -> SyncMode {
        SyncMode::Background
    }
    fn weight(&self) -> u32 {
        5
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
            debug!(task = "read_states", "sync phase: read_states start");
            ctx.report_progress("syncing read states");
            let list = ctx.store.conversations.list().await?;
            let ack_count = handler.push_local_read_states(&list).await?;
            debug!(
                task = "read_states",
                ack_count, "read_states ack dispatched"
            );
            debug!(task = "read_states", "sync phase: read_states done");
            Ok(SyncTaskResult::ok())
        })
    }
}
