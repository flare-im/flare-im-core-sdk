//! 已读状态同步任务（Background）：按会话上报本地已读位点（ReadAck）。
//! 构造时注入 [SyncProtocolAdapter]，在 execute 内直接调用。

use std::pin::Pin;
use std::sync::Arc;

use tracing::info;

use super::super::SyncProtocolAdapter;
use crate::core::{SyncContext, SyncMode, SyncResult, SyncTask, SyncTaskResult};

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
    fn execute(
        &self,
        ctx: SyncContext,
    ) -> Pin<Box<dyn std::future::Future<Output = SyncResult<SyncTaskResult>> + Send>> {
        let handler = self.0.clone();
        Box::pin(async move {
            info!(task = "read_states", "sync phase: read_states start");
            ctx.report_progress("syncing read states");
            let list = ctx.store.conversations.list().await?;
            let mut ack_count = 0usize;
            for c in list {
                // 只允许上报“真实已读位点（last_read_seq）”。
                // 不能使用同步游标 last_seq：last_seq 代表“已拉取/已同步到本地”，
                // 并不等于“用户已阅读”，否则会导致服务端未读数被错误清零或少算。
                let mut read_seq = c.last_read_seq;
                // 自愈兜底：历史版本可能把 last_read_seq 覆盖为 0，
                // 但本地 unread 已是 0（用户已读）。此时补发 max_seq，推动服务端收敛。
                if c.unread_count == 0 && c.max_seq > 0 && read_seq < c.max_seq {
                    read_seq = c.max_seq;
                }
                if read_seq > 0 {
                    info!(
                        task = "read_states",
                        conversation_id = %c.conversation_id,
                        unread_count = c.unread_count,
                        max_seq = c.max_seq,
                        last_read_seq = c.last_read_seq,
                        ack_read_seq = read_seq,
                        "dispatch read ack"
                    );
                    let _ = handler.send_read_ack(&c.conversation_id, read_seq).await;
                    ack_count = ack_count.saturating_add(1);
                }
            }
            info!(task = "read_states", ack_count = ack_count, "read_states ack dispatched");
            info!(task = "read_states", "sync phase: read_states done");
            Ok(SyncTaskResult::ok())
        })
    }
}
