use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::oneshot;
use tracing::{info, warn};

use crate::core::lifecycle::{SdkState, StateManager};
use crate::error::{SdkError, Result};
use crate::event::{EventBus, SdkEvent};
use crate::protocol::PacketSender;
use crate::store::StoreProvider;
use super::conversation_sync::ConversationSync;
use super::message_sync::MessageSync;

// ── SyncTask trait + 完成模式 ────────────────────────────────

/// 同步任务完成模式
///
/// - `Done` — 同步完成，`execute()` 返回时所有工作已完成
/// - `Pending` — 异步完成，返回 oneshot::Receiver，SyncEngine 等待信号
///
/// # 示例（同步完成）
///
/// ```ignore
/// async fn execute(&self, ctx: &SyncContext) -> Result<SyncCompletion> {
///     let data = ctx.sender.sync_contacts().await?;
///     ctx.stores.save_contacts(&data).await?;
///     Ok(SyncCompletion::Done)   // 所有工作已完成
/// }
/// ```
///
/// # 示例（异步完成）
///
/// ```ignore
/// async fn execute(&self, ctx: &SyncContext) -> Result<SyncCompletion> {
///     let (tx, rx) = oneshot::channel();
///     let sender = ctx.sender.clone();
///     tokio::spawn(async move {
///         let result = heavy_sync(&sender).await;
///         let _ = tx.send(result);
///     });
///     Ok(SyncCompletion::Pending { completion: rx })
/// }
/// ```
pub enum SyncCompletion {
    /// 同步完成 — execute() 返回即代表任务完成
    Done,
    /// 异步完成 — 后台任务执行中，通过 receiver 通知完成
    Pending { completion: oneshot::Receiver<Result<()>> },
}

/// 同步执行模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    /// 必须在 bootstrap 完成前同步完成（阻塞 Ready 状态）
    Required,
    /// 后台异步执行，不阻塞 bootstrap
    Background,
}

/// 同步阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncPhase {
    /// 会话列表同步完成后、消息同步之前
    AfterConversations,
    /// 全部内置同步完成后
    AfterSync,
}

/// 同步上下文 — 传给每个 SyncTask
pub struct SyncContext {
    pub sender: Arc<PacketSender>,
    pub stores: Arc<StoreProvider>,
    pub bus: EventBus,
}

/// 自定义同步任务 trait
///
/// 用户实现此 trait 并通过 `IMClientBuilder::add_sync_task()` 注册。
/// SDK 在 bootstrap 流程中根据 `phase()` + `mode()` 自动编排。
///
/// # 同步完成 vs 异步完成
///
/// - 返回 `SyncCompletion::Done` → 视为同步完成
/// - 返回 `SyncCompletion::Pending` → SDK 等待 oneshot receiver
///
/// 对于 `Required` 任务，无论哪种模式 SDK 都会等待完成后才继续。
/// 对于 `Background` 任务，SDK spawn 后不等待。
#[async_trait]
pub trait SyncTask: Send + Sync {
    fn name(&self) -> &str;
    fn mode(&self) -> SyncMode;
    fn phase(&self) -> SyncPhase;
    async fn execute(&self, ctx: &SyncContext) -> Result<SyncCompletion>;
}

// ── SyncManager ──────────────────────────────────────────────

/// 同步管理器 — 编排内置同步 + 用户注册的自定义同步任务
///
/// # Bootstrap 流程
///
/// ```text
/// bootstrap() 开始
/// ├── ConversationSync: 全量拉取会话 → store + emit
/// ├── [Phase: AfterConversations]
/// │   ├── Required tasks → 并发执行 + 等待完成（同步/异步均等待）
/// │   └── Background tasks → spawn（不等待）
/// ├── MessageSync: 拉取每个会话消息 → store
/// ├── [Phase: AfterSync]
/// │   ├── Required tasks → 并发执行 + 等待完成
/// │   └── Background tasks → spawn
/// └── 状态迁移 → Ready
/// ```
pub struct SyncManager {
    sender: Arc<PacketSender>,
    stores: Arc<StoreProvider>,
    state: Arc<StateManager>,
    bus: EventBus,
    tasks: Vec<Arc<dyn SyncTask>>,
}

impl SyncManager {
    pub fn new(
        sender: Arc<PacketSender>,
        stores: Arc<StoreProvider>,
        state: Arc<StateManager>,
        bus: EventBus,
    ) -> Self {
        Self { sender, stores, state, bus, tasks: Vec::new() }
    }

    /// 注册自定义同步任务
    pub fn register_task(&mut self, task: impl SyncTask + 'static) {
        self.tasks.push(Arc::new(task));
    }

    pub fn register_task_arc(&mut self, task: Arc<dyn SyncTask>) {
        self.tasks.push(task);
    }

    /// 执行 Bootstrap 全量同步
    pub async fn bootstrap(&self) -> Result<()> {
        if !self.state.transition(SdkState::Connected, SdkState::Syncing) {
            let cur = self.state.get();
            if cur == SdkState::Ready { return Ok(()); }
            return Err(SdkError::InvalidState { expected: "Connected", actual: cur.to_string() });
        }
        self.bus.publish(SdkEvent::StateChanged { state: SdkState::Syncing });

        if !self.tasks.is_empty() {
            info!(
                tasks = ?self.tasks.iter().map(|t| (t.name(), t.mode(), t.phase())).collect::<Vec<_>>(),
                "bootstrap with registered sync tasks"
            );
        }

        match self.do_bootstrap().await {
            Ok(()) => {
                self.state.set(SdkState::Ready);
                self.bus.publish(SdkEvent::StateChanged { state: SdkState::Ready });
                info!("bootstrap completed");
                Ok(())
            }
            Err(e) => {
                warn!(error = %e, "bootstrap failed, reverting to Connected");
                self.state.set(SdkState::Connected);
                self.bus.publish(SdkEvent::StateChanged { state: SdkState::Connected });
                Err(e)
            }
        }
    }

    async fn do_bootstrap(&self) -> Result<()> {
        let ctx = Arc::new(SyncContext {
            sender: self.sender.clone(),
            stores: self.stores.clone(),
            bus: self.bus.clone(),
        });

        // Step 1: 全量拉取会话
        ConversationSync::sync_all(&self.sender, &self.stores, &self.bus).await?;

        // Step 2: AfterConversations 阶段任务
        self.run_phase(SyncPhase::AfterConversations, &ctx).await?;

        // Step 3: 拉取每个会话的消息
        MessageSync::sync_all(&self.sender, &self.stores).await?;

        // Step 4: AfterSync 阶段任务
        self.run_phase(SyncPhase::AfterSync, &ctx).await?;

        Ok(())
    }

    async fn run_phase(&self, phase: SyncPhase, ctx: &Arc<SyncContext>) -> Result<()> {
        let required: Vec<_> = self.tasks.iter()
            .filter(|t| t.phase() == phase && t.mode() == SyncMode::Required)
            .cloned().collect();

        let background: Vec<_> = self.tasks.iter()
            .filter(|t| t.phase() == phase && t.mode() == SyncMode::Background)
            .cloned().collect();

        // ── Required: 并发执行 + 全部等待 ────────────────────
        if !required.is_empty() {
            info!(phase = ?phase, count = required.len(), "running required sync tasks");
            let mut set = tokio::task::JoinSet::new();
            for task in &required {
                let task = Arc::clone(task);
                let ctx = Arc::clone(ctx);
                set.spawn(async move {
                    let name = task.name().to_string();
                    match task.execute(&ctx).await {
                        Ok(SyncCompletion::Done) => Ok(name),
                        Ok(SyncCompletion::Pending { completion }) => {
                            // 异步完成: 等待 oneshot receiver
                            match completion.await {
                                Ok(Ok(())) => Ok(name),
                                Ok(Err(e)) => Err((name, e)),
                                Err(_) => Err((name, SdkError::SyncFailed("task dropped sender".into()))),
                            }
                        }
                        Err(e) => Err((name, e)),
                    }
                });
            }

            let mut first_error: Option<SdkError> = None;
            while let Some(result) = set.join_next().await {
                match result {
                    Ok(Ok(name)) => {
                        info!(task = %name, "required sync task completed");
                        self.bus.publish(SdkEvent::SyncTaskCompleted { task: name });
                    }
                    Ok(Err((name, e))) => {
                        warn!(task = %name, error = %e, "required sync task failed");
                        self.bus.publish(SdkEvent::SyncTaskFailed { task: name, error: e.to_string() });
                        if first_error.is_none() {
                            first_error = Some(e);
                        }
                    }
                    Err(join_err) => {
                        warn!(error = %join_err, "sync task panicked");
                    }
                }
            }
            if let Some(e) = first_error {
                return Err(e);
            }
        }

        // ── Background: spawn, 不等待 ───────────────────────
        if !background.is_empty() {
            info!(phase = ?phase, count = background.len(), "spawning background sync tasks");
            for task in background {
                let task = Arc::clone(&task);
                let ctx = Arc::clone(ctx);
                let bus = self.bus.clone();
                tokio::spawn(async move {
                    let name = task.name().to_string();
                    match task.execute(&ctx).await {
                        Ok(SyncCompletion::Done) => {
                            info!(task = %name, "background sync task completed");
                            bus.publish(SdkEvent::SyncTaskCompleted { task: name });
                        }
                        Ok(SyncCompletion::Pending { completion }) => {
                            match completion.await {
                                Ok(Ok(())) => {
                                    info!(task = %name, "background async sync task completed");
                                    bus.publish(SdkEvent::SyncTaskCompleted { task: name });
                                }
                                Ok(Err(e)) => {
                                    warn!(task = %name, error = %e, "background async sync task failed");
                                    bus.publish(SdkEvent::SyncTaskFailed { task: name, error: e.to_string() });
                                }
                                Err(_) => {
                                    warn!(task = %name, "background async sync task sender dropped");
                                }
                            }
                        }
                        Err(e) => {
                            warn!(task = %name, error = %e, "background sync task failed");
                            bus.publish(SdkEvent::SyncTaskFailed { task: name, error: e.to_string() });
                        }
                    }
                });
            }
        }

        Ok(())
    }

    /// 重新全量同步会话列表
    pub async fn sync_conversations(&self) -> Result<()> {
        ConversationSync::sync_all(&self.sender, &self.stores, &self.bus).await
    }

    /// 增量同步单个会话
    pub async fn sync_conversation(&self, conversation_id: &str) -> Result<()> {
        MessageSync::sync_one(&self.sender, &self.stores, conversation_id).await
    }
}
