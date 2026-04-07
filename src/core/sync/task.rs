//! 同步任务抽象：可注册到 [super::SyncManager] 的自定义任务。
//!
//! [SyncMode] 区分 Init（阻塞 UI 就绪）与 Background；[SyncContext] 仅提供 store、进度、检查点。
//! 会话列表同步由同步引擎在连接后自动执行（ConversationsSyncTask），不暴露给上层，降低系统风险。
//! [SessionSyncRunner] 仅暴露单会话消息同步与已读上报，供 IMClient 的 sync_conversation / mark_session_read 使用。

use std::pin::Pin;
use std::sync::Arc;

use crate::store::StoreProvider;

use super::checkpoint::{CheckpointStore, SyncCheckpoint};
use super::error::SyncResult;
use super::progress::SyncProgressReporter;

/// 单会话消息同步与已读上报（由 [crate::application::SyncProtocolAdapter] 实现，供 IMClient 使用）。
/// 会话列表全量同步由同步引擎内部触发，不通过本 trait 暴露。
pub trait SessionSyncRunner: Send + Sync {
    fn request_message_sync(
        &self,
        conversation_id: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = crate::error::Result<()>> + Send + '_>>;
    fn request_message_sync_from_seq(
        &self,
        conversation_id: &str,
        last_seq: u64,
        limit: i32,
    ) -> Pin<Box<dyn std::future::Future<Output = crate::error::Result<()>> + Send + '_>>;
    fn send_read_ack(
        &self,
        conversation_id: &str,
        read_seq: u64,
    ) -> Pin<Box<dyn std::future::Future<Output = crate::error::Result<()>> + Send + '_>>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncMode {
    Init,
    Background,
}

#[derive(Clone, Debug)]
pub enum SyncPhase {
    Start,
    Progress,
    Done,
}

#[derive(Clone, Debug, Default)]
pub struct SyncTaskResult {
    pub success: bool,
    pub message: Option<String>,
    pub cursor: Option<String>,
}

impl SyncTaskResult {
    pub fn ok() -> Self {
        Self {
            success: true,
            message: None,
            cursor: None,
        }
    }
    pub fn ok_with_cursor(cursor: impl Into<String>) -> Self {
        Self {
            success: true,
            message: None,
            cursor: Some(cursor.into()),
        }
    }
    pub fn fail(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            message: Some(msg.into()),
            cursor: None,
        }
    }
}

/// 执行时由引擎注入：store、进度上报、检查点（不包含协议能力，任务自行持有处理器并调用）。
#[derive(Clone)]
pub struct SyncContext {
    pub user_id: String,
    pub task_id: String,
    pub store: StoreProvider,
    pub progress: Option<Arc<dyn SyncProgressReporter>>,
    pub checkpoint_store: Option<Arc<CheckpointStore>>,
}

impl SyncContext {
    pub fn report_progress(&self, detail: impl Into<String>) {
        if let Some(ref p) = self.progress {
            p.report_current(detail.into());
        }
    }

    pub async fn load_checkpoint(&self) -> crate::error::Result<Option<SyncCheckpoint>> {
        let Some(ref store) = self.checkpoint_store else {
            return Ok(None);
        };
        let cp = store.load(&self.task_id).await?;
        Ok(if cp.cursor.is_some() { Some(cp) } else { None })
    }

    pub async fn save_checkpoint(
        &self,
        cursor: Option<impl AsRef<str>>,
    ) -> crate::error::Result<()> {
        let Some(ref store) = self.checkpoint_store else {
            return Ok(());
        };
        let cp = SyncCheckpoint::new(self.task_id.clone(), cursor.map(|c| c.as_ref().to_string()));
        store.save(&cp).await
    }
}

pub trait SyncTask: Send + Sync {
    fn id(&self) -> &'static str;
    fn mode(&self) -> SyncMode {
        SyncMode::Background
    }
    fn weight(&self) -> u32 {
        1
    }
    fn execute(
        &self,
        ctx: SyncContext,
    ) -> Pin<Box<dyn std::future::Future<Output = SyncResult<SyncTaskResult>> + Send>>;
}
