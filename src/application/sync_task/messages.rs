//! 消息同步任务（Init）：按会话列表逐会话拉取消息，限流并发请求。
//! 构造时注入 [SyncProtocolAdapter]，在 execute 内直接调用。

use std::pin::Pin;
use std::sync::Arc;

use futures::stream::{self, StreamExt};
use tracing::info;

use super::super::SyncProtocolAdapter;
use crate::core::{SyncContext, SyncMode, SyncResult, SyncTask, SyncTaskResult};

pub struct MessagesSyncTask(pub(crate) Arc<SyncProtocolAdapter>);
const MAX_SYNC_CONCURRENCY: usize = 8;

impl MessagesSyncTask {
    pub fn new(handler: Arc<SyncProtocolAdapter>) -> Self {
        Self(handler)
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
    fn execute(
        &self,
        ctx: SyncContext,
    ) -> Pin<Box<dyn std::future::Future<Output = SyncResult<SyncTaskResult>> + Send>> {
        let handler = self.0.clone();
        Box::pin(async move {
            info!(task = "messages", "sync phase: messages start");
            ctx.report_progress("syncing messages");
            let list: Vec<crate::model::Conversation> = ctx.store.conversations.list().await?;
            let ids: Vec<String> = list.into_iter().map(|c| c.conversation_id).collect();

            let mut failed = 0usize;
            let mut synced = 0usize;
            let mut jobs = stream::iter(ids.into_iter().map(|id| {
                let handler = handler.clone();
                async move { (id.clone(), handler.sync_conversation(&id).await) }
            }))
            .buffer_unordered(MAX_SYNC_CONCURRENCY);

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
            info!(
                task = "messages",
                synced, failed, "sync phase: messages result"
            );
            info!(task = "messages", "sync phase: messages done");
            Ok(SyncTaskResult::ok())
        })
    }
}
