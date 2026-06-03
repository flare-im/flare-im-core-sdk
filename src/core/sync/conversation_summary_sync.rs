//! 会话摘要列表同步（供多端补偿、非 Init 全量登录流程单独触发）。

use std::pin::Pin;

use crate::core::SyncRunContext;
use crate::shared::error::Result;

pub trait ConversationSummarySync: Send + Sync {
    fn sync_conversation_summaries(
        &self,
        user_id: &str,
        run: SyncRunContext,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;
}
