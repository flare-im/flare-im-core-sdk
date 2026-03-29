//! 同步编排：按 Init / Background 分类，**并行**执行同阶段任务，发 SyncFinished。

use std::sync::Arc;

use crate::event::{EventBus, SdkEvent, SyncNotify, SyncPhase};
use crate::store::StoreProvider;

use super::checkpoint::CheckpointStore;
use super::progress::{EventBusProgressReporter, SyncProgressReporter};
use super::task::{SyncContext, SyncMode, SyncTask};

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

    /// 执行全部任务：Init 并行 → SyncFinished(Init) → Background 并行 → SyncFinished(Background)。
    pub fn run(&self, user_id: &str, tasks: Vec<Arc<dyn SyncTask>>) {
        let (init_tasks, bg_tasks): (Vec<_>, Vec<_>) =
            tasks.into_iter().partition(|t| t.mode() == SyncMode::Init);
        let total_weight: u32 = init_tasks.iter().map(|t| t.weight()).sum::<u32>()
            + bg_tasks.iter().map(|t| t.weight()).sum::<u32>();
        let progress_reporter: Arc<dyn SyncProgressReporter> = Arc::new(
            EventBusProgressReporter::new(self.bus.clone(), total_weight),
        );

        let bus = self.bus.clone();
        let store = self.store.clone();
        let checkpoint_store = self.checkpoint_store.clone();
        let user_id = user_id.to_string();

        tokio::spawn(async move {
            bus.publish(SdkEvent::Sync(SyncNotify::Started));

            run_phase(
                &bus,
                &user_id,
                &store,
                &checkpoint_store,
                progress_reporter.clone(),
                init_tasks,
            )
            .await;

            bus.publish(SdkEvent::Sync(SyncNotify::Finished {
                phase: SyncPhase::Init,
            }));

            run_phase(
                &bus,
                &user_id,
                &store,
                &checkpoint_store,
                progress_reporter,
                bg_tasks,
            )
            .await;

            bus.publish(SdkEvent::Sync(SyncNotify::Finished {
                phase: SyncPhase::Background,
            }));
        });
    }
}

async fn run_phase(
    bus: &EventBus,
    user_id: &str,
    store: &StoreProvider,
    checkpoint_store: &Arc<CheckpointStore>,
    progress_reporter: Arc<dyn SyncProgressReporter>,
    tasks: Vec<Arc<dyn SyncTask>>,
) {
    let handles: Vec<_> = tasks
        .into_iter()
        .map(|task| {
            let task_id = task.id().to_string();
            let weight = task.weight();
            let progress = progress_reporter.clone();
            progress.report_current(task_id.clone());
            let ctx = SyncContext {
                user_id: user_id.to_string(),
                task_id: task_id.clone(),
                store: store.clone(),
                progress: Some(progress.clone()),
                checkpoint_store: Some(checkpoint_store.clone()),
            };
            let bus = bus.clone();
            tokio::spawn(async move {
                match task.execute(ctx.clone()).await {
                    Ok(res) => {
                        if let Some(cursor) = res.cursor {
                            let _ = ctx.save_checkpoint(Some(cursor)).await;
                        }
                        progress.report_weight_completed(&task_id, weight, "done".into());
                        bus.publish(SdkEvent::Sync(SyncNotify::TaskCompleted { task: task_id }));
                    }
                    Err(e) => {
                        bus.publish(SdkEvent::Sync(SyncNotify::Failed {
                            task: task_id,
                            message: format!("{}", e),
                        }));
                    }
                }
            })
        })
        .collect();
    for h in handles {
        let _ = h.await;
    }
}
