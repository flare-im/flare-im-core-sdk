//! 同步编排：按 Init / Background 分类，**并行**执行同阶段任务，发 SyncFinished。

use std::sync::Arc;

use crate::infrastructure::persistence::StoreProvider;
use crate::kernel::event::{EventBus, ReadinessStage, SdkEvent, SyncNotify, SyncPhase};
use crate::shared::util::{BackgroundTask, now_millis, spawn_background_task};
use tokio::task::JoinSet;
use tracing::{debug, warn};

use super::checkpoint::CheckpointStore;
use super::progress::{EventBusProgressReporter, SyncProgressReporter};
use super::task::{
    SharedConversationsSnapshot, SyncContext, SyncFailurePolicy, SyncMode, SyncRunContext, SyncTask,
};

pub struct Orchestrator {
    store: StoreProvider,
    bus: EventBus,
    checkpoint_store: Arc<CheckpointStore>,
}

impl Orchestrator {
    pub fn new(store: StoreProvider, bus: EventBus) -> Self {
        let checkpoint_store = Arc::new(CheckpointStore::new(store.cursors.clone()));
        Self {
            store,
            bus,
            checkpoint_store,
        }
    }

    /// 执行全部任务：Init 并行 → SyncFinished(Init) → Background 静默并行 → SyncFinished(Background)。
    pub fn run(
        &self,
        user_id: &str,
        run: SyncRunContext,
        tasks: Vec<Arc<dyn SyncTask>>,
    ) -> BackgroundTask {
        let (init_tasks, bg_tasks): (Vec<_>, Vec<_>) =
            tasks.into_iter().partition(|t| t.mode() == SyncMode::Init);
        let init_weight: u32 = init_tasks.iter().map(|t| t.weight()).sum();
        let bg_weight: u32 = bg_tasks.iter().map(|t| t.weight()).sum();
        let init_progress_reporter: Arc<dyn SyncProgressReporter> = Arc::new(
            EventBusProgressReporter::new(self.bus.clone(), run.clone(), init_weight),
        );
        let bg_run = run.for_background_phase();
        let bg_progress_reporter: Arc<dyn SyncProgressReporter> = Arc::new(
            EventBusProgressReporter::new(self.bus.clone(), bg_run.clone(), bg_weight),
        );

        let bus = self.bus.clone();
        let store = self.store.clone();
        let checkpoint_store = self.checkpoint_store.clone();
        let user_id = user_id.to_string();

        spawn_background_task(async move {
            bus.publish(SdkEvent::Sync(SyncNotify::Started { run: run.clone() }));

            let init_outcome = run_phase(
                &bus,
                &user_id,
                run.clone(),
                &store,
                &checkpoint_store,
                init_progress_reporter,
                init_tasks,
            )
            .await;

            if init_outcome.failed_required {
                warn!(
                    run_id = %run.run_id,
                    failed_tasks = ?init_outcome.failed_tasks,
                    "required init sync failed; skip Init finished and background phase"
                );
                bus.publish(SdkEvent::Sync(SyncNotify::Failed {
                    run: run.clone(),
                    task: "init_phase".to_string(),
                    message: format!(
                        "required init sync failed: {}",
                        init_outcome.failed_tasks.join(", ")
                    ),
                }));
                return;
            }

            bus.publish(SdkEvent::Sync(SyncNotify::Finished {
                run: run.clone(),
                phase: SyncPhase::Init,
            }));
            // T1 前台新鲜度达成：首屏会话快照已与服务端对齐。
            bus.publish(SdkEvent::Sync(SyncNotify::Readiness {
                run: run.clone(),
                stage: ReadinessStage::ForegroundFresh,
            }));

            bus.publish(SdkEvent::Sync(SyncNotify::Started {
                run: bg_run.clone(),
            }));
            let _bg_outcome = run_phase(
                &bus,
                &user_id,
                bg_run.clone(),
                &store,
                &checkpoint_store,
                bg_progress_reporter,
                bg_tasks,
            )
            .await;

            bus.publish(SdkEvent::Sync(SyncNotify::Finished {
                run: bg_run.clone(),
                phase: SyncPhase::Background,
            }));
            // T2 环境收敛完成：长尾会话 / 已读 / 关键事件等已补齐。
            bus.publish(SdkEvent::Sync(SyncNotify::Readiness {
                run: bg_run,
                stage: ReadinessStage::Converged,
            }));
        })
    }

    /// 静默执行指定 Background 任务（不中断登录/重连全量 sync）。
    pub fn spawn_background_tasks(
        &self,
        user_id: String,
        run: SyncRunContext,
        tasks: Vec<Arc<dyn SyncTask>>,
    ) -> BackgroundTask {
        let store = self.store.clone();
        let bus = self.bus.clone();
        let checkpoint_store = self.checkpoint_store.clone();
        let weight: u32 = tasks.iter().map(|task| task.weight()).sum();
        spawn_background_task(async move {
            let progress_reporter: Arc<dyn SyncProgressReporter> = Arc::new(
                EventBusProgressReporter::new(bus.clone(), run.clone(), weight),
            );
            let _ = run_phase(
                &bus,
                &user_id,
                run,
                &store,
                &checkpoint_store,
                progress_reporter,
                tasks,
            )
            .await;
        })
    }
}

async fn run_phase(
    bus: &EventBus,
    user_id: &str,
    run: SyncRunContext,
    store: &StoreProvider,
    checkpoint_store: &Arc<CheckpointStore>,
    progress_reporter: Arc<dyn SyncProgressReporter>,
    tasks: Vec<Arc<dyn SyncTask>>,
) -> PhaseOutcome {
    let mut join_set = JoinSet::new();
    // 同 phase 各任务共享同一份会话列表快照（首个用到的任务触发一次查询）。
    let shared_conversations = SharedConversationsSnapshot::default();
    for task in tasks {
        let task_id = task.id().to_string();
        let weight = task.weight();
        let mode = task.mode();
        let failure_policy = task.failure_policy();
        let progress = progress_reporter.clone();
        progress.report_current(task_id.clone());
        let ctx = SyncContext {
            user_id: user_id.to_string(),
            task_id: task_id.clone(),
            run: run.clone(),
            store: store.clone(),
            progress: Some(progress.clone()),
            checkpoint_store: Some(checkpoint_store.clone()),
            conversations: shared_conversations.clone(),
        };
        let bus = bus.clone();
        let run = run.clone();
        join_set.spawn(async move {
            let started_ms = now_millis();
            debug!(task = %task_id, mode = ?mode, weight = weight, "sync task started");
            match task.execute(ctx.clone()).await {
                Ok(res) => {
                    if let Some(cursor) = res.cursor {
                        let _ = ctx.save_checkpoint(Some(cursor)).await;
                    }
                    progress.report_weight_completed(&task_id, weight, "done".into());
                    debug!(
                        task = %task_id,
                        mode = ?mode,
                        elapsed_ms = now_millis().saturating_sub(started_ms),
                        "sync task completed"
                    );
                    bus.publish(SdkEvent::Sync(SyncNotify::TaskCompleted {
                        run,
                        task: task_id,
                    }));
                }
                Err(e) => {
                    warn!(
                        task = %task_id,
                        mode = ?mode,
                        elapsed_ms = now_millis().saturating_sub(started_ms),
                        error = %e,
                        "sync task failed"
                    );
                    bus.publish(SdkEvent::Sync(SyncNotify::Failed {
                        run,
                        task: task_id.clone(),
                        message: format!("{}", e),
                    }));
                    if failure_policy == SyncFailurePolicy::FailRun {
                        return Some(task_id);
                    }
                }
            }
            None
        });
    }
    debug!(
        task_count = join_set.len(),
        "sync phase waiting for all tasks"
    );
    let mut failed_required = false;
    let mut failed_tasks = Vec::new();
    while let Some(res) = join_set.join_next().await {
        match res {
            Ok(Some(task_id)) => {
                failed_required = true;
                failed_tasks.push(task_id);
            }
            Ok(None) => {}
            Err(e) => {
                failed_required = true;
                failed_tasks.push("join_error".to_string());
                warn!(error = %e, "sync task join error");
            }
        }
    }
    PhaseOutcome {
        failed_required,
        failed_tasks,
    }
}

#[derive(Debug, Default)]
struct PhaseOutcome {
    failed_required: bool,
    failed_tasks: Vec<String>,
}
