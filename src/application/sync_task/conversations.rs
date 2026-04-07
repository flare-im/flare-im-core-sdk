//! 会话列表同步任务（Init）：拉取服务端会话列表并落库，bootstrap 时由编排器执行。
//! 构造时注入 [SyncProtocolAdapter]，在 execute 内直接调用，与用户扩展任务一致。

use std::pin::Pin;
use std::sync::Arc;

use tracing::info;

use super::super::SyncProtocolAdapter;
use crate::core::{SyncContext, SyncMode, SyncResult, SyncTask, SyncTaskResult};

pub struct ConversationsSyncTask(pub(crate) Arc<SyncProtocolAdapter>);

impl ConversationsSyncTask {
    pub fn new(handler: Arc<SyncProtocolAdapter>) -> Self {
        Self(handler)
    }
}

impl SyncTask for ConversationsSyncTask {
    fn id(&self) -> &'static str {
        "conversations"
    }
    fn mode(&self) -> SyncMode {
        SyncMode::Init
    }
    fn weight(&self) -> u32 {
        10
    }
    fn execute(
        &self,
        ctx: SyncContext,
    ) -> Pin<Box<dyn std::future::Future<Output = SyncResult<SyncTaskResult>> + Send>> {
        let handler = self.0.clone();
        let user_id = ctx.user_id.clone();
        Box::pin(async move {
            info!(task = "conversations", "sync phase: conversations start");
            ctx.report_progress("syncing conversations");
            handler.sync_conversations_impl(&user_id).await?;
            info!(task = "conversations", "sync phase: conversations done");
            Ok(SyncTaskResult::ok())
        })
    }
}
