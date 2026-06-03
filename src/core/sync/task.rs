//! 同步任务抽象：可注册到 [super::SyncManager] 的自定义任务。
//!
//! [SyncMode] 区分 Init（阻塞 UI 就绪）与 Background；[SyncRunContext] 描述同步触发源、范围和可见性。
//! 会话列表同步由同步引擎在连接后自动执行（ConversationsSyncTask），不暴露给上层，降低系统风险。
//! [SessionSyncRunner] 仅暴露单会话消息同步与已读上报，供 IMClient 的 sync_conversation / mark_session_read 使用。

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::infrastructure::persistence::StoreProvider;

use super::checkpoint::{CheckpointStore, SyncCheckpoint};
use super::error::SyncResult;
use super::progress::SyncProgressReporter;

/// 单会话消息同步与已读上报（由 [crate::application::SyncProtocolAdapter] 实现，供 IMClient 使用）。
/// 会话列表全量同步由同步引擎内部触发，不通过本 trait 暴露。
pub trait SessionSyncRunner: Send + Sync {
    fn request_message_sync(
        &self,
        conversation_id: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = crate::shared::error::Result<()>> + Send + '_>>;
    fn request_message_sync_from_seq(
        &self,
        conversation_id: &str,
        last_seq: u64,
        limit: i32,
    ) -> Pin<Box<dyn std::future::Future<Output = crate::shared::error::Result<()>> + Send + '_>>;
    fn send_read_ack(
        &self,
        conversation_id: &str,
        read_seq: u64,
    ) -> Pin<Box<dyn std::future::Future<Output = crate::shared::error::Result<()>> + Send + '_>>;
    fn request_participants_sync(
        &self,
        conversation_id: &str,
        limit: i32,
    ) -> Pin<
        Box<
            dyn std::future::Future<
                    Output = crate::shared::error::Result<
                        Vec<crate::model::ConversationParticipant>,
                    >,
                > + Send
                + '_,
        >,
    >;
}

static NEXT_SYNC_RUN_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncMode {
    Init,
    Background,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncFailurePolicy {
    /// 必要同步门禁：任务失败后本阶段失败，不再发布阶段完成事件。
    FailRun,
    /// 后台补偿/可选任务：记录失败并继续，不影响用户可见同步状态。
    Continue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncTrigger {
    InitialLogin,
    Reconnect,
    Manual,
    SeqGapRepair,
    CriticalEventReplay,
    ReadStateUpload,
    BackgroundMaintenance,
    RecoveryHint,
}

impl SyncTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InitialLogin => "InitialLogin",
            Self::Reconnect => "Reconnect",
            Self::Manual => "Manual",
            Self::SeqGapRepair => "SeqGapRepair",
            Self::CriticalEventReplay => "CriticalEventReplay",
            Self::ReadStateUpload => "ReadStateUpload",
            Self::BackgroundMaintenance => "BackgroundMaintenance",
            Self::RecoveryHint => "RecoveryHint",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncScope {
    Global,
    Conversations,
    SingleConversation,
    CriticalEvents,
    ReadStates,
}

impl SyncScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "Global",
            Self::Conversations => "Conversations",
            Self::SingleConversation => "SingleConversation",
            Self::CriticalEvents => "CriticalEvents",
            Self::ReadStates => "ReadStates",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncVisibility {
    UserVisible,
    Silent,
}

impl SyncVisibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserVisible => "UserVisible",
            Self::Silent => "Silent",
        }
    }

    pub fn is_user_visible(self) -> bool {
        matches!(self, Self::UserVisible)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncReason {
    Startup,
    ReconnectCatchUp,
    UserRequested,
    SequenceGap,
    CriticalEventCompensation,
    ReadStateFlush,
    BackgroundCatchUp,
    ServerRecoveryHint,
}

impl SyncReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "Startup",
            Self::ReconnectCatchUp => "ReconnectCatchUp",
            Self::UserRequested => "UserRequested",
            Self::SequenceGap => "SequenceGap",
            Self::CriticalEventCompensation => "CriticalEventCompensation",
            Self::ReadStateFlush => "ReadStateFlush",
            Self::BackgroundCatchUp => "BackgroundCatchUp",
            Self::ServerRecoveryHint => "ServerRecoveryHint",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncRunContext {
    pub run_id: String,
    pub trigger: SyncTrigger,
    pub scope: SyncScope,
    pub visibility: SyncVisibility,
    pub reason: SyncReason,
}

impl SyncRunContext {
    pub fn new(
        trigger: SyncTrigger,
        scope: SyncScope,
        visibility: SyncVisibility,
        reason: SyncReason,
    ) -> Self {
        let id = NEXT_SYNC_RUN_ID.fetch_add(1, Ordering::Relaxed);
        Self {
            run_id: format!("sync-{id}"),
            trigger,
            scope,
            visibility,
            reason,
        }
    }

    pub fn initial_login() -> Self {
        Self::new(
            SyncTrigger::InitialLogin,
            SyncScope::Global,
            SyncVisibility::UserVisible,
            SyncReason::Startup,
        )
    }

    pub fn reconnect() -> Self {
        Self::new(
            SyncTrigger::Reconnect,
            SyncScope::Global,
            SyncVisibility::UserVisible,
            SyncReason::ReconnectCatchUp,
        )
    }

    pub fn manual_single_conversation() -> Self {
        Self::new(
            SyncTrigger::Manual,
            SyncScope::SingleConversation,
            SyncVisibility::UserVisible,
            SyncReason::UserRequested,
        )
    }

    pub fn silent_gap_repair() -> Self {
        Self::new(
            SyncTrigger::SeqGapRepair,
            SyncScope::SingleConversation,
            SyncVisibility::Silent,
            SyncReason::SequenceGap,
        )
    }

    pub fn silent_background(scope: SyncScope, reason: SyncReason) -> Self {
        Self::new(
            SyncTrigger::BackgroundMaintenance,
            scope,
            SyncVisibility::Silent,
            reason,
        )
    }

    pub fn for_background_phase(&self) -> Self {
        Self {
            run_id: self.run_id.clone(),
            trigger: SyncTrigger::BackgroundMaintenance,
            scope: self.scope,
            visibility: SyncVisibility::Silent,
            reason: SyncReason::BackgroundCatchUp,
        }
    }

    /// 多端私有数据补偿（如通讯录备注）：静默、不阻塞 UI。
    pub fn silent_multidevice_private_data() -> Self {
        Self::silent_background(SyncScope::Global, SyncReason::CriticalEventCompensation)
    }
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
    pub run: SyncRunContext,
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

    pub async fn load_checkpoint(&self) -> crate::shared::error::Result<Option<SyncCheckpoint>> {
        let Some(ref store) = self.checkpoint_store else {
            return Ok(None);
        };
        let cp = store.load(&self.task_id).await?;
        Ok(if cp.cursor.is_some() { Some(cp) } else { None })
    }

    pub async fn save_checkpoint(
        &self,
        cursor: Option<impl AsRef<str>>,
    ) -> crate::shared::error::Result<()> {
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
    fn failure_policy(&self) -> SyncFailurePolicy {
        match self.mode() {
            SyncMode::Init => SyncFailurePolicy::FailRun,
            SyncMode::Background => SyncFailurePolicy::Continue,
        }
    }
    fn weight(&self) -> u32 {
        1
    }
    fn execute(
        &self,
        ctx: SyncContext,
    ) -> Pin<Box<dyn std::future::Future<Output = SyncResult<SyncTaskResult>> + Send>>;
}

#[cfg(test)]
mod tests {
    use super::{SyncReason, SyncRunContext, SyncScope, SyncTrigger, SyncVisibility};

    #[test]
    fn initial_login_sync_is_user_visible_global_startup() {
        let run = SyncRunContext::initial_login();

        assert_eq!(run.trigger, SyncTrigger::InitialLogin);
        assert_eq!(run.scope, SyncScope::Global);
        assert_eq!(run.visibility, SyncVisibility::UserVisible);
        assert_eq!(run.reason, SyncReason::Startup);
        assert!(run.visibility.is_user_visible());
    }

    #[test]
    fn reconnect_sync_is_user_visible_catch_up() {
        let run = SyncRunContext::reconnect();

        assert_eq!(run.trigger, SyncTrigger::Reconnect);
        assert_eq!(run.visibility, SyncVisibility::UserVisible);
        assert_eq!(run.reason, SyncReason::ReconnectCatchUp);
    }

    #[test]
    fn seq_gap_repair_is_silent_single_conversation_sync() {
        let run = SyncRunContext::silent_gap_repair();

        assert_eq!(run.trigger, SyncTrigger::SeqGapRepair);
        assert_eq!(run.scope, SyncScope::SingleConversation);
        assert_eq!(run.visibility, SyncVisibility::Silent);
        assert_eq!(run.reason, SyncReason::SequenceGap);
    }

    #[test]
    fn background_phase_keeps_run_id_but_becomes_silent() {
        let run = SyncRunContext::initial_login();
        let background = run.for_background_phase();

        assert_eq!(background.run_id, run.run_id);
        assert_eq!(background.trigger, SyncTrigger::BackgroundMaintenance);
        assert_eq!(background.visibility, SyncVisibility::Silent);
        assert_eq!(background.reason, SyncReason::BackgroundCatchUp);
    }
}
