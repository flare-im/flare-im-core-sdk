//! 内部事件总线 + 类型化回调 API
//!
//! 流程：内部发布 SdkEvent → bounded fan-out → 按事件类型调用已注册的回调和 raw 订阅者。
//! 不暴露大 trait，仅暴露 `on_*` 类型化注册，便于跨语言绑定（Swift / Kotlin / TypeScript）。

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use flare_proto::common::{
    CapabilityPacket, CustomEvent, MessageRecallEvent, SendAck, TypingStatePacket,
};

use crate::model::IMMessage;
use tokio::sync::mpsc;

use tracing::warn;

use super::selector::{
    CustomEventSelector, EventFilter, ExtensionEventType, MessageEventType, NotificationEventType,
    SdkEventKind, SdkEventType,
};
use super::types::{
    ConnectionEvent, ConversationEvent, MessageEvent, NotificationEvent, SdkEvent, SyncNotify,
    SyncPhase,
};
use crate::core::SdkState;
use crate::core::SyncState;
use crate::extension::middleware::MiddlewareChain;
#[cfg(target_arch = "wasm32")]
use crate::shared::util::{delay, spawn_background};
#[cfg(target_arch = "wasm32")]
use std::time::Duration;

/// 每个 raw 订阅者的默认队列容量。慢消费者超过容量时会被丢事件保护 SDK 内存。
const BUS_CAPACITY: usize = 2048;
const CALLBACK_DISPATCH_CAPACITY: usize = 8192;
const REPLAY_DISPATCH_CAPACITY: usize = 1024;
const REPLAY_DELAY_MS: u64 = 10;
static CALLBACK_DISPATCH_DROPPED: AtomicU64 = AtomicU64::new(0);
static REPLAY_DISPATCH_DROPPED: AtomicU64 = AtomicU64::new(0);
static RAW_SUBSCRIBER_DROPPED: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishOutcome {
    Published { receiver_count: usize },
    DroppedSilentSync,
    DroppedByMiddleware,
    NoReceivers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventReceiveError {
    Closed,
}

/// Bounded event receiver. Each subscription owns an independent queue.
pub struct EventReceiver {
    rx: mpsc::Receiver<SdkEvent>,
    filter: EventFilter,
    lagged: Arc<AtomicBool>,
    dropped_events: Arc<AtomicU64>,
}

pub type FilteredEventReceiver = EventReceiver;

impl EventReceiver {
    pub fn new(
        rx: mpsc::Receiver<SdkEvent>,
        filter: EventFilter,
        lagged: Arc<AtomicBool>,
        dropped_events: Arc<AtomicU64>,
    ) -> Self {
        Self {
            rx,
            filter,
            lagged,
            dropped_events,
        }
    }

    pub fn filter(&self) -> &EventFilter {
        &self.filter
    }

    pub async fn recv(&mut self) -> Result<SdkEvent, EventReceiveError> {
        if let Some(event) = self.take_resync_event() {
            return Ok(event);
        }
        self.rx.recv().await.ok_or(EventReceiveError::Closed)
    }

    pub fn try_recv(&mut self) -> Result<SdkEvent, mpsc::error::TryRecvError> {
        if let Some(event) = self.take_resync_event() {
            return Ok(event);
        }
        self.rx.try_recv()
    }

    fn take_resync_event(&self) -> Option<SdkEvent> {
        if !self.lagged.swap(false, Ordering::AcqRel) {
            return None;
        }
        let dropped_events = self.dropped_events.swap(0, Ordering::AcqRel).max(1);
        Some(SdkEvent::Sync(SyncNotify::ResyncNeeded {
            scope: "global".to_string(),
            reason: "event_queue_lagged".to_string(),
            dropped_events,
        }))
    }
}

/// 单一长生命周期事件分发线程的发送端（懒初始化）。
///
/// 取代「每次发布 spawn_blocking / 新线程」：有界 FIFO 队列保证回调按**发布顺序**执行，
/// 线程数恒定为 1，且不占用 Tokio blocking 池（避免事件风暴饿死 DB/网络等阻塞任务）。
#[cfg(not(target_arch = "wasm32"))]
fn event_dispatch_sender() -> &'static std::sync::mpsc::SyncSender<Box<dyn FnOnce() + Send>> {
    use std::sync::OnceLock;
    static DISPATCH: OnceLock<std::sync::mpsc::SyncSender<Box<dyn FnOnce() + Send>>> =
        OnceLock::new();
    DISPATCH.get_or_init(|| {
        let (tx, rx) =
            std::sync::mpsc::sync_channel::<Box<dyn FnOnce() + Send>>(CALLBACK_DISPATCH_CAPACITY);
        if let Err(error) = std::thread::Builder::new()
            .name("flare-sdk-event-dispatch".into())
            .spawn(move || {
                // 回调内部已各自 catch_unwind，此处顺序执行不会因单个回调 panic 而中断。
                while let Ok(job) = rx.recv() {
                    job();
                }
            })
        {
            warn!(%error, "failed to spawn flare-sdk-event-dispatch thread");
        }
        tx
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_callback(f: impl FnOnce() + Send + 'static) {
    let job = Box::new(move || {
        if catch_unwind(AssertUnwindSafe(f)).is_err() {
            warn!("EventBus callback panicked; continuing");
        }
    }) as Box<dyn FnOnce() + Send>;
    match event_dispatch_sender().try_send(job) {
        Ok(()) => {}
        Err(std::sync::mpsc::TrySendError::Full(_)) => {
            let dropped = CALLBACK_DISPATCH_DROPPED.fetch_add(1, Ordering::Relaxed) + 1;
            if dropped == 1 || dropped % 1024 == 0 {
                warn!(
                    total_dropped = dropped,
                    "EventBus callback dispatch queue full; callback dropped"
                );
            }
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            warn!("EventBus dispatch thread unavailable; callback dropped");
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn spawn_callback(f: impl FnOnce() + 'static) {
    spawn_background(async move {
        if catch_unwind(AssertUnwindSafe(f)).is_err() {
            warn!("EventBus callback panicked; continuing");
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn replay_after_dispatch_window(f: impl FnOnce() + Send + 'static) {
    match replay_dispatch_sender().try_send(Box::new(f)) {
        Ok(()) => {}
        Err(std::sync::mpsc::TrySendError::Full(_)) => {
            let dropped = REPLAY_DISPATCH_DROPPED.fetch_add(1, Ordering::Relaxed) + 1;
            if dropped == 1 || dropped % 1024 == 0 {
                warn!(
                    total_dropped = dropped,
                    "EventBus replay dispatch queue full; replay callback dropped"
                );
            }
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            warn!("EventBus replay dispatch thread unavailable; replay callback dropped");
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn replay_dispatch_sender() -> &'static std::sync::mpsc::SyncSender<Box<dyn FnOnce() + Send>> {
    use std::sync::OnceLock;
    static REPLAY: OnceLock<std::sync::mpsc::SyncSender<Box<dyn FnOnce() + Send>>> =
        OnceLock::new();
    REPLAY.get_or_init(|| {
        let (tx, rx) =
            std::sync::mpsc::sync_channel::<Box<dyn FnOnce() + Send>>(REPLAY_DISPATCH_CAPACITY);
        if let Err(error) = std::thread::Builder::new()
            .name("flare-sdk-event-replay".into())
            .spawn(move || {
                while let Ok(first) = rx.recv() {
                    std::thread::sleep(std::time::Duration::from_millis(REPLAY_DELAY_MS));
                    spawn_callback(first);
                    while let Ok(job) = rx.try_recv() {
                        spawn_callback(job);
                    }
                }
            })
        {
            warn!(%error, "failed to spawn flare-sdk-event-replay thread");
        }
        tx
    })
}

#[cfg(target_arch = "wasm32")]
fn replay_after_dispatch_window(f: impl FnOnce() + 'static) {
    spawn_background(async move {
        delay(Duration::from_millis(REPLAY_DELAY_MS)).await;
        if catch_unwind(AssertUnwindSafe(f)).is_err() {
            warn!("EventBus callback panicked; continuing");
        }
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

fn dispatch_callbacks<T, F>(callbacks: &Arc<RwLock<Vec<T>>>, invoke: F) -> usize
where
    T: Clone + Send + 'static,
    F: Fn(&T) + Send + 'static,
{
    let callbacks = callback_snapshot(callbacks);
    let count = callbacks.len();
    if callbacks.is_empty() {
        return 0;
    }

    spawn_callback(move || {
        for callback in callbacks {
            invoke(&callback);
        }
    });
    count
}

fn dispatch_callbacks_with<T, P, M, F>(
    callbacks: &Arc<RwLock<Vec<T>>>,
    make_payload: M,
    invoke: F,
) -> usize
where
    T: Clone + Send + 'static,
    P: Send + 'static,
    M: FnOnce() -> P,
    F: Fn(&T, &P) + Send + 'static,
{
    let callbacks = callback_snapshot(callbacks);
    let count = callbacks.len();
    if callbacks.is_empty() {
        return 0;
    }

    let payload = make_payload();
    spawn_callback(move || {
        for callback in callbacks {
            invoke(&callback, &payload);
        }
    });
    count
}

fn dispatch_any_callbacks(callbacks: &Arc<RwLock<Vec<FnAny>>>, event: SdkEvent) -> usize {
    let callbacks = callback_snapshot(callbacks);
    let count = callbacks.len();
    if callbacks.is_empty() {
        return 0;
    }

    let event = Arc::new(event);
    spawn_callback(move || {
        for callback in callbacks {
            callback(Arc::clone(&event));
        }
    });
    count
}

fn dispatch_route_callbacks(routes: &Arc<RwLock<Vec<EventRoute>>>, event: &SdkEvent) -> usize {
    let callbacks = {
        let routes = routes.safe_read("event_bus");
        routes
            .iter()
            .filter(|route| route.filter.matches(event))
            .map(|route| route.callback.clone())
            .collect::<Vec<_>>()
    };
    let count = callbacks.len();
    if callbacks.is_empty() {
        return 0;
    }

    let event = Arc::new(event.clone());
    spawn_callback(move || {
        for callback in callbacks {
            callback(Arc::clone(&event));
        }
    });
    count
}

fn same_arc<T: ?Sized>(left: &Arc<T>, right: &Arc<T>) -> bool {
    Arc::ptr_eq(left, right)
}

fn same_event_route(left: &EventRoute, right: &EventRoute) -> bool {
    left.filter == right.filter && Arc::ptr_eq(&left.callback, &right.callback)
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
type FnTyping = Arc<dyn Fn(String, TypingStatePacket) + Send + Sync>;
type FnCapability = Arc<dyn Fn(String, CapabilityPacket) + Send + Sync>;
type FnConversationIds = Arc<dyn Fn(Vec<String>) + Send + Sync>;
type FnConversationId = Arc<dyn Fn(String) + Send + Sync>;
type FnConversationUnreadCountChanged = Arc<dyn Fn(String, u32) + Send + Sync>;
type FnExtension = Arc<dyn Fn(String, String, Vec<u8>) + Send + Sync>;
type FnNotification = Arc<dyn Fn(IMMessage) + Send + Sync>;
type FnSyncPhase = Arc<dyn Fn(SyncPhase) + Send + Sync>;
type FnSyncProgress = Arc<dyn Fn(String, f32, String) + Send + Sync>;
type FnAny = Arc<dyn Fn(Arc<SdkEvent>) + Send + Sync>;

#[derive(Clone)]
struct EventRoute {
    filter: EventFilter,
    callback: FnAny,
}

#[derive(Clone)]
struct RawSubscriber {
    filter: EventFilter,
    tx: mpsc::Sender<SdkEvent>,
    lagged: Arc<AtomicBool>,
    dropped_events: Arc<AtomicU64>,
}

#[derive(Clone)]
pub struct EventBus {
    middleware: Arc<MiddlewareChain>,
    raw_subscriber_capacity: usize,
    subscribers: Arc<RwLock<Vec<RawSubscriber>>>,
    last_connection_state: Arc<RwLock<Option<SdkState>>>,
    last_sync_state: Arc<RwLock<Option<SyncState>>>,
    last_sync_finished: Arc<RwLock<Option<SyncPhase>>>,
    // Connection（std::sync::RwLock 支持同步注册，回调在专用分发线程中执行）
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
    capability_listeners: Arc<RwLock<Vec<FnCapability>>>,
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
    // Filtered event routes
    routes: Arc<RwLock<Vec<EventRoute>>>,
    // Any
    on_any: Arc<RwLock<Vec<FnAny>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::with_capacity(BUS_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self::with_middleware_and_capacity(Arc::new(MiddlewareChain::new()), capacity)
    }

    pub fn with_middleware(middleware: Arc<MiddlewareChain>) -> Self {
        Self::with_middleware_and_capacity(middleware, BUS_CAPACITY)
    }

    pub fn with_middleware_and_capacity(middleware: Arc<MiddlewareChain>, capacity: usize) -> Self {
        let raw_subscriber_capacity = capacity.max(1);
        let subscribers = Arc::new(RwLock::new(Vec::new()));
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
        let capability_listeners = Arc::new(RwLock::new(Vec::new()));
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
        let routes = Arc::new(RwLock::new(Vec::new()));
        let on_any = Arc::new(RwLock::new(Vec::new()));

        Self {
            middleware,
            raw_subscriber_capacity,
            subscribers,
            last_connection_state,
            last_sync_state,
            last_sync_finished,
            on_connected,
            on_disconnected,
            on_state_changed,
            on_sync_state_changed,
            on_server_error,
            on_kicked_off,
            on_token_expired,
            on_message,
            on_message_batch,
            on_send_ack,
            on_send_failed,
            on_recalled,
            on_typing,
            capability_listeners,
            on_conversation_synced,
            on_conversation_created,
            on_conversation_updated,
            on_conversation_unread_count_changed,
            on_conversation_deleted,
            on_extension,
            on_notification,
            on_sync_started,
            on_sync_finished,
            on_sync_failed,
            on_sync_progress,
            on_sync_task_completed,
            routes,
            on_any,
        }
    }

    fn publish_to_subscribers(&self, event: &SdkEvent) -> usize {
        let mut delivered = 0;
        let mut has_closed = false;
        let mut dropped_full = 0usize;

        let subscribers = self.subscribers.safe_read("event_bus").clone();
        for subscriber in subscribers.iter() {
            if subscriber.tx.is_closed() {
                has_closed = true;
                continue;
            }
            if !subscriber.filter.matches(event) {
                continue;
            }
            match subscriber.tx.try_send(event.clone()) {
                Ok(()) => delivered += 1,
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    has_closed = true;
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    subscriber.lagged.store(true, Ordering::Release);
                    subscriber.dropped_events.fetch_add(1, Ordering::Relaxed);
                    dropped_full += 1;
                }
            }
        }

        if dropped_full > 0 {
            let previous = RAW_SUBSCRIBER_DROPPED.fetch_add(dropped_full as u64, Ordering::Relaxed);
            let current = previous + dropped_full as u64;
            if previous == 0 || previous / 1024 != current / 1024 {
                warn!(
                    dropped = dropped_full,
                    total_dropped = current,
                    "EventBus raw subscriber queue full; dropping events for slow subscribers"
                );
            }
        }

        if has_closed {
            self.subscribers
                .safe_write("event_bus")
                .retain(|subscriber| !subscriber.tx.is_closed());
        }

        delivered
    }

    fn dispatch_typed_callbacks(&self, ev: &SdkEvent) -> usize {
        let mut dispatched = 0;
        match ev {
            SdkEvent::Connection(ce) => match ce {
                ConnectionEvent::Connected => {
                    dispatched += dispatch_callbacks(&self.on_connected, |f| f());
                }
                ConnectionEvent::Disconnected { reason } => {
                    dispatched += dispatch_callbacks_with(
                        &self.on_disconnected,
                        || reason.clone(),
                        |f, reason| f(reason.clone()),
                    );
                }
                ConnectionEvent::StateChanged { state } => {
                    dispatched += dispatch_callbacks_with(
                        &self.on_state_changed,
                        || state.clone(),
                        |f, state| f(state.clone()),
                    );
                }
                ConnectionEvent::SyncStateChanged { state } => {
                    let s = *state;
                    dispatched += dispatch_callbacks(&self.on_sync_state_changed, move |f| {
                        f(s);
                    });
                }
                ConnectionEvent::ServerError { code, message } => {
                    dispatched += dispatch_callbacks_with(
                        &self.on_server_error,
                        || (*code, message.clone()),
                        |f, (code, message)| f(*code, message.clone()),
                    );
                }
                ConnectionEvent::KickedOff { reason } => {
                    dispatched += dispatch_callbacks_with(
                        &self.on_kicked_off,
                        || reason.clone(),
                        |f, reason| f(reason.clone()),
                    );
                }
                ConnectionEvent::TokenExpired { message } => {
                    dispatched += dispatch_callbacks_with(
                        &self.on_token_expired,
                        || message.clone(),
                        |f, message| f(message.clone()),
                    );
                }
                ConnectionEvent::Reconnecting { .. } => {}
            },
            SdkEvent::Message(me) => match me {
                MessageEvent::Received { message } => {
                    dispatched += dispatch_callbacks_with(
                        &self.on_message,
                        || message.as_ref().clone(),
                        |f, message| f(message.clone()),
                    );
                }
                MessageEvent::ReceivedBatch { messages } => {
                    dispatched += dispatch_callbacks_with(
                        &self.on_message_batch,
                        || messages.clone(),
                        |f, messages| f(messages.clone()),
                    );
                }
                MessageEvent::SendAck { ack } => {
                    dispatched += dispatch_callbacks_with(
                        &self.on_send_ack,
                        || ack.as_ref().clone(),
                        |f, ack| f(ack.clone()),
                    );
                }
                MessageEvent::SendFailed {
                    client_msg_id,
                    reason,
                } => {
                    dispatched += dispatch_callbacks_with(
                        &self.on_send_failed,
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
                    dispatched += dispatch_callbacks_with(
                        &self.on_recalled,
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
                    dispatched += dispatch_callbacks_with(
                        &self.on_typing,
                        || (conversation_id.clone(), event.clone()),
                        |f, (conversation_id, event)| {
                            f(conversation_id.clone(), event.clone());
                        },
                    );
                }
                MessageEvent::Capability {
                    conversation_id,
                    packet,
                } => {
                    dispatched += dispatch_callbacks_with(
                        &self.capability_listeners,
                        || (conversation_id.clone(), packet.as_ref().clone()),
                        |f, (conversation_id, packet)| {
                            f(conversation_id.clone(), packet.clone());
                        },
                    );
                }
                MessageEvent::Edited { .. }
                | MessageEvent::ReactionChanged { .. }
                | MessageEvent::Deleted { .. }
                | MessageEvent::ReadReceipt { .. }
                | MessageEvent::RetentionScheduled { .. }
                | MessageEvent::RetentionExpired { .. }
                | MessageEvent::RetentionPurged { .. }
                | MessageEvent::Pinned { .. }
                | MessageEvent::Unpinned { .. }
                | MessageEvent::Marked { .. }
                | MessageEvent::Unmarked { .. }
                | MessageEvent::PresenceChanged { .. }
                | MessageEvent::Custom { .. } => {}
            },
            SdkEvent::Notification(NotificationEvent::Received { message }) => {
                dispatched += dispatch_callbacks_with(
                    &self.on_notification,
                    || message.as_ref().clone(),
                    |f, message| f(message.clone()),
                );
            }
            SdkEvent::Conversation(ce) => match ce {
                ConversationEvent::Synced { conversation_ids } => {
                    dispatched += dispatch_callbacks_with(
                        &self.on_conversation_synced,
                        || conversation_ids.clone(),
                        |f, conversation_ids| f(conversation_ids.clone()),
                    );
                }
                ConversationEvent::Created { conversation_id } => {
                    dispatched += dispatch_callbacks_with(
                        &self.on_conversation_created,
                        || conversation_id.clone(),
                        |f, conversation_id| f(conversation_id.clone()),
                    );
                }
                ConversationEvent::Updated { conversation_id } => {
                    dispatched += dispatch_callbacks_with(
                        &self.on_conversation_updated,
                        || conversation_id.clone(),
                        |f, conversation_id| f(conversation_id.clone()),
                    );
                }
                ConversationEvent::UnreadCountChanged {
                    conversation_id,
                    unread_count,
                } => {
                    dispatched += dispatch_callbacks_with(
                        &self.on_conversation_unread_count_changed,
                        || (conversation_id.clone(), *unread_count),
                        |f, (conversation_id, unread_count)| {
                            f(conversation_id.clone(), *unread_count);
                        },
                    );
                }
                ConversationEvent::Deleted { conversation_id } => {
                    dispatched += dispatch_callbacks_with(
                        &self.on_conversation_deleted,
                        || conversation_id.clone(),
                        |f, conversation_id| f(conversation_id.clone()),
                    );
                }
            },
            SdkEvent::Sync(se) => match se {
                SyncNotify::Started { run } if run.visibility.is_user_visible() => {
                    dispatched += dispatch_callbacks(&self.on_sync_started, |f| f());
                }
                SyncNotify::Finished { run, phase } if run.visibility.is_user_visible() => {
                    dispatched += dispatch_callbacks_with(
                        &self.on_sync_finished,
                        || phase.clone(),
                        |f, phase| f(phase.clone()),
                    );
                }
                SyncNotify::Failed { run, task, message } if run.visibility.is_user_visible() => {
                    dispatched += dispatch_callbacks_with(
                        &self.on_sync_failed,
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
                    dispatched += dispatch_callbacks_with(
                        &self.on_sync_progress,
                        || (task.clone(), *progress, detail.clone()),
                        |f, (task, progress, detail)| {
                            f(task.clone(), *progress, detail.clone());
                        },
                    );
                }
                SyncNotify::TaskCompleted { run, task } if run.visibility.is_user_visible() => {
                    dispatched += dispatch_callbacks_with(
                        &self.on_sync_task_completed,
                        || task.clone(),
                        |f, task| f(task.clone()),
                    );
                }
                SyncNotify::StateChanged { run, state } if run.visibility.is_user_visible() => {
                    let s = *state;
                    dispatched += dispatch_callbacks(&self.on_sync_state_changed, move |f| {
                        f(s);
                    });
                }
                _ => {}
            },
            SdkEvent::Extension(ext) => {
                dispatched += dispatch_callbacks_with(
                    &self.on_extension,
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
        dispatched
    }

    pub fn publish(&self, mut event: SdkEvent) -> PublishOutcome {
        if matches!(&event, SdkEvent::Sync(sync) if !sync.is_user_visible()) {
            return PublishOutcome::DroppedSilentSync;
        }
        if self.middleware.before_publish(&mut event).is_drop() {
            return PublishOutcome::DroppedByMiddleware;
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
        let raw_count = self.publish_to_subscribers(&event);
        let typed_count = self.dispatch_typed_callbacks(&event);
        let route_count = dispatch_route_callbacks(&self.routes, &event);
        let any_count = dispatch_any_callbacks(&self.on_any, event);
        let receiver_count = raw_count + typed_count + route_count + any_count;
        if receiver_count == 0 {
            PublishOutcome::NoReceivers
        } else {
            PublishOutcome::Published { receiver_count }
        }
    }

    pub fn publish_extension(
        &self,
        source: impl Into<String>,
        event_type: impl Into<String>,
        payload: impl Into<Vec<u8>>,
    ) -> PublishOutcome {
        self.publish(SdkEvent::custom_extension(source, event_type, payload))
    }

    pub fn subscribe(&self) -> EventReceiver {
        self.subscribe_filter(EventFilter::Any)
    }

    pub fn subscribe_filter(&self, filter: impl Into<EventFilter>) -> FilteredEventReceiver {
        let filter = filter.into();
        let (tx, rx) = mpsc::channel(self.raw_subscriber_capacity);
        let lagged = Arc::new(AtomicBool::new(false));
        let dropped_events = Arc::new(AtomicU64::new(0));
        self.subscribers
            .safe_write("event_bus")
            .push(RawSubscriber {
                filter: filter.clone(),
                tx,
                lagged: lagged.clone(),
                dropped_events: dropped_events.clone(),
            });
        EventReceiver::new(rx, filter, lagged, dropped_events)
    }

    pub fn subscribe_kind(&self, kind: SdkEventKind) -> FilteredEventReceiver {
        self.subscribe_filter(kind)
    }

    pub fn subscribe_event_type(&self, event_type: SdkEventType) -> FilteredEventReceiver {
        self.subscribe_filter(event_type)
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

    /// 注册「正在输入」回调（DATA realtime_control.typing）
    pub fn on_typing<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str, &TypingStatePacket) + Send + Sync + 'static,
    {
        let f: FnTyping = Arc::new(move |cid, e| f(cid.as_str(), &e));
        self.callback_subscription(&self.on_typing, f, same_arc)
    }

    /// 注册能力包下行（DATA capability；RTC/通话等插件信令统一走这里）。
    pub fn on_capability<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str, &CapabilityPacket) + Send + Sync + 'static,
    {
        let f: FnCapability = Arc::new(move |cid, packet| f(cid.as_str(), &packet));
        self.callback_subscription(&self.capability_listeners, f, same_arc)
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

    /// 注册某个扩展源 + 扩展事件类型的回调。
    pub fn on_extension_event<F>(
        &self,
        source: impl Into<String>,
        event_type: impl Into<String>,
        f: F,
    ) -> Subscription
    where
        F: Fn(&str, &str, &[u8]) + Send + Sync + 'static,
    {
        let event_type = SdkEventType::Extension(ExtensionEventType::named(source, event_type));
        self.on_event_type(event_type, move |event| {
            if let SdkEvent::Extension(extension) = event.as_ref() {
                f(
                    extension.source.as_str(),
                    extension.event_type.as_str(),
                    extension.payload.as_slice(),
                );
            }
        })
    }

    /// 注册 IM 下行 Notification 回调（与聊天 `on_message` 分离）。
    pub fn on_notification<F>(&self, f: F) -> Subscription
    where
        F: Fn(&IMMessage) + Send + Sync + 'static,
    {
        let f: FnNotification = Arc::new(move |m| f(&m));
        self.callback_subscription(&self.on_notification, f, same_arc)
    }

    /// 注册某个 `NotificationContent.notification_type` 的回调。
    pub fn on_notification_type<F>(
        &self,
        notification_type: impl Into<String>,
        f: F,
    ) -> Subscription
    where
        F: Fn(&IMMessage) + Send + Sync + 'static,
    {
        let event_type =
            SdkEventType::Notification(NotificationEventType::notification_type(notification_type));
        self.on_event_type(event_type, move |event| {
            if let SdkEvent::Notification(NotificationEvent::Received { message }) = event.as_ref()
            {
                f(message.as_ref());
            }
        })
    }

    /// 注册某个 durable `CustomEvent` 的回调。
    pub fn on_custom_event<F>(&self, selector: impl Into<CustomEventSelector>, f: F) -> Subscription
    where
        F: Fn(&str, &CustomEvent) + Send + Sync + 'static,
    {
        let event_type = SdkEventType::Message(MessageEventType::CustomNamed(selector.into()));
        self.on_event_type(event_type, move |event| {
            if let SdkEvent::Message(MessageEvent::Custom {
                conversation_id,
                event,
            }) = event.as_ref()
            {
                f(conversation_id.as_str(), event);
            }
        })
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
    /// 订阅原始事件流（每个订阅者独立 lossless queue）。
    pub fn subscribe_raw(&self) -> EventReceiver {
        self.subscribe()
    }

    /// 注册过滤后的事件回调。多个相同过滤器的回调会全部执行。
    pub fn on_event_filter<F>(&self, filter: impl Into<EventFilter>, f: F) -> Subscription
    where
        F: Fn(Arc<SdkEvent>) + Send + Sync + 'static,
    {
        let route = EventRoute {
            filter: filter.into(),
            callback: Arc::new(f),
        };
        self.callback_subscription(&self.routes, route, same_event_route)
    }

    /// 注册某个顶层事件域的回调。
    pub fn on_event_kind<F>(&self, kind: SdkEventKind, f: F) -> Subscription
    where
        F: Fn(Arc<SdkEvent>) + Send + Sync + 'static,
    {
        self.on_event_filter(kind, f)
    }

    /// 注册某个精确事件类型的回调。
    pub fn on_event_type<F>(&self, event_type: SdkEventType, f: F) -> Subscription
    where
        F: Fn(Arc<SdkEvent>) + Send + Sync + 'static,
    {
        self.on_event_filter(event_type, f)
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

/// Callback subscription handle.
///
/// Dropping the handle removes the callback from the EventBus. This keeps
/// hot-reload, screen remount, and long-running app sessions from accumulating
/// stale host callbacks.
pub struct Subscription {
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
    use super::{EventBus, PublishOutcome, RecoverableRwLock};
    use crate::core::SyncRunContext;
    use crate::core::event::{
        ConnectionEvent, ConnectionEventType, CustomEventDefinition, MessageEvent,
        MessageEventType, SdkEvent, SyncNotify,
    };
    use std::time::Duration;
    use tokio::sync::mpsc::{self, error::TryRecvError};

    #[tokio::test]
    async fn publish_drops_silent_sync_events_before_raw_subscribers() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe_raw();

        let outcome = bus.publish(SdkEvent::Sync(SyncNotify::Started {
            run: SyncRunContext::silent_gap_repair(),
        }));

        assert_eq!(outcome, PublishOutcome::DroppedSilentSync);
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn publish_keeps_user_visible_sync_events() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe_raw();

        let outcome = bus.publish(SdkEvent::Sync(SyncNotify::Started {
            run: SyncRunContext::initial_login(),
        }));

        assert!(matches!(
            outcome,
            PublishOutcome::Published { receiver_count } if receiver_count >= 1
        ));
        let received = rx.try_recv().expect("user visible sync event emitted");
        assert!(matches!(
            received,
            SdkEvent::Sync(SyncNotify::Started { .. })
        ));
    }

    #[tokio::test]
    async fn raw_subscriber_queue_honors_capacity_hint() {
        let bus = EventBus::with_capacity(1);
        let mut rx = bus.subscribe_raw();

        let first_outcome = bus.publish(SdkEvent::Sync(SyncNotify::Started {
            run: SyncRunContext::initial_login(),
        }));
        let second_outcome = bus.publish(SdkEvent::Sync(SyncNotify::Finished {
            run: SyncRunContext::initial_login(),
            phase: crate::core::event::SyncPhase::Init,
        }));

        assert!(matches!(
            first_outcome,
            PublishOutcome::Published { receiver_count } if receiver_count == 1
        ));
        assert_eq!(second_outcome, PublishOutcome::NoReceivers);

        let resync = rx.try_recv().expect("overflow emits resync marker first");
        assert!(matches!(
            resync,
            SdkEvent::Sync(SyncNotify::ResyncNeeded {
                scope,
                reason,
                dropped_events: 1,
            }) if scope == "global" && reason == "event_queue_lagged"
        ));
        let first = rx.try_recv().expect("first event retained");
        assert!(matches!(first, SdkEvent::Sync(SyncNotify::Started { .. })));
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn raw_subscriber_queue_drops_overflow_for_slow_consumers() {
        let bus = EventBus::with_capacity(2);
        let mut rx = bus.subscribe_raw();
        let total = 10_000usize;

        for i in 0..total {
            bus.publish(SdkEvent::Connection(ConnectionEvent::Disconnected {
                reason: format!("network-{i}"),
            }));
        }

        let resync = rx
            .try_recv()
            .expect("resync marker emitted before retained events");
        assert!(matches!(
            resync,
            SdkEvent::Sync(SyncNotify::ResyncNeeded {
                dropped_events,
                ..
            }) if dropped_events == (total - 2) as u64
        ));
        let received = rx.try_recv().expect("first burst event retained");
        assert!(matches!(
            received,
            SdkEvent::Connection(ConnectionEvent::Disconnected { reason })
                if reason == "network-0"
        ));
        let second = rx.try_recv().expect("second burst event retained");
        assert!(matches!(
            second,
            SdkEvent::Connection(ConnectionEvent::Disconnected { reason })
                if reason == "network-1"
        ));
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn filtered_receiver_gets_resync_marker_after_own_queue_overflows() {
        let bus = EventBus::with_capacity(1);
        let mut rx = bus.subscribe_event_type(ConnectionEventType::Connected.into());

        bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));
        bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));

        let resync = rx.try_recv().expect("resync marker emitted first");
        assert!(matches!(
            resync,
            SdkEvent::Sync(SyncNotify::ResyncNeeded {
                dropped_events: 1,
                ..
            })
        ));
        assert!(matches!(
            rx.try_recv().expect("retained matching event follows"),
            SdkEvent::Connection(ConnectionEvent::Connected)
        ));
    }

    #[tokio::test]
    async fn filtered_subscribers_are_isolated_under_burst() {
        let bus = EventBus::new();
        let mut connection_rx = bus.subscribe_event_type(ConnectionEventType::Connected.into());
        let mut custom_rx = bus.subscribe_event_type(MessageEventType::Custom.into());
        let total = 1_000usize;

        for i in 0..total {
            bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));
            bus.publish(SdkEvent::Message(MessageEvent::Custom {
                conversation_id: "c1".into(),
                event: CustomEventDefinition::new("app.iso", format!("event_{i}")).build(vec![]),
            }));
        }

        for _ in 0..total {
            assert!(matches!(
                connection_rx.try_recv().expect("connection event retained"),
                SdkEvent::Connection(ConnectionEvent::Connected)
            ));
        }
        assert!(matches!(connection_rx.try_recv(), Err(TryRecvError::Empty)));

        for i in 0..total {
            assert!(matches!(
                custom_rx.try_recv().expect("custom event retained"),
                SdkEvent::Message(MessageEvent::Custom { event, .. })
                    if event.name == format!("event_{i}")
            ));
        }
        assert!(matches!(custom_rx.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn closed_raw_subscriber_is_pruned_on_next_publish() {
        let bus = EventBus::new();
        let rx = bus.subscribe_raw();

        assert_eq!(bus.subscribers.safe_read("event_bus").len(), 1);
        drop(rx);
        bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));
        assert_eq!(bus.subscribers.safe_read("event_bus").len(), 0);
    }

    #[test]
    fn dropping_subscription_removes_registered_callback() {
        let bus = EventBus::new();
        let subscription = bus.on_connected(|| {});

        assert_eq!(bus.on_connected.safe_read("event_bus").len(), 1);
        drop(subscription);
        assert_eq!(bus.on_connected.safe_read("event_bus").len(), 0);
    }

    #[tokio::test]
    async fn connected_callback_replays_last_state_after_registration() {
        let bus = EventBus::new();
        bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));

        let (tx, mut rx) = mpsc::channel(1);
        let _subscription = bus.on_connected(move || {
            let _ = tx.try_send(());
        });

        tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("connected replay callback invoked")
            .expect("connected replay value");
    }

    #[test]
    fn dropping_filtered_subscription_removes_registered_route() {
        let bus = EventBus::new();
        let subscription = bus.on_event_type(ConnectionEventType::Connected.into(), |_| {});

        assert_eq!(bus.routes.safe_read("event_bus").len(), 1);
        drop(subscription);
        assert_eq!(bus.routes.safe_read("event_bus").len(), 0);
    }

    #[tokio::test]
    async fn filtered_receiver_skips_unmatched_events() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe_event_type(ConnectionEventType::Connected.into());

        bus.publish(SdkEvent::Connection(ConnectionEvent::Disconnected {
            reason: "network".into(),
        }));
        bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));

        let received = rx.try_recv().expect("connected event emitted");
        assert!(matches!(
            received,
            SdkEvent::Connection(ConnectionEvent::Connected)
        ));
    }

    #[tokio::test]
    async fn event_type_routes_fan_out_to_multiple_handlers() {
        let bus = EventBus::new();
        let (tx, mut rx) = mpsc::unbounded_channel();

        let tx1 = tx.clone();
        let _sub1 = bus.on_event_type(ConnectionEventType::Connected.into(), move |_| {
            tx1.send("first").expect("first route sends");
        });
        let _sub2 = bus.on_event_type(ConnectionEventType::Connected.into(), move |_| {
            tx.send("second").expect("second route sends");
        });

        bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));

        let mut seen = vec![
            tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("first route invoked")
                .expect("first route value"),
            tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("second route invoked")
                .expect("second route value"),
        ];
        seen.sort_unstable();
        assert_eq!(seen, vec!["first", "second"]);
    }

    #[tokio::test]
    async fn callbacks_fire_in_publish_order() {
        // 单一分发线程的核心保证：回调严格按发布顺序执行（取代 spawn_blocking 的并发乱序）。
        let bus = EventBus::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let _sub = bus.on_event_type(ConnectionEventType::Disconnected.into(), move |ev| {
            if let SdkEvent::Connection(ConnectionEvent::Disconnected { reason }) = ev.as_ref() {
                let _ = tx.send(reason.clone());
            }
        });

        const N: u32 = 64;
        for i in 0..N {
            bus.publish(SdkEvent::Connection(ConnectionEvent::Disconnected {
                reason: i.to_string(),
            }));
        }

        let mut got = Vec::with_capacity(N as usize);
        for _ in 0..N {
            let reason = tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("callback invoked within window")
                .expect("callback value");
            got.push(reason);
        }
        let expected: Vec<String> = (0..N).map(|i| i.to_string()).collect();
        assert_eq!(
            got, expected,
            "single dispatch thread must preserve publish order"
        );
    }

    #[tokio::test]
    async fn custom_event_definition_builds_and_filters_user_event() {
        let bus = EventBus::new();
        let definition = CustomEventDefinition::new("app.orders", "order_paid").with_version("v1");
        let mut rx = bus.subscribe_event_type(definition.event_type());

        bus.publish(SdkEvent::Message(MessageEvent::Custom {
            conversation_id: "c1".into(),
            event: CustomEventDefinition::new("app.orders", "order_cancelled")
                .with_version("v1")
                .build(Vec::new()),
        }));
        bus.publish(SdkEvent::Message(MessageEvent::Custom {
            conversation_id: "c1".into(),
            event: definition.build(b"{\"order_id\":\"o1\"}".to_vec()),
        }));

        let received = rx.try_recv().expect("custom event emitted");
        assert!(matches!(
            received,
            SdkEvent::Message(MessageEvent::Custom { event, .. })
                if event.namespace == "app.orders"
                    && event.name == "order_paid"
                    && event.version == "v1"
        ));
    }

    #[tokio::test]
    async fn broad_custom_event_filter_matches_all_custom_events() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe_event_type(MessageEventType::Custom.into());

        bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));
        bus.publish(SdkEvent::Message(MessageEvent::Custom {
            conversation_id: "c1".into(),
            event: CustomEventDefinition::new("app.any", "anything").build(Vec::new()),
        }));

        let received = rx.try_recv().expect("custom event emitted");
        assert!(matches!(
            received,
            SdkEvent::Message(MessageEvent::Custom { .. })
        ));
    }
}
