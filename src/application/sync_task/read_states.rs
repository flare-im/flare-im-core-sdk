//! 已读状态同步任务（Background）：按会话上报本地已读位点（ReadAck）。
//! 构造时注入 [SyncHandler]，在 execute 内直接调用。

use std::pin::Pin;
use std::sync::Arc;

use tracing::info;

use super::super::handlers::SyncHandler;
use crate::core::{SyncContext, SyncMode, SyncResult, SyncTask, SyncTaskResult};

pub struct ReadStatesSyncTask(pub(crate) Arc<SyncHandler>);

impl ReadStatesSyncTask {
    pub fn new(handler: Arc<SyncHandler>) -> Self {
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
    fn execute(
        &self,
        ctx: SyncContext,
    ) -> Pin<Box<dyn std::future::Future<Output = SyncResult<SyncTaskResult>> + Send>> {
        let handler = self.0.clone();
        Box::pin(async move {
            info!(task = "read_states", "sync phase: read_states start");
            ctx.report_progress("syncing read states");
            let list = ctx.store.conversations.list().await?;
            for c in list {
                let read_seq = ctx
                    .store
                    .cursors
                    .get_conversation_cursor(&ctx.user_id, &c.conversation_id)
                    .await?
                    .map(|cursor| cursor.last_seq)
                    .unwrap_or(c.last_read_seq);
                if read_seq > 0 {
                    let _ = handler.send_read_ack(&c.conversation_id, read_seq).await;
                }
            }
            info!(task = "read_states", "sync phase: read_states done");
            Ok(SyncTaskResult::ok())
        })
    }
}
