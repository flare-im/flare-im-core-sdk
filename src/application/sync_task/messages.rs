//! 消息同步任务（Init）：按会话列表逐会话拉取消息，限流并发请求。
//! 构造时注入 [SyncProtocolAdapter]，在 execute 内直接调用。

use std::pin::Pin;
use std::sync::Arc;

use futures::stream::{self, StreamExt};
use tracing::debug;

use super::super::SyncProtocolAdapter;
use crate::core::{SyncContext, SyncFailurePolicy, SyncMode, SyncResult, SyncTask, SyncTaskResult};

pub struct MessagesSyncTask {
    pub(crate) handler: Arc<SyncProtocolAdapter>,
    max_sync_concurrency: usize,
}
const DEFAULT_SYNC_CONCURRENCY: usize = 4;

impl MessagesSyncTask {
    pub fn new(handler: Arc<SyncProtocolAdapter>) -> Self {
        Self::with_max_sync_concurrency(handler, DEFAULT_SYNC_CONCURRENCY)
    }

    pub fn with_max_sync_concurrency(
        handler: Arc<SyncProtocolAdapter>,
        max_sync_concurrency: usize,
    ) -> Self {
        Self {
            handler,
            max_sync_concurrency: Self::normalize_max_sync_concurrency(max_sync_concurrency),
        }
    }

    pub(crate) fn normalize_max_sync_concurrency(value: usize) -> usize {
        value.max(1)
    }
}

impl SyncTask for MessagesSyncTask {
    fn id(&self) -> &'static str {
        "messages"
    }
    fn mode(&self) -> SyncMode {
        // 会话快照先完成，消息再做分页补齐，避免 Init 阶段并行导致空会话列表。
        SyncMode::Background
    }
    fn weight(&self) -> u32 {
        20
    }
    fn failure_policy(&self) -> SyncFailurePolicy {
        SyncFailurePolicy::Continue
    }
    fn execute(
        &self,
        ctx: SyncContext,
    ) -> Pin<Box<dyn std::future::Future<Output = SyncResult<SyncTaskResult>> + Send>> {
        let handler = self.handler.clone();
        let max_sync_concurrency = self.max_sync_concurrency;
        Box::pin(async move {
            debug!(task = "messages", "sync phase: messages start");
            ctx.report_progress("syncing messages");
            let list: Vec<crate::model::Conversation> = ctx.store.conversations.list().await?;
            let ids: Vec<String> = list.into_iter().map(|c| c.conversation_id).collect();
            let run = ctx.run.clone();

            let mut failed = 0usize;
            let mut synced = 0usize;
            let mut jobs = stream::iter(ids.into_iter().map(|id| {
                let handler = handler.clone();
                let run = run.clone();
                async move {
                    (
                        id.clone(),
                        handler.sync_conversation_with_context(&id, run).await,
                    )
                }
            }))
            .buffer_unordered(max_sync_concurrency);

            while let Some((id, result)) = jobs.next().await {
                match result {
                    Ok(_) => {
                        synced += 1;
                    }
                    Err(e) => {
                        failed += 1;
                        tracing::warn!(conversation_id = %id, error = %e, "sync conversation failed");
                    }
                }
            }
            debug!(
                task = "messages",
                synced, failed, "sync phase: messages result"
            );
            debug!(task = "messages", "sync phase: messages done");
            Ok(SyncTaskResult::ok())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::MessagesSyncTask;

    #[test]
    fn max_sync_concurrency_is_never_zero() {
        assert_eq!(MessagesSyncTask::normalize_max_sync_concurrency(0), 1);
    }

    #[test]
    fn max_sync_concurrency_preserves_mobile_budget() {
        assert_eq!(MessagesSyncTask::normalize_max_sync_concurrency(2), 2);
    }
}
