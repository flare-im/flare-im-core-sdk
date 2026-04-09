//! 事件订阅 API 模块
//!
//! 实现事件类型映射、订阅、转发等功能

use std::ffi::{c_void, CString};
use std::sync::Arc;

use flare_im_core_sdk::event::{
    ConnectionEvent,
    ConversationEvent,
    MessageEvent,
    SdkEvent,
    SyncNotify,
};

use crate::callback::FlareEventCallback;
use crate::error::FlareErrorCode;
use crate::handle::{get_instance, next_subscription_id, EventSubscriptionInner, FlareEventSubscription, FlareImHandle};

/// 事件类型字符串
pub fn event_type_to_string(event: &SdkEvent) -> &'static str {
    match event {
        SdkEvent::Connection(ConnectionEvent::Connected) => "connection.connected",
        SdkEvent::Connection(ConnectionEvent::Disconnected { .. }) => "connection.disconnected",
        SdkEvent::Connection(ConnectionEvent::StateChanged { .. }) => "connection.state_changed",
        SdkEvent::Connection(ConnectionEvent::SyncStateChanged { .. }) => "connection.sync_state_changed",
        SdkEvent::Connection(ConnectionEvent::ServerError { .. }) => "connection.server_error",
        SdkEvent::Connection(ConnectionEvent::Reconnecting { .. }) => "connection.reconnecting",
        SdkEvent::Connection(ConnectionEvent::KickedOff { .. }) => "connection.kicked_off",
        SdkEvent::Connection(ConnectionEvent::TokenExpired { .. }) => "connection.token_expired",
        SdkEvent::Message(MessageEvent::Received { .. }) => "message.received",
        SdkEvent::Message(MessageEvent::ReceivedBatch { .. }) => "message.received_batch",
        SdkEvent::Message(MessageEvent::SendAck { .. }) => "message.send_ack",
        SdkEvent::Message(MessageEvent::SendFailed { .. }) => "message.send_failed",
        SdkEvent::Message(MessageEvent::Recalled { .. }) => "message.recalled",
        SdkEvent::Message(MessageEvent::Typing { .. }) => "message.typing",
        SdkEvent::Message(MessageEvent::Edited { .. }) => "message.edited",
        SdkEvent::Message(MessageEvent::ReactionChanged { .. }) => "message.reaction_changed",
        SdkEvent::Message(MessageEvent::Deleted { .. }) => "message.deleted",
        SdkEvent::Message(MessageEvent::ReadReceipt { .. }) => "message.read_receipt",
        SdkEvent::Message(MessageEvent::Pinned { .. }) => "message.pinned",
        SdkEvent::Message(MessageEvent::Unpinned { .. }) => "message.unpinned",
        SdkEvent::Conversation(ConversationEvent::Updated { .. }) => "conversation.updated",
        SdkEvent::Conversation(ConversationEvent::Synced { .. }) => "conversation.synced",
        SdkEvent::Sync(SyncNotify::Started) => "sync.started",
        SdkEvent::Sync(SyncNotify::Finished { .. }) => "sync.finished",
        _ => "unknown",
    }
}

/// 事件到 JSON
pub fn event_to_json(event: &SdkEvent) -> Result<String, FlareErrorCode> {
    serde_json::to_string(&EventPayload::from(event)).map_err(|e| {
        tracing::error!("Failed to serialize event: {}", e);
        FlareErrorCode::InternalError
    })
}

/// 事件载荷（用于 JSON 序列化）
#[derive(Debug, Clone, serde::Serialize)]
pub struct EventPayload {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl From<&SdkEvent> for EventPayload {
    fn from(event: &SdkEvent) -> Self {
        let event_type = event_type_to_string(event).to_string();
        let data = match event {
            SdkEvent::Connection(ConnectionEvent::Disconnected { reason }) => {
                Some(serde_json::json!({ "reason": reason }))
            }
            SdkEvent::Connection(ConnectionEvent::StateChanged { state }) => {
                Some(serde_json::json!({ "state": format!("{:?}", state) }))
            }
            SdkEvent::Connection(ConnectionEvent::SyncStateChanged { state }) => {
                Some(serde_json::json!({ "state": format!("{:?}", state) }))
            }
            SdkEvent::Connection(ConnectionEvent::ServerError { code, message }) => {
                Some(serde_json::json!({ "code": code, "message": message }))
            }
            SdkEvent::Connection(ConnectionEvent::Reconnecting { attempt }) => {
                Some(serde_json::json!({ "attempt": attempt }))
            }
            SdkEvent::Connection(ConnectionEvent::KickedOff { reason }) => {
                Some(serde_json::json!({ "reason": reason }))
            }
            SdkEvent::Connection(ConnectionEvent::TokenExpired { message }) => {
                Some(serde_json::json!({ "message": message }))
            }
            SdkEvent::Message(MessageEvent::Received { message }) => {
                Some(serde_json::json!({ "message": message }))
            }
            SdkEvent::Message(MessageEvent::ReceivedBatch { messages }) => {
                Some(serde_json::json!({ "messages": messages }))
            }
            SdkEvent::Message(MessageEvent::SendAck { ack }) => {
                Some(serde_json::json!({ "ack": ack }))
            }
            SdkEvent::Message(MessageEvent::SendFailed { client_msg_id, reason }) => {
                Some(serde_json::json!({
                    "client_msg_id": client_msg_id,
                    "reason": reason,
                }))
            }
            SdkEvent::Conversation(ConversationEvent::Updated { conversation_id }) => {
                Some(serde_json::json!({ "conversation_id": conversation_id }))
            }
            SdkEvent::Conversation(ConversationEvent::Synced { conversation_ids }) => {
                Some(serde_json::json!({ "conversation_ids": conversation_ids }))
            }
            SdkEvent::Sync(SyncNotify::Finished { phase }) => {
                Some(serde_json::json!({ "phase": format!("{:?}", phase) }))
            }
            _ => None,
        };
        Self { event_type, data }
    }
}

/// 订阅 SDK 事件
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `context` - 用户上下文，将传递给 callback
/// * `callback` - 事件回调
///
/// # Returns
/// 订阅句柄，用于取消订阅；id == 0 表示失败
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_subscribe_events(
    handle: FlareImHandle,
    context: *mut c_void,
    callback: FlareEventCallback,
) -> FlareEventSubscription {
    match subscribe_events_inner(handle, context, callback) {
        Ok(sub) => sub,
        Err(e) => {
            tracing::error!("Failed to subscribe events: {:?}", e);
            FlareEventSubscription::default()
        }
    }
}

fn subscribe_events_inner(
    handle: FlareImHandle,
    context: *mut c_void,
    callback: FlareEventCallback,
) -> Result<FlareEventSubscription, FlareErrorCode> {
    // 获取实例
    let instance = get_instance(handle)?;

    // 生成订阅 ID
    let id = next_subscription_id();

    // 创建取消通道
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();

    // 创建订阅内部状态
    let inner = Arc::new(EventSubscriptionInner { id, cancel_tx });

    // 启动事件转发任务
    spawn_event_forwarder(instance.clone(), inner.clone(), context, callback, cancel_rx);

    // 注册到实例
    instance.event_subscriptions.write().map_err(|_| FlareErrorCode::InternalError)?.push(inner);

    Ok(FlareEventSubscription { id })
}

/// 启动事件转发任务
fn spawn_event_forwarder(
    instance: Arc<crate::handle::SdkInstance>,
    _subscription: Arc<EventSubscriptionInner>,
    user_context: *mut c_void,
    callback: FlareEventCallback,
    mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
) {
    // 克隆客户端
    let client = instance.client.clone();

    // 在 Tokio runtime 中启动事件转发任务
    instance.runtime.spawn(async move {
        // 获取事件总线
        let Ok(bus) = client.bus() else {
            tracing::error!("Failed to get event bus");
            return;
        };

        // 订阅事件
        let mut rx = bus.subscribe();

        tracing::info!("Event forwarder started");

        loop {
            tokio::select! {
                // 处理取消信号
                _ = &mut cancel_rx => {
                    tracing::info!("Event forwarder cancelled");
                    break;
                }

                // 接收事件
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            // 转换事件类型
                            let event_type = match CString::new(event_type_to_string(&event)) {
                                Ok(s) => s,
                                Err(e) => {
                                    tracing::error!("Failed to create event type string: {}", e);
                                    continue;
                                }
                            };

                            // 序列化事件为 JSON
                            let event_json = match event_to_json(&event) {
                                Ok(json) => match CString::new(json) {
                                    Ok(s) => s.into_raw(),
                                    Err(e) => {
                                        tracing::error!("Failed to create event json string: {}", e);
                                        continue;
                                    }
                                },
                                Err(e) => {
                                    tracing::error!("Failed to serialize event: {:?}", e);
                                    continue;
                                }
                            };

                            // 调用回调
                            callback(user_context, event_type.as_ptr(), event_json);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            tracing::info!("Event bus closed");
                            break;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("Event forwarder lagged, missed {} events", n);
                            continue;
                        }
                    }
                }
            }
        }

        tracing::info!("Event forwarder stopped");
    });
}

/// 取消事件订阅
///
/// # Arguments
/// * `subscription` - 订阅句柄
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_unsubscribe(subscription: FlareEventSubscription) {
    // 注意：取消订阅需要从实例的订阅列表中移除并发送取消信号
    // 由于订阅存储在实例中，这里需要遍历所有实例来查找
    // 这是一个简化实现，实际可能需要维护一个全局订阅表
    tracing::warn!("Unsubscribe called for subscription {}, but not fully implemented", subscription.id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_to_string() {
        let event = SdkEvent::Connection(ConnectionEvent::Connected);
        assert_eq!(event_type_to_string(&event), "connection.connected");

        let event = SdkEvent::Connection(ConnectionEvent::Disconnected { reason: "test".to_string() });
        assert_eq!(event_type_to_string(&event), "connection.disconnected");
    }

    #[test]
    fn test_event_to_json() {
        let event = SdkEvent::Connection(ConnectionEvent::Connected);
        let json = event_to_json(&event);
        assert!(json.is_ok());
        let json_str = json.unwrap();
        assert!(json_str.contains("connection.connected"));
    }

    #[test]
    fn test_event_payload_from_event() {
        let event = SdkEvent::Connection(ConnectionEvent::Connected);
        let payload = EventPayload::from(&event);
        assert_eq!(payload.event_type, "connection.connected");
        assert!(payload.data.is_none());

        let event = SdkEvent::Connection(ConnectionEvent::Disconnected { reason: "test".to_string() });
        let payload = EventPayload::from(&event);
        assert_eq!(payload.event_type, "connection.disconnected");
        assert!(payload.data.is_some());
    }
}
