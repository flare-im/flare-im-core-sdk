//! Router：下行载荷 → EventBus。与 flare-proto 对齐：消息=MessagePush，事件=Event/EventEnvelope，回执=SendAck；同步=`DataPacket.sync_response`→`SyncRes`，扩展=`DataPacket.user_custom`/`CustomData`。

use std::sync::Arc;

use flare_proto::common::event::Payload as EventPayload;
use tracing::warn;

use crate::core::SyncResponseHandler;
use crate::event::{ConversationEvent, EventBus, ExtensionEvent, MessageEvent, SdkEvent};
use crate::infrastructure::protocol::DownlinkPayload;
use crate::model::IMMessage;
use crate::reliable_queue::ReliableSendQueue;
use crate::store::StoreProvider;

pub struct Dispatcher {
    bus: EventBus,
    reliable_queue: Option<Arc<ReliableSendQueue>>,
    sync_response_handler: Option<Arc<dyn SyncResponseHandler>>,
    /// 用于推送消息落库，使双端/查库与事件一致
    stores: Option<StoreProvider>,
}

impl Dispatcher {
    pub fn new(
        bus: EventBus,
        reliable_queue: Option<Arc<ReliableSendQueue>>,
        sync_response_handler: Option<Arc<dyn SyncResponseHandler>>,
        stores: Option<StoreProvider>,
    ) -> Self {
        Self {
            bus,
            reliable_queue,
            sync_response_handler,
            stores,
        }
    }

    pub fn bus(&self) -> &EventBus {
        &self.bus
    }

    pub async fn dispatch(
        &self,
        payload: DownlinkPayload,
    ) -> flare_core::common::error::Result<()> {
        match payload {
            DownlinkPayload::MessagePush(push) => {
                let mut all = Vec::new();
                all.extend(push.messages.clone());
                all.extend(push.notifications.clone());
                let messages: Vec<IMMessage> = all.into_iter().map(IMMessage::new).collect();
                if !messages.is_empty() {
                    if let Some(ref stores) = self.stores {
                        if let Err(e) = stores.messages.save_batch(&messages).await {
                            warn!(error = %e, "MessagePush save_batch failed");
                        } else if let Some(latest) = messages.iter().max_by_key(|m| m.seq) {
                            let conv_id = latest.conversation_id.clone();
                            let _ = stores
                                .conversations
                                .update_last_message(
                                    &conv_id,
                                    latest.server_id(),
                                    latest.sender_id(),
                                    latest.timestamp,
                                    latest.text_for_storage().as_deref(),
                                    latest.seq,
                                )
                                .await;
                        }
                    }
                    if messages.len() == 1 {
                        self.bus.publish(SdkEvent::Message(MessageEvent::Received {
                            message: messages.into_iter().next().unwrap(),
                        }));
                    } else {
                        self.bus
                            .publish(SdkEvent::Message(MessageEvent::ReceivedBatch { messages }));
                    }
                }
            }
            DownlinkPayload::Event(ev) => {
                self.dispatch_single_event(&ev).await;
            }
            DownlinkPayload::EventEnvelope(env) => {
                tracing::info!(
                    event_count = env.events.len(),
                    "dispatch EventEnvelope (push/sync)"
                );
                for ev in &env.events {
                    self.dispatch_single_event(ev).await;
                }
            }
            DownlinkPayload::SendAck(ack) => {
                if let Some(q) = &self.reliable_queue {
                    let _ = q.on_ack(ack.clone()).await;
                }
                self.bus
                    .publish(SdkEvent::Message(MessageEvent::SendAck { ack }));
            }
            DownlinkPayload::CustomData(data) => {
                self.bus.publish(SdkEvent::Extension(ExtensionEvent {
                    source: "custom".to_string(),
                    event_type: data.r#type.clone(),
                    payload: data.payload.clone(),
                }));
            }
            DownlinkPayload::SyncResp(resp) => {
                if let Some(h) = &self.sync_response_handler {
                    h.handle_sync_response(resp).await;
                }
            }
        }
        Ok(())
    }

    /// 分发单条 Event（从 EventEnvelope 或单条 Event 下行复用）
    async fn dispatch_single_event(&self, ev: &flare_proto::common::Event) {
        let mut messages: Vec<IMMessage> = Vec::new();
        if let Some(EventPayload::Message(m)) = &ev.payload {
            messages.push(IMMessage::new(m.clone()));
        }
        if !messages.is_empty() {
            if let Some(ref stores) = self.stores {
                if let Err(e) = stores.messages.save_batch(&messages).await {
                    warn!(error = %e, "single event message save_batch failed");
                } else if let Some(latest) = messages.iter().max_by_key(|m| m.seq) {
                    let conv_id = latest.conversation_id.clone();
                    let _ = stores
                        .conversations
                        .update_last_message(
                            &conv_id,
                            latest.server_id(),
                            latest.sender_id(),
                            latest.timestamp,
                            latest.text_for_storage().as_deref(),
                            ev.seq.max(latest.seq),
                        )
                        .await;
                }
            }
            for imm in messages {
                self.bus
                    .publish(SdkEvent::Message(MessageEvent::Received { message: imm }));
            }
        }
        let conv_id = ev.conversation_id.clone();
        if let Some(p) = &ev.payload {
            match p {
                EventPayload::Recall(recall) => {
                    self.bus.publish(SdkEvent::Message(MessageEvent::Recalled {
                        conversation_id: conv_id,
                        event: recall.clone(),
                    }));
                }
                EventPayload::Typing(typing) => {
                    self.bus.publish(SdkEvent::Message(MessageEvent::Typing {
                        conversation_id: conv_id,
                        event: typing.clone(),
                    }));
                }
                EventPayload::Conversation(_) => {
                    self.bus
                        .publish(SdkEvent::Conversation(ConversationEvent::Updated {
                            conversation_id: conv_id,
                        }));
                }
                EventPayload::ConversationDelete(_) => {
                    self.bus
                        .publish(SdkEvent::Conversation(ConversationEvent::Deleted {
                            conversation_id: conv_id,
                        }));
                }
                _ => {}
            }
        }
    }
}
