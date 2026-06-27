//! 会话摘要列表同步（供多端补偿、非 Init 全量登录流程单独触发）。

use std::pin::Pin;

use crate::kernel::SyncRunContext;
use crate::shared::error::Result;

pub trait ConversationSummarySync: Send + Sync {
    fn sync_conversation_summaries(
        &self,
        user_id: &str,
        run: SyncRunContext,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;

    fn sync_foreground_convergence(
        &self,
        user_id: &str,
        run: SyncRunContext,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        self.sync_conversation_summaries(user_id, run)
    }
}
