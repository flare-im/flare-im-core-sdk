//! 内部事件总线 + 类型化回调 API
//!
//! 流程：内部发布 SdkEvent → broadcast 通道 → 异步分发任务 → 按事件类型调用已注册的回调。
//! 不暴露大 trait，仅暴露 `on_*` 类型化注册，便于跨语言绑定（Swift / Kotlin / TypeScript）。

use std::sync::{Arc, RwLock};

use flare_proto::common::{MessageRecallEvent, SendAck, TypingEvent};

use crate::model::IMMessage;
use tokio::sync::broadcast;

use tracing::warn;

use super::types::{
    ConnectionEvent, ConversationEvent, MessageEvent, SdkEvent, SyncNotify, SyncPhase,
};
use crate::core::SdkState;
use crate::fsm::SyncState;

/// 事件通道容量。过小在同步/推送高峰时会导致 Lagged，分发循环会跳过部分事件并继续；适当增大可减少 Lagged。
const BUS_CAPACITY: usize = 256;

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
type FnConversationIds = Arc<dyn Fn(Vec<String>) + Send + Sync>;
type FnConversationId = Arc<dyn Fn(String) + Send + Sync>;
type FnExtension = Arc<dyn Fn(String, String, Vec<u8>) + Send + Sync>;
type FnSyncPhase = Arc<dyn Fn(SyncPhase) + Send + Sync>;
type FnSyncProgress = Arc<dyn Fn(String, f32, String) + Send + Sync>;
type FnAny = Arc<dyn Fn(Arc<SdkEvent>) + Send + Sync>;

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<SdkEvent>,
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
    // Conversation
    on_conversation_synced: Arc<RwLock<Vec<FnConversationIds>>>,
    on_conversation_created: Arc<RwLock<Vec<FnConversationId>>>,
    on_conversation_updated: Arc<RwLock<Vec<FnConversationId>>>,
    on_conversation_unread_count_changed: Arc<RwLock<Vec<Arc<dyn Fn(String, u32) + Send + Sync>>>>,
    on_conversation_deleted: Arc<RwLock<Vec<FnConversationId>>>,
    // Extension
    on_extension: Arc<RwLock<Vec<FnExtension>>>,
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
        let (tx, mut rx) = broadcast::channel(BUS_CAPACITY);

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
        let on_conversation_synced = Arc::new(RwLock::new(Vec::new()));
        let on_conversation_created = Arc::new(RwLock::new(Vec::new()));
        let on_conversation_updated = Arc::new(RwLock::new(Vec::new()));
        let on_conversation_unread_count_changed = Arc::new(RwLock::new(Vec::new()));
        let on_conversation_deleted = Arc::new(RwLock::new(Vec::new()));
        let on_extension = Arc::new(RwLock::new(Vec::new()));
        let on_sync_started = Arc::new(RwLock::new(Vec::new()));
        let on_sync_finished = Arc::new(RwLock::new(Vec::new()));
        let on_sync_failed = Arc::new(RwLock::new(Vec::new()));
        let on_sync_progress = Arc::new(RwLock::new(Vec::new()));
        let on_sync_task_completed = Arc::new(RwLock::new(Vec::new()));
        let on_any = Arc::new(RwLock::new(Vec::new()));

        let bus = Self {
            tx: tx.clone(),
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
            on_conversation_synced: on_conversation_synced.clone(),
            on_conversation_created: on_conversation_created.clone(),
            on_conversation_updated: on_conversation_updated.clone(),
            on_conversation_unread_count_changed: on_conversation_unread_count_changed.clone(),
            on_conversation_deleted: on_conversation_deleted.clone(),
            on_extension: on_extension.clone(),
            on_sync_started: on_sync_started.clone(),
            on_sync_finished: on_sync_finished.clone(),
            on_sync_failed: on_sync_failed.clone(),
            on_sync_progress: on_sync_progress.clone(),
            on_sync_task_completed: on_sync_task_completed.clone(),
            on_any: on_any.clone(),
        };

        // 异步分发任务：非阻塞接收，每类回调在 spawn_blocking 中执行，避免阻塞事件循环。
        // 必须处理 Lagged：若只写 while let Ok(ev)，当通道积压导致 Lagged 时循环会退出，后续推送等事件将永远不再被分发。
        tokio::spawn(async move {
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
                let ev = Arc::new(ev);
                match ev.as_ref() {
                    SdkEvent::Connection(ce) => match ce {
                        ConnectionEvent::Connected => {
                            let list = on_connected.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f();
                                }
                            });
                        }
                        ConnectionEvent::Disconnected { reason } => {
                            let r = reason.clone();
                            let list = on_disconnected.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(r.clone());
                                }
                            });
                        }
                        ConnectionEvent::StateChanged { state } => {
                            let state = state.clone();
                            let list = on_state_changed.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(state.clone());
                                }
                            });
                        }
                        ConnectionEvent::SyncStateChanged { state } => {
                            let s = *state;
                            let list = on_sync_state_changed.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(s);
                                }
                            });
                        }
                        ConnectionEvent::ServerError { code, message } => {
                            let (c, m) = (*code, message.clone());
                            let list = on_server_error.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(c, m.clone());
                                }
                            });
                        }
                        ConnectionEvent::KickedOff { reason } => {
                            let r = reason.clone();
                            let list = on_kicked_off.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(r.clone());
                                }
                            });
                        }
                        ConnectionEvent::TokenExpired { message } => {
                            let m = message.clone();
                            let list = on_token_expired.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(m.clone());
                                }
                            });
                        }
                        ConnectionEvent::Reconnecting { .. } => {}
                    },
                    SdkEvent::Message(me) => match me {
                        MessageEvent::Received { message } => {
                            let msg = message.clone();
                            let list = on_message.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(msg.clone());
                                }
                            });
                        }
                        MessageEvent::ReceivedBatch { messages } => {
                            let msgs = messages.clone();
                            let list = on_message_batch.clone();
                            let single_list = on_message.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(msgs.clone());
                                }
                                // 向后兼容：批量下发也逐条触发 on_message，避免业务仅监听单条回调时漏消息。
                                let single = single_list.read().unwrap();
                                for m in msgs.iter() {
                                    for f in single.iter() {
                                        f(m.clone());
                                    }
                                }
                            });
                        }
                        MessageEvent::SendAck { ack } => {
                            let a = ack.clone();
                            let list = on_send_ack.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(a.clone());
                                }
                            });
                        }
                        MessageEvent::SendFailed {
                            client_msg_id,
                            reason,
                        } => {
                            let (id, r) = (client_msg_id.clone(), reason.clone());
                            let list = on_send_failed.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(id.clone(), r.clone());
                                }
                            });
                        }
                        MessageEvent::Recalled {
                            conversation_id,
                            event,
                        } => {
                            let (cid, e) = (conversation_id.clone(), event.clone());
                            let list = on_recalled.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(cid.clone(), e.clone());
                                }
                            });
                        }
                        MessageEvent::Typing {
                            conversation_id,
                            event,
                        } => {
                            let (cid, e) = (conversation_id.clone(), event.clone());
                            let list = on_typing.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(cid.clone(), e.clone());
                                }
                            });
                        }
                    },
                    SdkEvent::Conversation(ce) => match ce {
                        ConversationEvent::Synced { conversation_ids } => {
                            let ids = conversation_ids.clone();
                            let list = on_conversation_synced.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(ids.clone());
                                }
                            });
                        }
                        ConversationEvent::Created { conversation_id } => {
                            let cid = conversation_id.clone();
                            let list = on_conversation_created.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(cid.clone());
                                }
                            });
                        }
                        ConversationEvent::Updated { conversation_id } => {
                            let cid = conversation_id.clone();
                            let list = on_conversation_updated.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(cid.clone());
                                }
                            });
                        }
                        ConversationEvent::UnreadCountChanged {
                            conversation_id,
                            unread_count,
                        } => {
                            let (cid, cnt) = (conversation_id.clone(), *unread_count);
                            let list = on_conversation_unread_count_changed.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(cid.clone(), cnt);
                                }
                            });
                        }
                        ConversationEvent::Deleted { conversation_id } => {
                            let cid = conversation_id.clone();
                            let list = on_conversation_deleted.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(cid.clone());
                                }
                            });
                        }
                    },
                    SdkEvent::Sync(se) => match se {
                        SyncNotify::Started => {
                            let list = on_sync_started.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f();
                                }
                            });
                        }
                        SyncNotify::Finished { phase } => {
                            let p = phase.clone();
                            let list = on_sync_finished.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(p.clone());
                                }
                            });
                        }
                        SyncNotify::Failed { task, message } => {
                            let (t, m) = (task.clone(), message.clone());
                            let list = on_sync_failed.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(t.clone(), m.clone());
                                }
                            });
                        }
                        SyncNotify::Progress {
                            task,
                            progress,
                            detail,
                        } => {
                            let (t, p, d) = (task.clone(), *progress, detail.clone());
                            let list = on_sync_progress.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(t.clone(), p, d.clone());
                                }
                            });
                        }
                        SyncNotify::TaskCompleted { task } => {
                            let t = task.clone();
                            let list = on_sync_task_completed.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(t.clone());
                                }
                            });
                        }
                        SyncNotify::StateChanged { state } => {
                            let s = *state;
                            let list = on_sync_state_changed.clone();
                            tokio::task::spawn_blocking(move || {
                                let list = list.read().unwrap();
                                for f in list.iter() {
                                    f(s);
                                }
                            });
                        }
                    },
                    SdkEvent::Extension(ext) => {
                        let (src, ty, payload) = (
                            ext.source.clone(),
                            ext.event_type.clone(),
                            ext.payload.clone(),
                        );
                        let list = on_extension.clone();
                        tokio::task::spawn_blocking(move || {
                            let list = list.read().unwrap();
                            for f in list.iter() {
                                f(src.clone(), ty.clone(), payload.clone());
                            }
                        });
                    }
                }
                // on_any 收到完整事件
                let list = on_any.clone();
                let ev_clone = Arc::clone(&ev);
                tokio::task::spawn_blocking(move || {
                    let list = list.read().unwrap();
                    for f in list.iter() {
                        f(Arc::clone(&ev_clone));
                    }
                });
            }
        });

        bus
    }

    pub fn publish(&self, event: SdkEvent) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SdkEvent> {
        self.tx.subscribe()
    }

    fn subscription(&self) -> Subscription {
        Subscription {
            _tx: self.tx.clone(),
        }
    }

    // ---------- Connection ----------
    /// 注册「连接成功」回调
    pub fn on_connected<F>(&self, f: F) -> Subscription
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_connected.write().unwrap().push(Arc::new(f));
        self.subscription()
    }

    /// 注册「断开连接」回调
    pub fn on_disconnected<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let f: FnDisconnected = Arc::new(move |s| f(s.as_str()));
        self.on_disconnected.write().unwrap().push(f);
        self.subscription()
    }

    /// 注册「连接状态变更」回调
    pub fn on_state_changed<F>(&self, f: F) -> Subscription
    where
        F: Fn(SdkState) + Send + Sync + 'static,
    {
        self.on_state_changed.write().unwrap().push(Arc::new(f));
        self.subscription()
    }

    /// 注册「同步状态变更」回调
    pub fn on_sync_state_changed<F>(&self, f: F) -> Subscription
    where
        F: Fn(SyncState) + Send + Sync + 'static,
    {
        self.on_sync_state_changed
            .write()
            .unwrap()
            .push(Arc::new(f));
        self.subscription()
    }

    /// 注册「服务端错误」回调
    pub fn on_server_error<F>(&self, f: F) -> Subscription
    where
        F: Fn(i32, &str) + Send + Sync + 'static,
    {
        let f: FnServerError = Arc::new(move |code, msg| f(code, msg.as_str()));
        self.on_server_error.write().unwrap().push(f);
        self.subscription()
    }

    /// 注册「被踢下线」回调（账号在其他设备/地点登录，当前设备被踢）
    pub fn on_kicked_off<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let f: FnKickedOff = Arc::new(move |r| f(r.as_str()));
        self.on_kicked_off.write().unwrap().push(f);
        self.subscription()
    }

    /// 注册「登录凭证过期」回调（需重新登录）
    pub fn on_token_expired<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let f: FnTokenExpired = Arc::new(move |m| f(m.as_str()));
        self.on_token_expired.write().unwrap().push(f);
        self.subscription()
    }

    // ---------- Message ----------
    /// 注册「收到一条新消息」回调（参数为 SDK 统一类型 IMMessage）
    pub fn on_message<F>(&self, f: F) -> Subscription
    where
        F: Fn(&IMMessage) + Send + Sync + 'static,
    {
        let f: FnMessage = Arc::new(move |m| f(&m));
        self.on_message.write().unwrap().push(f);
        self.subscription()
    }

    /// 注册「新消息批量」回调（同步或批量推送时一次多条，参数为 IMMessage 切片）
    pub fn on_message_batch<F>(&self, f: F) -> Subscription
    where
        F: Fn(&[IMMessage]) + Send + Sync + 'static,
    {
        let f: FnMessageBatch = Arc::new(move |msgs| f(msgs.as_slice()));
        self.on_message_batch.write().unwrap().push(f);
        self.subscription()
    }

    /// 注册「发送回执」回调
    pub fn on_send_ack<F>(&self, f: F) -> Subscription
    where
        F: Fn(&SendAck) + Send + Sync + 'static,
    {
        let f: FnSendAck = Arc::new(move |a| f(&a));
        self.on_send_ack.write().unwrap().push(f);
        self.subscription()
    }

    /// 注册「发送失败」回调
    pub fn on_send_failed<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str, &str) + Send + Sync + 'static,
    {
        let f: FnSendFailed = Arc::new(move |id, r| f(id.as_str(), r.as_str()));
        self.on_send_failed.write().unwrap().push(f);
        self.subscription()
    }

    /// 注册「消息撤回」回调
    pub fn on_recalled<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str, &MessageRecallEvent) + Send + Sync + 'static,
    {
        let f: FnRecalled = Arc::new(move |cid, e| f(cid.as_str(), &e));
        self.on_recalled.write().unwrap().push(f);
        self.subscription()
    }

    /// 注册「正在输入」回调
    pub fn on_typing<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str, &TypingEvent) + Send + Sync + 'static,
    {
        let f: FnTyping = Arc::new(move |cid, e| f(cid.as_str(), &e));
        self.on_typing.write().unwrap().push(f);
        self.subscription()
    }

    // ---------- Conversation ----------
    /// 注册「会话列表同步完成」回调
    pub fn on_conversation_synced<F>(&self, f: F) -> Subscription
    where
        F: Fn(&[String]) + Send + Sync + 'static,
    {
        let f: FnConversationIds = Arc::new(move |ids| f(ids.as_slice()));
        self.on_conversation_synced.write().unwrap().push(f);
        self.subscription()
    }

    /// 注册「新会话」回调
    pub fn on_conversation_created<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let f: FnConversationId = Arc::new(move |cid| f(cid.as_str()));
        self.on_conversation_created.write().unwrap().push(f);
        self.subscription()
    }

    /// 注册「会话更新」回调
    pub fn on_conversation_updated<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let f: FnConversationId = Arc::new(move |cid| f(cid.as_str()));
        self.on_conversation_updated.write().unwrap().push(f);
        self.subscription()
    }

    /// 注册「会话未读数变化」回调
    pub fn on_conversation_unread_count_changed<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str, u32) + Send + Sync + 'static,
    {
        let f = Arc::new(move |cid: String, cnt: u32| f(cid.as_str(), cnt));
        self.on_conversation_unread_count_changed
            .write()
            .unwrap()
            .push(f);
        self.subscription()
    }

    /// 注册「会话删除」回调
    pub fn on_conversation_deleted<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let f: FnConversationId = Arc::new(move |cid| f(cid.as_str()));
        self.on_conversation_deleted.write().unwrap().push(f);
        self.subscription()
    }

    // ---------- Extension ----------
    /// 注册「扩展事件」回调
    pub fn on_extension<F>(&self, f: F) -> Subscription
    where
        F: Fn(&str, &str, &[u8]) + Send + Sync + 'static,
    {
        let f: FnExtension = Arc::new(move |s, t, p| f(s.as_str(), t.as_str(), &p));
        self.on_extension.write().unwrap().push(f);
        self.subscription()
    }

    // ---------- Sync ----------
    /// 注册「同步开始」回调
    pub fn on_sync_started<F>(&self, f: F) -> Subscription
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_sync_started.write().unwrap().push(Arc::new(f));
        self.subscription()
    }

    /// 注册「同步阶段结束」回调
    pub fn on_sync_finished<F>(&self, f: F) -> Subscription
    where
        F: Fn(SyncPhase) + Send + Sync + 'static,
    {
        self.on_sync_finished.write().unwrap().push(Arc::new(f));
        self.subscription()
    }

    /// 注册「同步失败」回调
    pub fn on_sync_failed<F>(&self, f: F) -> Subscription
    where
        F: Fn(String, String) + Send + Sync + 'static,
    {
        self.on_sync_failed.write().unwrap().push(Arc::new(f));
        self.subscription()
    }

    /// 注册「同步进度」回调
    pub fn on_sync_progress<F>(&self, f: F) -> Subscription
    where
        F: Fn(String, f32, String) + Send + Sync + 'static,
    {
        self.on_sync_progress.write().unwrap().push(Arc::new(f));
        self.subscription()
    }

    /// 注册「同步单任务完成」回调
    pub fn on_sync_task_completed<F>(&self, f: F) -> Subscription
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        let f: FnConversationId = Arc::new(move |t| f(t));
        self.on_sync_task_completed.write().unwrap().push(f);
        self.subscription()
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
        self.on_any.write().unwrap().push(Arc::new(f));
        self.subscription()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

pub type EventReceiver = broadcast::Receiver<SdkEvent>;

/// 持有即保持对总线的引用；可用于取消订阅的占位（当前实现中不主动移除回调，仅防止总线被 drop）
pub struct Subscription {
    pub(crate) _tx: broadcast::Sender<SdkEvent>,
}
