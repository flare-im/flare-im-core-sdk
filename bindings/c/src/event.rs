//! 事件 API - 事件订阅和监听
//!
//! 统一事件总线,支持所有平台

use std::ffi::c_void;
use std::sync::Arc;

use crate::abi;
use crate::helpers::string_to_flare;
use crate::registry::{require_instance, SdkInstance};
use crate::types::{FlareEventCallback, FlareHandle, FlareSubscriptionHandle};
use dashmap::DashMap;
use tokio::sync::oneshot;
use tokio::time::{sleep, Duration};

/// 事件类型码
pub const FLARE_EVENT_CONNECTION_CONNECTED: i32 = 1001;
pub const FLARE_EVENT_CONNECTION_DISCONNECTED: i32 = 1002;
pub const FLARE_EVENT_MESSAGE_RECEIVED: i32 = 2001;
pub const FLARE_EVENT_MESSAGE_SEND_ACK: i32 = 2002;
pub const FLARE_EVENT_CONVERSATION_UPDATED: i32 = 3001;

lazy_static::lazy_static! {
    static ref EVENT_SUBSCRIPTIONS: DashMap<u64, oneshot::Sender<()>> = DashMap::new();
}

pub(crate) fn unsubscribe_all_events() {
    let keys: Vec<u64> = EVENT_SUBSCRIPTIONS.iter().map(|e| *e.key()).collect();
    for key in keys {
        if let Some((_, tx)) = EVENT_SUBSCRIPTIONS.remove(&key) {
            let _ = tx.send(());
        }
    }
}

/// 事件类型转换
fn event_type_to_code(event: &flare_im_core_sdk::event::SdkEvent) -> i32 {
    use flare_im_core_sdk::event::{ConnectionEvent, MessageEvent, ConversationEvent, SdkEvent};
    
    match event {
        SdkEvent::Connection(ConnectionEvent::Connected) => FLARE_EVENT_CONNECTION_CONNECTED,
        SdkEvent::Connection(ConnectionEvent::Disconnected { .. }) => FLARE_EVENT_CONNECTION_DISCONNECTED,
        SdkEvent::Message(MessageEvent::SendAck { .. }) => FLARE_EVENT_MESSAGE_SEND_ACK,
        SdkEvent::Message(MessageEvent::Received { .. })
        | SdkEvent::Message(MessageEvent::ReceivedBatch { .. })
        | SdkEvent::Message(MessageEvent::SendFailed { .. })
        | SdkEvent::Message(MessageEvent::Recalled { .. })
        | SdkEvent::Message(MessageEvent::Typing { .. })
        | SdkEvent::Message(MessageEvent::Edited { .. })
        | SdkEvent::Message(MessageEvent::ReactionChanged { .. })
        | SdkEvent::Message(MessageEvent::Deleted { .. })
        | SdkEvent::Message(MessageEvent::ReadReceipt { .. })
        | SdkEvent::Message(MessageEvent::Pinned { .. })
        | SdkEvent::Message(MessageEvent::Unpinned { .. })
        | SdkEvent::Message(MessageEvent::Marked { .. })
        | SdkEvent::Message(MessageEvent::Unmarked { .. })
        | SdkEvent::Message(MessageEvent::PresenceChanged { .. })
        | SdkEvent::Message(MessageEvent::CallSignal { .. })
        | SdkEvent::Message(MessageEvent::Custom { .. }) => FLARE_EVENT_MESSAGE_RECEIVED,
        SdkEvent::Conversation(ConversationEvent::Synced { .. })
        | SdkEvent::Conversation(ConversationEvent::Created { .. })
        | SdkEvent::Conversation(ConversationEvent::Updated { .. })
        | SdkEvent::Conversation(ConversationEvent::UnreadCountChanged { .. })
        | SdkEvent::Conversation(ConversationEvent::Deleted { .. }) => FLARE_EVENT_CONVERSATION_UPDATED,
        _ => 0, // 保留给未来扩展；不会在下游被丢弃
    }
}

/// 将事件转换为 JSON 字符串（手动序列化）
fn event_to_json(event: &flare_im_core_sdk::event::SdkEvent) -> String {
    use flare_im_core_sdk::event::{ConnectionEvent, MessageEvent, ConversationEvent, SdkEvent};
    
    match event {
        SdkEvent::Connection(ConnectionEvent::Connected) => {
            r#"{"type":"connection","event":"connected"}"#.to_string()
        }
        SdkEvent::Connection(ConnectionEvent::Disconnected { reason }) => {
            format!(r#"{{"type":"connection","event":"disconnected","reason":"{}"}}"#, 
                reason.replace('"', "\\\""))
        }
        SdkEvent::Message(MessageEvent::Received { message }) => {
            match serde_json::to_string(message) {
                Ok(msg_json) => format!(r#"{{"type":"message","event":"received","message":{}}}"#, msg_json),
                Err(_) => r#"{"type":"message","event":"received","error":"serialize_failed"}"#.to_string(),
            }
        }
        SdkEvent::Message(MessageEvent::ReceivedBatch { messages }) => {
            match serde_json::to_string(messages) {
                Ok(arr_json) => format!(
                    r#"{{"type":"message","event":"received_batch","messages":{}}}"#,
                    arr_json
                ),
                Err(_) => r#"{"type":"message","event":"received_batch","error":"serialize_failed"}"#
                    .to_string(),
            }
        }
        SdkEvent::Message(MessageEvent::SendAck { ack }) => {
            // 手动序列化 SendAck
            format!(
                r#"{{"type":"message","event":"send_ack","ack":{{"client_msg_id":"{}","server_msg_id":"{}","seq":{},"conversation_id":"{}","success":{}}}}}"#,
                ack.client_msg_id.replace('"', "\\\""),
                ack.server_msg_id.replace('"', "\\\""),
                ack.seq,
                ack.conversation_id.replace('"', "\\\""),
                ack.success
            )
        }
        SdkEvent::Message(MessageEvent::Typing { conversation_id, event }) => {
            format!(
                r#"{{"type":"message","event":"typing","conversation_id":"{}","user_id":"{}","typing":{}}}"#,
                conversation_id.replace('"', "\\\""),
                event.user_id.replace('"', "\\\""),
                event.typing
            )
        }
        SdkEvent::Message(MessageEvent::Recalled { conversation_id, event }) => {
            format!(
                r#"{{"type":"message","event":"recalled","conversation_id":"{}","message_id":"{}","server_msg_id":"{}","reason":"{}"}}"#,
                conversation_id.replace('"', "\\\""),
                event.server_msg_id.replace('"', "\\\""),
                event.server_msg_id.replace('"', "\\\""),
                event.reason.replace('"', "\\\"")
            )
        }
        SdkEvent::Message(MessageEvent::ReadReceipt { conversation_id, event }) => {
            format!(
                r#"{{"type":"message","event":"read_receipt","conversation_id":"{}","user_id":"{}","read_seq":{}}}"#,
                conversation_id.replace('"', "\\\""),
                event.user_id.replace('"', "\\\""),
                event.read_seq
            )
        }
        SdkEvent::Conversation(ConversationEvent::UnreadCountChanged { conversation_id, unread_count }) => {
            format!(
                r#"{{"type":"conversation","event":"unread_count_changed","conversation_id":"{}","unread_count":{}}}"#,
                conversation_id.replace('"', "\\\""),
                unread_count
            )
        }
        SdkEvent::Conversation(ConversationEvent::Created { conversation_id }) => {
            format!(r#"{{"type":"conversation","event":"created","conversation_id":"{}"}}"#, conversation_id)
        }
        SdkEvent::Conversation(ConversationEvent::Updated { conversation_id }) => {
            format!(r#"{{"type":"conversation","event":"updated","conversation_id":"{}"}}"#, conversation_id)
        }
        SdkEvent::Conversation(ConversationEvent::Deleted { conversation_id }) => {
            format!(r#"{{"type":"conversation","event":"deleted","conversation_id":"{}"}}"#, conversation_id)
        }
        SdkEvent::Conversation(ConversationEvent::Synced { conversation_ids }) => {
            match serde_json::to_string(conversation_ids) {
                Ok(ids) => format!(r#"{{"type":"conversation","event":"synced","conversation_ids":{}}}"#, ids),
                Err(_) => r#"{"type":"conversation","event":"synced","conversation_ids":[]}"#.to_string(),
            }
        }
        _ => r#"{"type":"unknown"}"#.to_string(),
    }
}

/// 订阅事件
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `context` - 用户上下文
/// * `callback` - 事件回调
///
/// # Returns
/// 订阅句柄,0 表示失败
#[unsafe(no_mangle)]
pub extern "C" fn flare_event_subscribe(
    handle: FlareHandle,
    context: *mut c_void,
    callback: FlareEventCallback,
) -> FlareSubscriptionHandle {
    abi::catch_ffi_subscription_handle(|| match subscribe_events_inner(handle, context, callback) {
        Ok(sub) => sub,
        Err(_) => 0,
    })
}

fn subscribe_events_inner(
    handle: FlareHandle,
    context: *mut c_void,
    callback: FlareEventCallback,
) -> Result<FlareSubscriptionHandle, i32> {
    let instance = require_instance(handle)?;
    
    // 创建取消通道
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    
    // 生成订阅 ID
    let subscription_id = {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_SUB_ID: AtomicU64 = AtomicU64::new(1);
        NEXT_SUB_ID.fetch_add(1, Ordering::SeqCst)
    };
    EVENT_SUBSCRIPTIONS.insert(subscription_id, cancel_tx);
    
    // 启动事件转发任务
    spawn_event_forwarder(instance, subscription_id, context as usize, callback, cancel_rx);
    
    Ok(subscription_id)
}

/// 启动事件转发任务
fn spawn_event_forwarder(
    instance: Arc<SdkInstance>,
    subscription_id: u64,
    user_context: usize, // 使用 usize 代替 *mut c_void
    callback: FlareEventCallback,
    mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let client = instance.client.clone();
    
    instance.runtime.spawn(async move {
        // 等待事件总线可用，避免登录后瞬时竞态导致“订阅假成功、事件全丢”。
        let mut rx = loop {
            match client.bus() {
                Ok(bus) => break bus.subscribe(),
                Err(e) => {
                    tracing::warn!(error = %e, "event bus not ready yet, retrying");
                    tokio::select! {
                        _ = &mut cancel_rx => {
                            tracing::debug!("Event subscription cancelled before bus ready");
                            EVENT_SUBSCRIPTIONS.remove(&subscription_id);
                            return;
                        }
                        _ = sleep(Duration::from_millis(200)) => {}
                    }
                }
            }
        };
        
        loop {
            tokio::select! {
                // 处理取消信号
                _ = &mut cancel_rx => {
                    tracing::debug!("Event subscription cancelled");
                    EVENT_SUBSCRIPTIONS.remove(&subscription_id);
                    break;
                }
                
                // 接收事件
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            let event_type = event_type_to_code(&event);
                            if event_type == 0 {
                                continue;
                            }
                            // 序列化事件为 JSON
                            let event_json = string_to_flare(event_to_json(&event));
                            
                            abi::invoke_user_c_callback("FlareEventCallback", || {
                                callback(user_context as *mut c_void, event_type, event_json);
                            });
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            tracing::debug!("Event bus closed");
                            EVENT_SUBSCRIPTIONS.remove(&subscription_id);
                            break;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("Event subscription lagged, missed {} events", n);
                            continue;
                        }
                    }
                }
            }
        }
    });
}

/// 取消事件订阅
///
/// # Arguments
/// * `subscription` - 订阅句柄
#[unsafe(no_mangle)]
pub extern "C" fn flare_event_unsubscribe(subscription: FlareSubscriptionHandle) {
    abi::catch_ffi_void(|| {
        if let Some((_, tx)) = EVENT_SUBSCRIPTIONS.remove(&subscription) {
            let _ = tx.send(());
        } else {
            tracing::debug!("Unsubscribe {} ignored: not found", subscription);
        }
    });
}

/// 取消全部事件订阅（用于 Flutter/iOS 热重启或宿主崩溃恢复后的兜底清理）。
#[unsafe(no_mangle)]
pub extern "C" fn flare_event_unsubscribe_all() {
    abi::catch_ffi_void(unsubscribe_all_events);
}
