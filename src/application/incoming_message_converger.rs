use std::sync::{Arc, RwLock};

use crate::Result;
use crate::domain::{IncomingMessageConvergenceDecision, MessageDeliveryService, MessageStore};
use crate::event::{EventBus, MessageEvent, SdkEvent};
use crate::model::IMMessage;
use crate::reliable_queue::ReliableSendQueue;

#[derive(Clone)]
pub(crate) struct IncomingMessageConverger {
    message_store: Arc<dyn MessageStore>,
    bus: EventBus,
    reliable_queue: Arc<RwLock<Option<Arc<ReliableSendQueue>>>>,
}

impl IncomingMessageConverger {
    pub(crate) fn new(
        message_store: Arc<dyn MessageStore>,
        bus: EventBus,
        reliable_queue: Option<Arc<ReliableSendQueue>>,
    ) -> Self {
        Self {
            message_store,
            bus,
            reliable_queue: Arc::new(RwLock::new(reliable_queue)),
        }
    }

    pub(crate) fn set_reliable_queue(&self, reliable_queue: Option<Arc<ReliableSendQueue>>) {
        if let Ok(mut guard) = self.reliable_queue.write() {
            *guard = reliable_queue;
        }
    }

    pub(crate) async fn converge_messages(
        &self,
        current_user_id: &str,
        messages: Vec<IMMessage>,
    ) -> Result<Vec<IMMessage>> {
        let mut out = Vec::with_capacity(messages.len());
        for message in messages {
            let local_by_client = if message.client_msg_id.trim().is_empty() {
                None
            } else {
                self.message_store
                    .get_by_client_msg_id(&message.client_msg_id)
                    .await?
            };
            match MessageDeliveryService::decide_incoming_message_convergence(
                current_user_id,
                &message,
                local_by_client.as_ref(),
            ) {
                IncomingMessageConvergenceDecision::EmitReceived => out.push(message),
                IncomingMessageConvergenceDecision::MergePendingAndAck => {
                    let ack = MessageDeliveryService::synthetic_ack_from_incoming(&message);
                    let reliable_queue = self
                        .reliable_queue
                        .read()
                        .ok()
                        .and_then(|guard| guard.clone());
                    if let Some(queue) = reliable_queue {
                        queue.on_ack(ack).await?;
                    } else {
                        let merged = MessageDeliveryService::merge_incoming_as_sent(
                            local_by_client.as_ref(),
                            &message,
                        );
                        self.message_store
                            .update_after_ack(&message.client_msg_id, &merged)
                            .await?;
                        self.bus
                            .publish(SdkEvent::Message(MessageEvent::SendAck { ack }));
                    }
                }
            }
        }
        Ok(out)
    }
}
