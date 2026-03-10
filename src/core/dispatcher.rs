use std::sync::Arc;

use tracing::debug;

use crate::event::{EventBus, SdkEvent, MessageEvent, ConversationEvent};
use crate::middleware::MiddlewareChain;
use crate::model::server_packet;
use crate::model::event::EventType;
use flare_proto::common::event::Payload as EventPayload;

/// Dispatcher — 将 ServerPacket 分发为 SdkEvent 并发布到 EventBus
///
/// 覆盖 event.proto 中定义的所有 EventType：
/// - 消息类：Message / Recall / Edit / Delete / ReadReceipt / Typing / Reaction / Pin / Unpin / Mark / Unmark
/// - 会话类：ConversationUpdate / ConversationDelete
/// - 扩展类：Presence / CallSignal / Custom → Extension 事件
///
/// ```text
/// ServerPacket → Dispatcher → MessageInterceptor → EventInterceptor → EventBus
/// ```
pub struct Dispatcher {
    bus: EventBus,
    chain: Arc<MiddlewareChain>,
}

impl Dispatcher {
    pub fn new(bus: EventBus, chain: Arc<MiddlewareChain>) -> Self {
        Self { bus, chain }
    }

    pub fn bus(&self) -> &EventBus {
        &self.bus
    }

    pub async fn dispatch(&self, packet: crate::model::ServerPacket) {
        let Some(payload) = packet.payload else {
            debug!("ServerPacket with empty payload, ignored");
            return;
        };

        match payload {
            server_packet::Payload::EventEnvelope(env) => {
                self.dispatch_envelope(&env).await;
            }
            server_packet::Payload::SendAck(ack) => {
                self.emit(SdkEvent::Message(MessageEvent::SendAck { ack })).await;
            }
            server_packet::Payload::SyncResp(resp) => {
                if let Some(ref env) = resp.envelope {
                    self.dispatch_envelope(env).await;
                }
            }
            server_packet::Payload::SyncConversationsResp(resp) => {
                for patch in resp.patches {
                    self.emit(SdkEvent::Conversation(ConversationEvent::Patched { patch })).await;
                }
            }
            server_packet::Payload::SyncConversationsAllResp(resp) => {
                self.emit(SdkEvent::Conversation(ConversationEvent::Synced {
                    conversations: resp.conversations,
                })).await;
            }
            server_packet::Payload::CustomPush(custom) => {
                self.handle_custom_push(custom.r#type, custom.payload, custom.metadata).await;
            }
            server_packet::Payload::Error(err) => {
                self.emit(SdkEvent::ServerError { code: err.code, message: err.message }).await;
            }
            _ => {
                debug!("unhandled ServerPacket variant");
            }
        }
    }

    async fn dispatch_envelope(&self, env: &flare_proto::common::EventEnvelope) {
        for ev in &env.events {
            let typ = EventType::try_from(ev.r#type).unwrap_or(EventType::Unspecified);
            let cid = ev.conversation_id.clone();

            match typ {
                // ── 消息类 ──────────────────────────────
                EventType::EventMessage => {
                    if let Some(EventPayload::Message(msg)) = &ev.payload {
                        tracing::debug!(
                            conversation_id = %msg.conversation_id,
                            sender_id = %msg.sender_id,
                            server_id = %msg.server_id,
                            "received push message, dispatching to bus"
                        );
                        self.emit_message_received(msg.clone()).await;
                    }
                }
                EventType::EventMessageRecall => {
                    if let Some(EventPayload::Recall(e)) = &ev.payload {
                        self.emit(SdkEvent::Message(MessageEvent::Recalled {
                            conversation_id: cid, event: e.clone(),
                        })).await;
                    }
                }
                EventType::EventMessageEdit => {
                    if let Some(EventPayload::Edit(e)) = &ev.payload {
                        self.emit(SdkEvent::Message(MessageEvent::Edited {
                            conversation_id: cid, event: e.clone(),
                        })).await;
                    }
                }
                EventType::EventMessageDelete => {
                    if let Some(EventPayload::Delete(e)) = &ev.payload {
                        self.emit(SdkEvent::Message(MessageEvent::Deleted {
                            conversation_id: cid, event: e.clone(),
                        })).await;
                    }
                }
                EventType::EventReadReceipt => {
                    if let Some(EventPayload::Read(e)) = &ev.payload {
                        self.emit(SdkEvent::Message(MessageEvent::ReadReceipt {
                            conversation_id: cid, event: e.clone(),
                        })).await;
                    }
                }
                EventType::EventTyping => {
                    if let Some(EventPayload::Typing(e)) = &ev.payload {
                        self.emit(SdkEvent::Message(MessageEvent::Typing {
                            conversation_id: cid, event: e.clone(),
                        })).await;
                    }
                }
                EventType::EventReaction => {
                    if let Some(EventPayload::Reaction(e)) = &ev.payload {
                        self.emit(SdkEvent::Message(MessageEvent::ReactionUpdated {
                            conversation_id: cid, event: e.clone(),
                        })).await;
                    }
                }
                EventType::EventPin => {
                    if let Some(EventPayload::Pin(e)) = &ev.payload {
                        self.emit(SdkEvent::Message(MessageEvent::Pinned {
                            conversation_id: cid, event: e.clone(),
                        })).await;
                    }
                }
                EventType::EventUnpin => {
                    if let Some(EventPayload::Unpin(e)) = &ev.payload {
                        self.emit(SdkEvent::Message(MessageEvent::Unpinned {
                            conversation_id: cid, event: e.clone(),
                        })).await;
                    }
                }
                EventType::EventMark => {
                    if let Some(EventPayload::Mark(e)) = &ev.payload {
                        self.emit(SdkEvent::Message(MessageEvent::Marked {
                            conversation_id: cid, event: e.clone(),
                        })).await;
                    }
                }
                EventType::EventUnmark => {
                    if let Some(EventPayload::Unmark(e)) = &ev.payload {
                        self.emit(SdkEvent::Message(MessageEvent::Unmarked {
                            conversation_id: cid, event: e.clone(),
                        })).await;
                    }
                }

                // ── 会话类 ──────────────────────────────
                EventType::EventConversationUpdate => {
                    if let Some(EventPayload::Conversation(e)) = &ev.payload {
                        self.emit(SdkEvent::Conversation(ConversationEvent::Updated {
                            conversation_id: cid,
                            event: e.clone(),
                        })).await;
                    }
                }
                EventType::EventConversationDelete => {
                    self.emit(SdkEvent::Conversation(ConversationEvent::Deleted {
                        conversation_id: cid,
                    })).await;
                }

                // ── 扩展类（非核心领域 → Extension 事件）──
                EventType::EventPresence => {
                    if let Some(EventPayload::Presence(e)) = &ev.payload {
                        self.emit(SdkEvent::extension("presence", "changed", e.clone())).await;
                    }
                }
                EventType::EventCallSignal => {
                    if let Some(EventPayload::CallSignal(e)) = &ev.payload {
                        self.emit(SdkEvent::extension("call_signal", &e.signal_type, e.clone())).await;
                    }
                }
                EventType::EventCustom => {
                    if let Some(EventPayload::Custom(e)) = &ev.payload {
                        let event_type = format!("{}.{}", e.namespace, e.name);
                        self.emit(SdkEvent::extension("custom_event", &event_type, e.clone())).await;
                    }
                }

                _ => {
                    debug!(event_type = ?typ, "unhandled event type");
                }
            }
        }
    }

    async fn emit_message_received(&self, message: flare_proto::common::Message) {
        if self.chain.has_message_interceptors() {
            match self.chain.intercept_incoming(message).await {
                Ok(Some(msg)) => self.emit(SdkEvent::Message(MessageEvent::Received { message: msg })).await,
                Ok(None) => {}
                Err(_) => {}
            }
        } else {
            self.emit(SdkEvent::Message(MessageEvent::Received { message })).await;
        }
    }

    async fn emit(&self, event: SdkEvent) {
        if self.chain.has_event_interceptors() {
            match self.chain.intercept_event(event).await {
                Ok(Some(e)) => self.bus.publish(e),
                _ => {}
            }
        } else {
            self.bus.publish(event);
        }
    }

    async fn handle_custom_push(
        &self,
        data_type: String,
        payload: Vec<u8>,
        metadata: std::collections::HashMap<String, String>,
    ) {
        let extra = self.chain.handle_custom_push(&data_type, &payload, &metadata).await;
        if !extra.is_empty() {
            for e in extra { self.emit(e).await; }
        } else {
            self.emit(SdkEvent::CustomPush { data_type, payload, metadata }).await;
        }
    }
}
