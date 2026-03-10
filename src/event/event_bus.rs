use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::broadcast;

use super::message_event::MessageEvent;
use super::conversation_event::ConversationEvent;

const DEFAULT_CAPACITY: usize = 4096;
static SUB_ID: AtomicU64 = AtomicU64::new(0);

// ── Unified SDK Event ────────────────────────────────────────

/// SDK 统一事件枚举
///
/// 核心只包含消息和会话领域事件，其他（用户、群组、在线状态等）
/// 通过 Extension / CustomPush 由扩展插件注入。
#[derive(Clone)]
pub enum SdkEvent {
    // ── 连接 ───────────────────────────────────────────────
    Connected,
    Disconnected { reason: String },
    Reconnecting { attempt: u32 },
    StateChanged { state: crate::core::SdkState },

    // ── 核心领域事件（消息 + 会话）─────────────────────────
    Message(MessageEvent),
    Conversation(ConversationEvent),

    // ── 同步进度 ───────────────────────────────────────────
    SyncProgress { task: String, progress: f32, detail: String },
    SyncTaskCompleted { task: String },
    SyncTaskFailed { task: String, error: String },

    // ── 自定义推送 ─────────────────────────────────────────
    CustomPush { data_type: String, payload: Vec<u8>, metadata: HashMap<String, String> },

    /// 扩展事件（payload 通过 downcast 获取强类型）
    ///
    /// 用户/群组/在线状态等非核心领域通过此通道扩展
    Extension { source: String, event_type: String, payload: Arc<dyn Any + Send + Sync> },

    // ── 错误 ───────────────────────────────────────────────
    ServerError { code: i32, message: String },
}

impl SdkEvent {
    pub fn extension<T: Send + Sync + 'static>(
        source: impl Into<String>,
        event_type: impl Into<String>,
        payload: T,
    ) -> Self {
        Self::Extension {
            source: source.into(),
            event_type: event_type.into(),
            payload: Arc::new(payload),
        }
    }
}

impl fmt::Debug for SdkEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connected => write!(f, "Connected"),
            Self::Disconnected { reason } => write!(f, "Disconnected({reason})"),
            Self::Reconnecting { attempt } => write!(f, "Reconnecting({attempt})"),
            Self::StateChanged { state } => write!(f, "StateChanged({state})"),
            Self::Message(e) => write!(f, "Message({e:?})"),
            Self::Conversation(e) => write!(f, "Conversation({e:?})"),
            Self::SyncProgress { task, progress, .. } => write!(f, "SyncProgress({task},{progress:.0}%)"),
            Self::SyncTaskCompleted { task } => write!(f, "SyncTaskCompleted({task})"),
            Self::SyncTaskFailed { task, error } => write!(f, "SyncTaskFailed({task}:{error})"),
            Self::CustomPush { data_type, .. } => write!(f, "CustomPush({data_type})"),
            Self::Extension { source, event_type, .. } => write!(f, "Extension({source}.{event_type})"),
            Self::ServerError { code, message } => write!(f, "ServerError({code}:{message})"),
        }
    }
}

pub type SharedEvent = Arc<SdkEvent>;

// ── EventBus ─────────────────────────────────────────────────

/// 事件总线 — broadcast fan-out
///
/// ```ignore
/// let bus = EventBus::new();
///
/// // 流式
/// let mut rx = bus.subscribe();
/// tokio::spawn(async move { while let Some(e) = rx.recv().await { println!("{e:?}"); } });
///
/// // 回调式
/// let _sub = bus.on_message(|msg| { println!("{}", msg.server_id); });
///
/// bus.publish(SdkEvent::Connected);
/// ```
#[derive(Clone)]
pub struct EventBus {
    tx: Arc<broadcast::Sender<SharedEvent>>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(DEFAULT_CAPACITY);
        Self { tx: Arc::new(tx) }
    }

    pub fn publish(&self, event: SdkEvent) {
        let _ = self.tx.send(Arc::new(event));
    }

    pub fn subscribe(&self) -> EventReceiver {
        EventReceiver { rx: self.tx.subscribe() }
    }

    /// 注册通用事件回调，返回 Subscription（drop 即取消）
    pub fn on<F>(&self, callback: F) -> Subscription
    where F: Fn(SharedEvent) + Send + Sync + 'static {
        let id = SUB_ID.fetch_add(1, Ordering::Relaxed);
        let mut rx = self.tx.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => callback(event),
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "event callback lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        Subscription { _id: id, handle }
    }

    /// 注册消息接收回调
    pub fn on_message<F>(&self, callback: F) -> Subscription
    where F: Fn(&crate::model::message::Message) + Send + Sync + 'static {
        self.on(move |e| {
            if let SdkEvent::Message(MessageEvent::Received { ref message }) = *e {
                callback(message);
            }
        })
    }

    /// 注册连接状态回调
    pub fn on_state_changed<F>(&self, callback: F) -> Subscription
    where F: Fn(crate::core::SdkState) + Send + Sync + 'static {
        self.on(move |e| {
            if let SdkEvent::StateChanged { state } = *e {
                callback(state);
            }
        })
    }

    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self { Self::new() }
}

// ── EventReceiver ────────────────────────────────────────────

pub struct EventReceiver {
    rx: broadcast::Receiver<SharedEvent>,
}

impl EventReceiver {
    pub async fn recv(&mut self) -> Option<SharedEvent> {
        loop {
            match self.rx.recv().await {
                Ok(event) => return Some(event),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "event receiver lagged");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}

// ── Subscription ─────────────────────────────────────────────

pub struct Subscription {
    _id: u64,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for Subscription {
    fn drop(&mut self) { self.handle.abort(); }
}
