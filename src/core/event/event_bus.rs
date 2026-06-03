//! 内部事件总线 + 类型化回调 API
//!
//! 流程：内部发布 SdkEvent → broadcast 通道 → 异步分发任务 → 按事件类型调用已注册的回调。
//! 不暴露大 trait，仅暴露 `on_*` 类型化注册，便于跨语言绑定（Swift / Kotlin / TypeScript）。

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use flare_proto::common::{CallSignalEvent, MessageRecallEvent, SendAck, TypingEvent};

use crate::model::IMMessage;
use tokio::sync::broadcast;

use tracing::warn;

use super::types::{
    ConnectionEvent, ConversationEvent, MessageEvent, NotificationEvent, SdkEvent, SyncNotify,
    SyncPhase,
};
use crate::core::SdkState;
use crate::core::SyncState;
use crate::extension::middleware::MiddlewareChain;

/// 事件通道容量。保持有界，避免宿主侧消费慢时无限占用内存；同步高峰下也要尽量减少 Lagged。
const BUS_CAPACITY: usize = 2048;
const REPLAY_DELAY_MS: u64 = 10;

/// 启动 EventBus 分发循环：在已有 Tokio runtime 内 `spawn`；否则在独立线程上创建 runtime（Tauri/FFI 同步初始化）。
fn spawn_event_dispatch_loop(fut: impl Future<Output = ()> + Send + 'static) {
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::spawn(fut);
        return;
    }
    if let Err(error) = std::thread::Builder::new()
        .name("flare-sdk-event-bus".into())
        .spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                warn!("failed to create flare-sdk EventBus runtime");
                return;
            };
            rt.block_on(fut);
        })
    {
        warn!(%error, "failed to spawn flare-sdk-event-bus thread");
    }
}

fn spawn_callback(f: impl FnOnce() + Send + 'static) {
    let f = move || {
        if catch_unwind(AssertUnwindSafe(f)).is_err() {
            warn!("EventBus callback panicked; continuing");
        }
    };

    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::spawn_blocking(f);
        return;
    }
    if let Err(error) = std::thread::Builder::new()
        .name("flare-sdk-event-callback".into())
        .spawn(f)
    {
        warn!(%error, "failed to spawn flare-sdk-event-callback thread");
    }
}

fn replay_after_dispatch_window(f: impl FnOnce() + Send + 'static) {
    spawn_callback(move || {
        std::thread::sleep(std::time::Duration::from_millis(REPLAY_DELAY_MS));
        f();
    });
}

trait RecoverableRwLock<T> {
    fn safe_read(&self, name: &'static str) -> RwLockReadGuard<'_, T>;
    fn safe_write(&self, name: &'static str) -> RwLockWriteGuard<'_, T>;
}

impl<T> RecoverableRwLock<T> for RwLock<T> {
    fn safe_read(&self, name: &'static str) -> RwLockReadGuard<'_, T> {
        match self.read() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!(
                    lock = name,
                    "EventBus lock poisoned; recovering read access"
                );
                poisoned.into_inner()
            }
        }
    }

    fn safe_write(&self, name: &'static str) -> RwLockWriteGuard<'_, T> {
        match self.write() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!(
                    lock = name,
                    "EventBus lock poisoned; recovering write access"
                );
                poisoned.into_inner()
            }
        }
    }
}

fn callback_snapshot<T: Clone>(callbacks: &Arc<RwLock<Vec<T>>>) -> Vec<T> {
    callbacks.safe_read("event_bus").clone()
}

fn dispatch_callbacks<T, F>(callbacks: &Arc<RwLock<Vec<T>>>, invoke: F)
where
    T: Clone + Send + 'static,
    F: Fn(&T) + Send + 'static,
{
    let callbacks = callback_snapshot(callbacks);
    if callbacks.is_empty() {
        return;
    }

    spawn_callback(move || {
        for callback in callbacks {
            invoke(&callback);
        }
    });
}

fn dispatch_callbacks_with<T, P, M, F>(callbacks: &Arc<RwLock<Vec<T>>>, make_payload: M, invoke: F)
where
    T: Clone + Send + 'static,
    P: Send + 'static,
    M: FnOnce() -> P,
    F: Fn(&T, &P) + Send + 'static,
{
    let callbacks = callback_snapshot(callbacks);
    if callbacks.is_empty() {
        return;
    }

    let payload = make_payload();
    spawn_callback(move || {
        for callback in callbacks {
            invoke(&callback, &payload);
        }
    });
}

fn dispatch_any_callbacks(callbacks: &Arc<RwLock<Vec<FnAny>>>, event: SdkEvent) {
    let callbacks = callback_snapshot(callbacks);
    if callbacks.is_empty() {
        return;
    }

    let event = Arc::new(event);
    spawn_callback(move || {
        for callback in callbacks {
            callback(Arc::clone(&event));
        }
    });
}

fn same_arc<T: ?Sized>(left: &Arc<T>, right: &Arc<T>) -> bool {
    Arc::ptr_eq(left, right)
}

// 回调存储：Arc 便于 clone 后传入 spawn_blocking，不阻塞分发循环
type FnConnected = Arc<dyn Fn() + Send + Sync>;
type FnDisconnected = Arc<dyn Fn(String) + Send + Sync>;
type FnStateChanged = Arc<dyn Fn(SdkState) + Send + Sync>;
type FnSyncStateChanged = Arc<dyn Fn(SyncState) + Send + Sync>;
type FnServerError = Arc<dyn Fn(i32, String) + Send + Sync>;
type FnKickedOff = Arc<dyn Fn(String) + Send + Sync>;
type FnTokenExpired = Arc<dyn Fn(String) + Send + Sync>;
type FnMessage = Arc<dyn Fn(IMMessage) + Send + Sync>;
type FnMessageBatch = Arc<dyn Fn(Vec<IMMessage>) + Send + Sync>;
type FnSendAck = Arc<dyn Fn(SendAck) + Send + Sync>;
type FnSendFailed = Arc<dyn Fn(String, String) + Send + Sync>;
type FnRecalled = Arc<dyn Fn(String, MessageRecallEvent) + Send + Sync>;
type FnTyping = Arc<dyn Fn(String, TypingEvent) + Send + Sync>;
type FnCallSignal = Arc<dyn Fn(String, CallSignalEvent) + Send + Sync>;
type FnConversationIds = Arc<dyn Fn(Vec<String>) + Send + Sync>;
type FnConversationId = Arc<dyn Fn(String) + Send + Sync>;
type FnConversationUnreadCountChanged = Arc<dyn Fn(String, u32) + Send + Sync>;
type FnExtension = Arc<dyn Fn(String, String, Vec<u8>) + Send + Sync>;
type FnNotification = Arc<dyn Fn(IMMessage) + Send + Sync>;
type FnSyncPhase = Arc<dyn Fn(SyncPhase) + Send + Sync>;
type FnSyncProgress = Arc<dyn Fn(String, f32, String) + Send + Sync>;
type FnAny = Arc<dyn Fn(Arc<SdkEvent>) + Send + Sync>;

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<SdkEvent>,
    middleware: Arc<MiddlewareChain>,
    last_connection_state: Arc<RwLock<Option<SdkState>>>,
    last_sync_state: Arc<RwLock<Option<SyncState>>>,
    last_sync_finished: Arc<RwLock<Option<SyncPhase>>>,
    // Connection（std::sync::RwLock 支持同步注册，分发时在 spawn_blocking 内读）
    on_connected: Arc<RwLock<Vec<FnConnected>>>,
    on_disconnected: Arc<RwLock<Vec<FnDisconnected>>>,
    on_state_changed: Arc<RwLock<Vec<FnStateChanged>>>,
    on_sync_state_changed: Arc<RwLock<Vec<FnSyncStateChanged>>>,
    on_server_error: Arc<RwLock<Vec<FnServerError>>>,
    on_kicked_off: Arc<RwLock<Vec<FnKickedOff>>>,
    on_token_expired: Arc<RwLock<Vec<FnTokenExpired>>>,
    // Message
    on_message: Arc<RwLock<Vec<FnMessage>>>,
    on_message_batch: Arc<RwLock<Vec<FnMessageBatch>>>,
    on_send_ack: Arc<RwLock<Vec<FnSendAck>>>,
    on_send_failed: Arc<RwLock<Vec<FnSendFailed>>>,
    on_recalled: Arc<RwLock<Vec<FnRecalled>>>,
    on_typing: Arc<RwLock<Vec<FnTyping>>>,
    call_signal_listeners: Arc<RwLock<Vec<FnCallSignal>>>,
    // Conversation
    on_conversation_synced: Arc<RwLock<Vec<FnConversationIds>>>,
    on_conversation_created: Arc<RwLock<Vec<FnConversationId>>>,
    on_conversation_updated: Arc<RwLock<Vec<FnConversationId>>>,
    on_conversation_unread_count_changed: Arc<RwLock<Vec<FnConversationUnreadCountChanged>>>,
    on_conversation_deleted: Arc<RwLock<Vec<FnConversationId>>>,
    // Extension
    on_extension: Arc<RwLock<Vec<FnExtension>>>,
    on_notification: Arc<RwLock<Vec<FnNotification>>>,
    // Sync
    on_sync_started: Arc<RwLock<Vec<FnConnected>>>,
    on_sync_finished: Arc<RwLock<Vec<FnSyncPhase>>>,
    on_sync_failed: Arc<RwLock<Vec<FnSendFailed>>>,
    on_sync_progress: Arc<RwLock<Vec<FnSyncProgress>>>,
    on_sync_task_completed: Arc<RwLock<Vec<FnConversationId>>>,
    // Any
    on_any: Arc<RwLock<Vec<FnAny>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::with_middleware(Arc::new(MiddlewareChain::new()))
    }

    pub fn with_middleware(middleware: Arc<MiddlewareChain>) -> Self {
        let (tx, mut rx) = broadcast::channel(BUS_CAPACITY);
        let last_connection_state = Arc::new(RwLock::new(None));
        let last_sync_state = Arc::new(RwLock::new(None));
        let last_sync_finished = Arc::new(RwLock::new(None));

        let on_connected = Arc::new(RwLock::new(Vec::<FnConnected>::new()));
        let on_disconnected = Arc::new(RwLock::new(Vec::new()));
        let on_state_changed = Arc::new(RwLock::new(Vec::new()));
        let on_sync_state_changed = Arc::new(RwLock::new(Vec::new()));
        let on_server_error = Arc::new(RwLock::new(Vec::new()));
        let on_kicked_off = Arc::new(RwLock::new(Vec::new()));
        let on_token_expired = Arc::new(RwLock::new(Vec::new()));
        let on_message = Arc::new(RwLock::new(Vec::new()));
        let on_message_batch = Arc::new(RwLock::new(Vec::new()));
        let on_send_ack = Arc::new(RwLock::new(Vec::new()));
        let on_send_failed = Arc::new(RwLock::new(Vec::new()));
        let on_recalled = Arc::new(RwLock::new(Vec::new()));
        let on_typing = Arc::new(RwLock::new(Vec::new()));
        let call_signal_listeners = Arc::new(RwLock::new(Vec::new()));
        let on_conversation_synced = Arc::new(RwLock::new(Vec::new()));
        let on_conversation_created = Arc::new(RwLock::new(Vec::new()));
        let on_conversation_updated = Arc::new(RwLock::new(Vec::new()));
        let on_conversation_unread_count_changed = Arc::new(RwLock::new(Vec::new()));
        let on_conversation_deleted = Arc::new(RwLock::new(Vec::new()));
        let on_extension = Arc::new(RwLock::new(Vec::new()));
        let on_notification = Arc::new(RwLock::new(Vec::new()));
        let on_sync_started = Arc::new(RwLock::new(Vec::new()));
        let on_sync_finished = Arc::new(RwLock::new(Vec::new()));
        let on_sync_failed = Arc::new(RwLock::new(Vec::new()));
        let on_sync_progress = Arc::new(RwLock::new(Vec::new()));
        let on_sync_task_completed = Arc::new(RwLock::new(Vec::new()));
        let on_any = Arc::new(RwLock::new(Vec::new()));

        let bus = Self {
            tx: tx.clone(),
            middleware,
            last_connection_state: last_connection_state.clone(),
            last_sync_state: last_sync_state.clone(),
            last_sync_finished: last_sync_finished.clone(),
            on_connected: on_connected.clone(),
            on_disconnected: on_disconnected.clone(),
            on_state_changed: on_state_changed.clone(),
            on_sync_state_changed: on_sync_state_changed.clone(),
            on_server_error: on_server_error.clone(),
            on_kicked_off: on_kicked_off.clone(),
            on_token_expired: on_token_expired.clone(),
            on_message: on_message.clone(),
            on_message_batch: on_message_batch.clone(),
            on_send_ack: on_send_ack.clone(),
            on_send_failed: on_send_failed.clone(),
            on_recalled: on_recalled.clone(),
            on_typing: on_typing.clone(),
            call_signal_listeners: call_signal_listeners.clone(),
            on_conversation_synced: on_conversation_synced.clone(),
            on_conversation_created: on_conversation_created.clone(),
            on_conversation_updated: on_conversation_updated.clone(),
            on_conversation_unread_count_changed: on_conversation_unread_count_changed.clone(),
            on_conversation_deleted: on_conversation_deleted.clone(),
            on_extension: on_extension.clone(),
            on_notification: on_notification.clone(),
            on_sync_started: on_sync_started.clone(),
            on_sync_finished: on_sync_finished.clone(),
            on_sync_failed: on_sync_failed.clone(),
            on_sync_progress: on_sync_progress.clone(),
            on_sync_task_completed: on_sync_task_completed.clone(),
            on_any: on_any.clone(),
        };

        // 异步分发任务：非阻塞接收，每类回调在 spawn_blocking 中执行，避免阻塞事件循环。
        // 必须处理 Lagged：若只写 while let Ok(ev)，当通道积压导致 Lagged 时循环会退出，后续推送等事件将永远不再被分发。
        spawn_event_dispatch_loop(async move {
            loop {
                let ev = match rx.recv().await {
                    Ok(ev) => ev,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(
                            dropped = n,
                            "EventBus dispatch lagged, dropped events (continuing)"
                        );
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                match &ev {
                    SdkEvent::Connection(ce) => match ce {
                        ConnectionEvent::Connected => {
                            dispatch_callbacks(&on_connected, |f| f());
                        }
                        ConnectionEvent::Disconnected { reason } => {
                            dispatch_callbacks_with(
                                &on_disconnected,
                                || reason.clone(),
                                |f, reason| f(reason.clone()),
                            );
                        }
                        ConnectionEvent::StateChanged { state } => {
                            dispatch_callbacks_with(
                                &on_state_changed,
                                || state.clone(),
                                |f, state| f(state.clone()),
                            );
                        }
                        ConnectionEvent::SyncStateChanged { state } => {
                            let s = *state;
                            dispatch_callbacks(&on_sync_state_changed, move |f| {
                                f(s);
                            });
                        }
                        ConnectionEvent::ServerError { code, message } => {
                            dispatch_callbacks_with(
                                &on_server_error,
                                || (*code, message.clone()),
                                |f, (code, message)| f(*code, message.clone()),
                            );
                        }
                        ConnectionEvent::KickedOff { reason } => {
                            dispatch_callbacks_with(
                                &on_kicked_off,
                                || reason.clone(),
                                |f, reason| f(reason.clone()),
                            );
                        }
                        ConnectionEvent::TokenExpired { message } => {
                            dispatch_callbacks_with(
                                &on_token_expired,
                                || message.clone(),
                                |f, message| f(message.clone()),
                            );
                        }
                        ConnectionEvent::Reconnecting { .. } => {}
                    },
                    SdkEvent::Message(me) => match me {
                        MessageEvent::Received { message } => {
                            dispatch_callbacks_with(
                                &on_message,
                                || message.as_ref().clone(),
                                |f, message| f(message.clone()),
                            );
                        }
                        MessageEvent::ReceivedBatch { messages } => {
                            dispatch_callbacks_with(
                                &on_message_batch,
                                || messages.clone(),
                                |f, messages| f(messages.clone()),
                            );
                        }
                        MessageEvent::SendAck { ack } => {
                            dispatch_callbacks_with(
                                &on_send_ack,
                                || ack.as_ref().clone(),
                                |f, ack| f(ack.clone()),
                            );
                        }
                        MessageEvent::SendFailed {
                            client_msg_id,
                            reason,
                        } => {
                            dispatch_callbacks_with(
                                &on_send_failed,
                                || (client_msg_id.clone(), reason.clone()),
                                |f, (client_msg_id, reason)| {
                                    f(client_msg_id.clone(), reason.clone());
                                },
                            );
                        }
                        MessageEvent::Recalled {
                            conversation_id,
                            event,
                        } => {
                            dispatch_callbacks_with(
                                &on_recalled,
                                || (conversation_id.clone(), event.clone()),
                                |f, (conversation_id, event)| {
                                    f(conversation_id.clone(), event.clone());
                                },
                            );
                        }
                        MessageEvent::Typing {
                            conversation_id,
                            event,
                        } => {
                            dispatch_callbacks_with(
                                &on_typing,
                                || (conversation_id.clone(), event.clone()),
                                |f, (conversation_id, event)| {
                                    f(conversation_id.clone(), event.clone());
                                },
                            );
                        }
                        MessageEvent::CallSignal {
                            conversation_id,
                            event,
                        } => {
                            dispatch_callbacks_with(
                                &call_signal_listeners,
                                || (conversation_id.clone(), event.as_ref().clone()),
                                |f, (conversation_id, event)| {
                                    f(conversation_id.clone(), event.clone());
                                },
                            );
                        }
                        MessageEvent::Edited { .. }
                        | MessageEvent::ReactionChanged { .. }
                        | MessageEvent::Deleted { .. }
                        | MessageEvent::ReadReceipt { .. }
                        | MessageEvent::BurnScheduled { .. }
                        | MessageEvent::Burned { .. }
                        | MessageEvent::HardDeleted { .. }
                        | MessageEvent::Pinned { .. }
                        | MessageEvent::Unpinned { .. }
                        | MessageEvent::Marked { .. }
                        | MessageEvent::Unmarked { .. }
                        | MessageEvent::PresenceChanged { .. }
                        | MessageEvent::Custom { .. } => {}
                    },
                    SdkEvent::Notification(NotificationEvent::Received { message }) => {
                        dispatch_callbacks_with(
                            &on_notification,
                            || message.as_ref().clone(),
                            |f, message| f(message.clone()),
                        );
                    }
                    SdkEvent::Conversation(ce) => match ce {
                        ConversationEvent::Synced { conversation_ids } => {
                            dispatch_callbacks_with(
                                &on_conversation_synced,
                                || conversation_ids.clone(),
                                |f, conversation_ids| f(conversation_ids.clone()),
                            );
                        }
                        ConversationEvent::Created { conversation_id } => {
                            dispatch_callbacks_with(
                                &on_conversation_created,
                                || conversation_id.clone(),
                                |f, conversation_id| f(conversation_id.clone()),
                            );
                        }
                        ConversationEvent::Updated { conversation_id } => {
                            dispatch_callbacks_with(
                                &on_conversation_updated,
                                || conversation_id.clone(),
                                |f, conversation_id| f(conversation_id.clone()),
                            );
                        }
                        ConversationEvent::UnreadCountChanged {
                            conversation_id,
                            unread_count,
                        } => {
                            dispatch_callbacks_with(
                                &on_conversation_unread_count_changed,
                                || (conversation_id.clone(), *unread_count),
                                |f, (conversation_id, unread_count)| {
                                    f(conversation_id.clone(), *unread_count);
                                },
                            );
                        }
                        ConversationEvent::Deleted { conversation_id } => {
                            dispatch_callbacks_with(
                                &on_conversation_deleted,
                                || conversation_id.clone(),
                                |f, conversation_id| f(conversation_id.clone()),
                            );
                        }
                    },
                    SdkEvent::Sync(se) => match se {
                        SyncNotify::Started { run } if run.visibility.is_user_visible() => {
                            dispatch_callbacks(&on_sync_started, |f| f());
                        }
                        SyncNotify::Finished { run, phase } if run.visibility.is_user_visible() => {
                            dispatch_callbacks_with(
                                &on_sync_finished,
                                || phase.clone(),
                                |f, phase| f(phase.clone()),
                            );
                        }
                        SyncNotify::Failed { run, task, message }
                            if run.visibility.is_user_visible() =>
                        {
                            dispatch_callbacks_with(
                                &on_sync_failed,
                                || (task.clone(), message.clone()),
                                |f, (task, message)| f(task.clone(), message.clone()),
                            );
                        }
                        SyncNotify::Progress {
                            run,
                            task,
                            progress,
                            detail,
                        } if run.visibility.is_user_visible() => {
                            dispatch_callbacks_with(
                                &on_sync_progress,
                                || (task.clone(), *progress, detail.clone()),
                                |f, (task, progress, detail)| {
                                    f(task.clone(), *progress, detail.clone());
                                },
                            );
                        }
                        SyncNotify::TaskCompleted { run, task }
                            if run.visibility.is_user_visible() =>
                        {
                            dispatch_callbacks_with(
                                &on_sync_task_completed,
                                || task.clone(),
                                |f, task| f(task.clone()),
                            );
                        }
                        SyncNotify::StateChanged { run, state }
                            if run.visibility.is_user_visible() =>
                        {
                            let s = *state;
                            dispatch_callbacks(&on_sync_state_changed, move |f| {
                                f(s);
                            });
                        }
                        _ => {}
                    },
                    SdkEvent::Extension(ext) => {
                        dispatch_callbacks_with(
                            &on_extension,
                            || {
                                (
                                    ext.source.clone(),
                                    ext.event_type.clone(),
                                    ext.payload.clone(),
                                )
                            },
                            |f, (source, event_type, payload)| {
                                f(source.clone(), event_type.clone(), payload.clone());
                            },
                        );
                    }
                }
                // on_any 收到完整事件；无订阅时不额外分配 Arc。
                dispatch_any_callbacks(&on_any, ev);
            }
        });

        bus
    }

    pub fn publish(&self, mut event: SdkEvent) {
        if matches!(&event, SdkEvent::Sync(sync) if !sync.is_user_visible()) {
            return;
        }
        if self.middleware.before_publish(&mut event).is_drop() {
            return;
        }
        match &event {
            SdkEvent::Connection(ConnectionEvent::StateChanged { state }) => {
                *self.last_connection_state.safe_write("event_bus") = Some(state.clone());
            }
            SdkEvent::Connection(ConnectionEvent::Connected) => {
                *self.last_connection_state.safe_write("event_bus") = Some(SdkState::Connected);
            }
            SdkEvent::Connection(ConnectionEvent::Disconnected { .. }) => {
                *self.last_connection_state.safe_write("event_bus") = Some(SdkState::Disconnected);
            }
            SdkEvent::Sync(SyncNotify::StateChanged { state, .. }) => {
                *self.last_sync_state.safe_write("event_bus") = Some(*state);
            }
            SdkEvent::Sync(SyncNotify::Finished { phase, .. }) => {
                *self.last_sync_finished.safe_write("event_bus") = Some(phase.clone());
            }
            SdkEvent::Sync(SyncNotify::Started { .. }) => {
                *self.last_sync_finished.safe_write("event_bus") = None;
            }
            _ => {}
        }
        self.middleware.on_publish(&event);
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SdkEvent> {
        self.tx.subscribe()
    }

    fn callback_subscription<T, S>(
        &self,
        callbacks: &Arc<RwLock<Vec<T>>>,
        callback: T,
        same: S,
    ) -> Subscription
    where
        T: Clone + Send + Sync + 'static,
        S: Fn(&T, &T) -> bool + Send + Sync + 'static,
    {
        callbacks.safe_write("event_bus").push(callback.clone());
        let callbacks = callbacks.clone();
        Subscription {
            _tx: self.tx.clone(),
            cleanup: Some(Box::new(move || {
                callbacks
                    .safe_write("event_bus")
                    .retain(|stored| !same(stored, &callback));
            })),
        }
    }

    // ---------- Connection ----------
    /// 注册「连接成功」回调
    pub fn on_connected<F>(&self, f: F) -> Subscription
    where
        F: Fn() + Send + Sync + 'static,
    {
        let f = Arc::new(f) as FnConnected;
        let invoked = Arc::new(AtomicBool::new(false));
        let callback = {
            let f = f.clone();
            let invoked = invoked.clone();
            Arc::new(move || {
                invoked.store(true, Ordering::Release);
                f();
            }) as FnConnected
        };
        let already_connected = matches!(
            *self.last_connection_state.safe_read("event_bus"),
            Some(SdkState::Connected | SdkState::Ready)
        );
        let subscription = self.callback_subscription(&self.on_connected, callback, same_arc);
        if already_connected {
            replay_after_dispatch_window(move || {
                if !invoked.load(Ordering::Acquire) {
                    f();
                }
            });
        }
        subscription
    }

    /// 注册「断开连接」回调
    pub fn on_disconnected<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let f: FnDisconnected = Arc::new(move |s| f(s.as_str()));
        self.callback_subscription(&self.on_disconnected, f, same_arc)
    }

    /// 注册「连接状态变更」回调
    pub fn on_state_changed<F>(&self, f: F) -> Subscription
    where
        F: Fn(SdkState) + Send + Sync + 'static,
    {
        let f = Arc::new(f) as FnStateChanged;
        let invoked = Arc::new(AtomicBool::new(false));
        let callback = {
            let f = f.clone();
            let invoked = invoked.clone();
            Arc::new(move |state| {
                invoked.store(true, Ordering::Release);
                f(state);
            }) as FnStateChanged
        };
        let last = self.last_connection_state.safe_read("event_bus").clone();
        let subscription = self.callback_subscription(&self.on_state_changed, callback, same_arc);
        if let Some(state) = last {
            replay_after_dispatch_window(move || {
                if !invoked.load(Ordering::Acquire) {
                    f(state);
                }
            });
        }
        subscription
    }

    /// 注册「同步状态变更」回调
    pub fn on_sync_state_changed<F>(&self, f: F) -> Subscription
    where
        F: Fn(SyncState) + Send + Sync + 'static,
    {
        let f = Arc::new(f) as FnSyncStateChanged;
        let invoked = Arc::new(AtomicBool::new(false));
        let callback = {
            let f = f.clone();
            let invoked = invoked.clone();
            Arc::new(move |state| {
                invoked.store(true, Ordering::Release);
                f(state);
            }) as FnSyncStateChanged
        };
        let last = *self.last_sync_state.safe_read("event_bus");
        let subscription =
            self.callback_subscription(&self.on_sync_state_changed, callback, same_arc);
        if let Some(state) = last {
            replay_after_dispatch_window(move || {
                if !invoked.load(Ordering::Acquire) {
                    f(state);
                }
            });
        }
        subscription
    }

    /// 注册「服务端错误」回调
    pub fn on_server_error<F>(&self, f: F) -> Subscription
    where
        F: Fn(i32, &str) + Send + Sync + 'static,
    {
        let f: FnServerError = Arc::new(move |code, msg| f(code, msg.as_str()));
        self.callback_subscription(&self.on_server_error, f, same_arc)
    }

    /// 注册「被踢下线」回调（账号在其他设备/地点登录，当前设备被踢）
    pub fn on_kicked_off<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let f: FnKickedOff = Arc::new(move |r| f(r.as_str()));
        self.callback_subscription(&self.on_kicked_off, f, same_arc)
    }

    /// 注册「登录凭证过期」回调（需重新登录）
    pub fn on_token_expired<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let f: FnTokenExpired = Arc::new(move |m| f(m.as_str()));
        self.callback_subscription(&self.on_token_expired, f, same_arc)
    }

    // ---------- Message ----------
    /// 注册「收到一条新消息」回调（参数为 SDK 统一类型 IMMessage）
    pub fn on_message<F>(&self, f: F) -> Subscription
    where
        F: Fn(&IMMessage) + Send + Sync + 'static,
    {
        let f: FnMessage = Arc::new(move |m| f(&m));
        self.callback_subscription(&self.on_message, f, same_arc)
    }

    /// 注册「新消息批量」回调（同步或批量推送时一次多条，参数为 IMMessage 切片）
    pub fn on_message_batch<F>(&self, f: F) -> Subscription
    where
        F: Fn(&[IMMessage]) + Send + Sync + 'static,
    {
        let f: FnMessageBatch = Arc::new(move |msgs| f(msgs.as_slice()));
        self.callback_subscription(&self.on_message_batch, f, same_arc)
    }

    /// 注册「发送回执」回调
    pub fn on_send_ack<F>(&self, f: F) -> Subscription
    where
        F: Fn(&SendAck) + Send + Sync + 'static,
    {
        let f: FnSendAck = Arc::new(move |a| f(&a));
        self.callback_subscription(&self.on_send_ack, f, same_arc)
    }

    /// 注册「发送失败」回调
    pub fn on_send_failed<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str, &str) + Send + Sync + 'static,
    {
        let f: FnSendFailed = Arc::new(move |id, r| f(id.as_str(), r.as_str()));
        self.callback_subscription(&self.on_send_failed, f, same_arc)
    }

    /// 注册「消息撤回」回调
    pub fn on_recalled<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str, &MessageRecallEvent) + Send + Sync + 'static,
    {
        let f: FnRecalled = Arc::new(move |cid, e| f(cid.as_str(), &e));
        self.callback_subscription(&self.on_recalled, f, same_arc)
    }

    /// 注册「正在输入」回调
    pub fn on_typing<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str, &TypingEvent) + Send + Sync + 'static,
    {
        let f: FnTyping = Arc::new(move |cid, e| f(cid.as_str(), &e));
        self.callback_subscription(&self.on_typing, f, same_arc)
    }

    /// 注册「通话信令」下行（`EVENT_CALL_SIGNAL` → [`MessageEvent::CallSignal`]）。
    pub fn on_call_signal<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str, &CallSignalEvent) + Send + Sync + 'static,
    {
        let f: FnCallSignal = Arc::new(move |cid, e| f(cid.as_str(), &e));
        self.callback_subscription(&self.call_signal_listeners, f, same_arc)
    }

    // ---------- Conversation ----------
    /// 注册「会话列表同步完成」回调
    pub fn on_conversation_synced<F>(&self, f: F) -> Subscription
    where
        F: Fn(&[String]) + Send + Sync + 'static,
    {
        let f: FnConversationIds = Arc::new(move |ids| f(ids.as_slice()));
        self.callback_subscription(&self.on_conversation_synced, f, same_arc)
    }

    /// 注册「新会话」回调
    pub fn on_conversation_created<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let f: FnConversationId = Arc::new(move |cid| f(cid.as_str()));
        self.callback_subscription(&self.on_conversation_created, f, same_arc)
    }

    /// 注册「会话更新」回调
    pub fn on_conversation_updated<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let f: FnConversationId = Arc::new(move |cid| f(cid.as_str()));
        self.callback_subscription(&self.on_conversation_updated, f, same_arc)
    }

    /// 注册「会话未读数变化」回调
    pub fn on_conversation_unread_count_changed<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str, u32) + Send + Sync + 'static,
    {
        let f: FnConversationUnreadCountChanged =
            Arc::new(move |cid: String, cnt: u32| f(cid.as_str(), cnt));
        self.callback_subscription(&self.on_conversation_unread_count_changed, f, same_arc)
    }

    /// 注册「会话删除」回调
    pub fn on_conversation_deleted<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let f: FnConversationId = Arc::new(move |cid| f(cid.as_str()));
        self.callback_subscription(&self.on_conversation_deleted, f, same_arc)
    }

    // ---------- Extension ----------
    /// 注册「扩展事件」回调
    pub fn on_extension<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str, &str, &[u8]) + Send + Sync + 'static,
    {
        let f: FnExtension = Arc::new(move |s, t, p| f(s.as_str(), t.as_str(), &p));
        self.callback_subscription(&self.on_extension, f, same_arc)
    }

    /// 注册 IM 下行 Notification 回调（与聊天 `on_message` 分离）。
    pub fn on_notification<F>(&self, f: F) -> Subscription
    where
        F: Fn(&IMMessage) + Send + Sync + 'static,
    {
        let f: FnNotification = Arc::new(move |m| f(&m));
        self.callback_subscription(&self.on_notification, f, same_arc)
    }

    // ---------- Sync ----------
    /// 注册「同步开始」回调
    pub fn on_sync_started<F>(&self, f: F) -> Subscription
    where
        F: Fn() + Send + Sync + 'static,
    {
        let f: FnConnected = Arc::new(f);
        self.callback_subscription(&self.on_sync_started, f, same_arc)
    }

    /// 注册「同步阶段结束」回调
    pub fn on_sync_finished<F>(&self, f: F) -> Subscription
    where
        F: Fn(SyncPhase) + Send + Sync + 'static,
    {
        let f = Arc::new(f) as FnSyncPhase;
        let invoked = Arc::new(AtomicBool::new(false));
        let callback = {
            let f = f.clone();
            let invoked = invoked.clone();
            Arc::new(move |phase| {
                invoked.store(true, Ordering::Release);
                f(phase);
            }) as FnSyncPhase
        };
        let last = self.last_sync_finished.safe_read("event_bus").clone();
        let subscription = self.callback_subscription(&self.on_sync_finished, callback, same_arc);
        if let Some(phase) = last {
            replay_after_dispatch_window(move || {
                if !invoked.load(Ordering::Acquire) {
                    f(phase);
                }
            });
        }
        subscription
    }

    /// 注册「同步失败」回调
    pub fn on_sync_failed<F>(&self, f: F) -> Subscription
    where
        F: Fn(String, String) + Send + Sync + 'static,
    {
        let f: FnSendFailed = Arc::new(f);
        self.callback_subscription(&self.on_sync_failed, f, same_arc)
    }

    /// 注册「同步进度」回调
    pub fn on_sync_progress<F>(&self, f: F) -> Subscription
    where
        F: Fn(String, f32, String) + Send + Sync + 'static,
    {
        let f: FnSyncProgress = Arc::new(f);
        self.callback_subscription(&self.on_sync_progress, f, same_arc)
    }

    /// 注册「同步单任务完成」回调
    pub fn on_sync_task_completed<F>(&self, f: F) -> Subscription
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        let f: FnConversationId = Arc::new(f);
        self.callback_subscription(&self.on_sync_task_completed, f, same_arc)
    }

    // ---------- Raw / Any ----------
    /// 订阅原始事件流（broadcast receiver）
    pub fn subscribe_raw(&self) -> broadcast::Receiver<SdkEvent> {
        self.subscribe()
    }

    /// 注册「任意事件」回调（拿到完整 SdkEvent，用于日志、审计或未分类扩展）
    pub fn on_any<F>(&self, f: F) -> Subscription
    where
        F: Fn(Arc<SdkEvent>) + Send + Sync + 'static,
    {
        let f: FnAny = Arc::new(f);
        self.callback_subscription(&self.on_any, f, same_arc)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

pub type EventReceiver = broadcast::Receiver<SdkEvent>;

/// Callback subscription handle.
///
/// Dropping the handle removes the callback from the EventBus. This keeps
/// hot-reload, screen remount, and long-running app sessions from accumulating
/// stale host callbacks.
pub struct Subscription {
    pub(crate) _tx: broadcast::Sender<SdkEvent>,
    cleanup: Option<Box<dyn FnOnce() + Send + 'static>>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{EventBus, RecoverableRwLock};
    use crate::core::SyncRunContext;
    use crate::core::event::{SdkEvent, SyncNotify};
    use tokio::sync::broadcast::error::TryRecvError;

    #[tokio::test]
    async fn publish_drops_silent_sync_events_before_raw_subscribers() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe_raw();

        bus.publish(SdkEvent::Sync(SyncNotify::Started {
            run: SyncRunContext::silent_gap_repair(),
        }));

        assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn publish_keeps_user_visible_sync_events() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe_raw();

        bus.publish(SdkEvent::Sync(SyncNotify::Started {
            run: SyncRunContext::initial_login(),
        }));

        let received = rx.try_recv().expect("user visible sync event emitted");
        assert!(matches!(
            received,
            SdkEvent::Sync(SyncNotify::Started { .. })
        ));
    }

    #[test]
    fn dropping_subscription_removes_registered_callback() {
        let bus = EventBus::new();
        let subscription = bus.on_connected(|| {});

        assert_eq!(bus.on_connected.safe_read("event_bus").len(), 1);
        drop(subscription);
        assert_eq!(bus.on_connected.safe_read("event_bus").len(), 0);
    }
}
