use std::collections::{HashMap, HashSet};
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
        let (mut messages, rewrites) = canonicalize_incoming_messages(messages, current_user_id);
        for rewrite in rewrites {
            self.apply_identity_rewrite(rewrite).await?;
        }
        for message in &mut messages {
            MessageDeliveryService::normalize_incoming_server_state(message);
        }
        let local_by_client = self.local_by_client_msg_id(&messages).await?;
        let local_by_server = self.local_by_server_id(&messages).await?;
        let mut out = Vec::with_capacity(messages.len());
        for message in messages {
            let local_by_client = trimmed_lookup(&local_by_client, &message.client_msg_id);
            let local_by_server = trimmed_lookup(&local_by_server, &message.server_id);
            match MessageDeliveryService::decide_incoming_message_convergence(
                current_user_id,
                &message,
                local_by_client,
                local_by_server,
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
                            local_by_client,
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
                IncomingMessageConvergenceDecision::DropDuplicate => {
                    if let Some(local) = local_by_server
                        && message.status > local.status
                        && !message.server_id.trim().is_empty()
                    {
                        self.message_store
                            .update_status(&message.server_id, message.status)
                            .await?;
                    }
                }
            }
        }
        Ok(out)
    }

    async fn local_by_client_msg_id(
        &self,
        messages: &[IMMessage],
    ) -> Result<HashMap<String, IMMessage>> {
        let ids = unique_non_empty_ids(
            messages
                .iter()
                .map(|message| message.client_msg_id.as_str()),
        );
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        Ok(self
            .message_store
            .get_by_client_msg_ids(&ids)
            .await?
            .into_iter()
            .filter_map(|message| keyed_message(message.client_msg_id.clone(), message))
            .collect())
    }

    async fn local_by_server_id(
        &self,
        messages: &[IMMessage],
    ) -> Result<HashMap<String, IMMessage>> {
        let ids = unique_non_empty_ids(messages.iter().map(|message| message.server_id.as_str()));
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        Ok(self
            .message_store
            .get_by_message_ids(&ids)
            .await?
            .into_iter()
            .filter_map(|message| keyed_message(message.server_id.clone(), message))
            .collect())
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

fn canonicalize_incoming_messages(
    messages: Vec<IMMessage>,
    current_user_id: &str,
) -> (Vec<IMMessage>, Vec<ConversationIdRewrite>) {
    let mut seen = HashSet::new();
    let mut rewrites = Vec::new();
    let mut out = Vec::with_capacity(messages.len());
    for mut message in messages {
        let Some(rewrite) = ConversationIdentityService::canonicalize_single_chat_message(
            &mut message,
            current_user_id,
        ) else {
            out.push(message);
            continue;
        };
        let key = (rewrite.from.clone(), rewrite.to.clone());
        if seen.insert(key) {
            rewrites.push(rewrite);
        }
        out.push(message);
    }
    (out, rewrites)
}

fn unique_non_empty_ids<'a>(ids: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.filter_map(|id| {
        let id = id.trim();
        if id.is_empty() || !seen.insert(id.to_string()) {
            None
        } else {
            Some(id.to_string())
        }
    })
    .collect()
}

fn keyed_message(key: String, message: IMMessage) -> Option<(String, IMMessage)> {
    let key = key.trim();
    if key.is_empty() {
        None
    } else {
        Some((key.to_string(), message))
    }
}

fn trimmed_lookup<'a>(map: &'a HashMap<String, IMMessage>, key: &str) -> Option<&'a IMMessage> {
    let key = key.trim();
    if key.is_empty() { None } else { map.get(key) }
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
    use crate::model::message::MessageStatus;
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

    #[tokio::test]
    async fn duplicate_server_message_refreshes_lower_local_status() {
        let current_user = "me";
        let peer = "peer";
        let conversation_id = generate_single_chat_conversation_id(current_user, peer);
        let messages = Arc::new(MemoryMessageStore::new());
        let conversations = Arc::new(MemoryConversationStore::new());

        let mut local = single_message("server-1", &conversation_id, peer, 7);
        local.status = MessageStatus::Created as i32;
        messages.save_one(&local).await.unwrap();

        let mut incoming = single_message("server-1", &conversation_id, peer, 7);
        incoming.status = MessageStatus::Created as i32;
        let converger =
            IncomingMessageConverger::new(messages.clone(), conversations, EventBus::new(), None);

        let fresh = converger
            .converge_messages(current_user, vec![incoming])
            .await
            .unwrap();

        assert!(fresh.is_empty());
        let stored = messages
            .get_by_message_ids(&["server-1".to_string()])
            .await
            .unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].status, MessageStatus::Sent as i32);
    }
}
