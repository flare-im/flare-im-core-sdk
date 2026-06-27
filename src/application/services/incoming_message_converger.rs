use std::sync::{Arc, RwLock};

use crate::domain::{
    ConversationIdRewrite, ConversationIdentityService, ConversationStore,
    IncomingMessageConvergenceDecision, MessageDeliveryService, MessageStore,
};
use crate::kernel::ReliableSendQueuePort;
use crate::kernel::event::{EventBus, MessageEvent, SdkEvent};
use crate::model::IMMessage;
use crate::shared::error::Result;

#[derive(Clone)]
pub(crate) struct IncomingMessageConverger {
    message_store: Arc<dyn MessageStore>,
    conversation_store: Arc<dyn ConversationStore>,
    bus: EventBus,
    reliable_queue: Arc<RwLock<Option<Arc<dyn ReliableSendQueuePort>>>>,
}

impl IncomingMessageConverger {
    pub(crate) fn new(
        message_store: Arc<dyn MessageStore>,
        conversation_store: Arc<dyn ConversationStore>,
        bus: EventBus,
        reliable_queue: Option<Arc<dyn ReliableSendQueuePort>>,
    ) -> Self {
        Self {
            message_store,
            conversation_store,
            bus,
            reliable_queue: Arc::new(RwLock::new(reliable_queue)),
        }
    }

    pub(crate) fn set_reliable_queue(
        &self,
        reliable_queue: Option<Arc<dyn ReliableSendQueuePort>>,
    ) {
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
            let mut message = message;
            if let Some(rewrite) = ConversationIdentityService::canonicalize_single_chat_message(
                &mut message,
                current_user_id,
            ) {
                self.apply_identity_rewrite(rewrite).await?;
            }
            let local_by_client = if message.client_msg_id.trim().is_empty() {
                None
            } else {
                self.message_store
                    .get_by_client_msg_id(&message.client_msg_id)
                    .await?
            };
            let local_by_server = if message.server_id.trim().is_empty() {
                None
            } else {
                self.message_store.get(&message.server_id).await?
            };
            match MessageDeliveryService::decide_incoming_message_convergence(
                current_user_id,
                &message,
                local_by_client.as_ref(),
                local_by_server.as_ref(),
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
                        self.bus.publish(SdkEvent::Message(MessageEvent::SendAck {
                            ack: Box::new(ack),
                        }));
                    }
                }
                IncomingMessageConvergenceDecision::DropDuplicate => {}
            }
        }
        Ok(out)
    }

    async fn apply_identity_rewrite(&self, rewrite: ConversationIdRewrite) -> Result<()> {
        if rewrite.from == rewrite.to {
            return Ok(());
        }
        let moved = self
            .message_store
            .rewrite_conversation_id(&rewrite.from, &rewrite.to)
            .await?;
        self.conversation_store
            .merge_conversation_identity(&rewrite.from, &rewrite.to)
            .await?;
        if moved > 0 {
            tracing::info!(
                from = %rewrite.from,
                to = %rewrite.to,
                moved,
                "canonicalized single chat conversation identity"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use flare_proto::common::{ConversationType as ProtoConversationType, Message as ProtoMessage};

    use crate::domain::conversation::generate_single_chat_conversation_id;
    use crate::domain::{ConversationReader, ConversationWriter, MessageReader, MessageWriter};
    use crate::infrastructure::persistence::memory_im::{
        MemoryConversationStore, MemoryMessageStore,
    };
    use crate::kernel::event::EventBus;
    use crate::model::conversation::ConversationType;
    use crate::model::{Conversation, IMMessage};

    fn single_message(server_id: &str, conversation_id: &str, peer: &str, seq: u64) -> IMMessage {
        IMMessage::new(ProtoMessage {
            server_id: server_id.to_string(),
            conversation_id: conversation_id.to_string(),
            client_msg_id: format!("client-{server_id}"),
            sender_id: peer.to_string(),
            conversation_seq: seq,
            conversation_type: ProtoConversationType::Single as i32,
            channel_id: peer.to_string(),
            created_at: 1_000 + seq as i64,
            ..Default::default()
        })
    }

    #[tokio::test]
    async fn canonicalizes_single_chat_messages_and_migrates_existing_local_rows() {
        let current_user = "me";
        let peer = "peer";
        let canonical = generate_single_chat_conversation_id(current_user, peer);
        let messages = Arc::new(MemoryMessageStore::new());
        let conversations = Arc::new(MemoryConversationStore::new());

        let old_message = single_message("old-server", peer, peer, 1);
        messages.save_one(&old_message).await.unwrap();

        let mut old_conversation = Conversation::from_conversation_id(peer.to_string());
        old_conversation.conversation_type = ConversationType::Single;
        old_conversation.channel_id = peer.to_string();
        old_conversation.max_seq = 1;
        conversations.save_one(&old_conversation).await.unwrap();

        let converger = IncomingMessageConverger::new(
            messages.clone(),
            conversations.clone(),
            EventBus::new(),
            None,
        );
        let incoming = single_message("new-server", peer, peer, 2);
        let fresh = converger
            .converge_messages(current_user, vec![incoming])
            .await
            .unwrap();

        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].conversation_id, canonical);
        assert_eq!(fresh[0].channel_id, peer);

        let moved = messages
            .get_by_conversation(&canonical, 0, 20)
            .await
            .unwrap();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].server_id, "old-server");
        assert!(
            messages
                .get_by_conversation(peer, 0, 20)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(conversations.get(peer).await.unwrap().is_none());
        assert!(conversations.get(&canonical).await.unwrap().is_some());
    }
}
