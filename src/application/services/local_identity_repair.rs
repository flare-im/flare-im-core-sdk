use std::sync::Arc;

use crate::domain::{ConversationIdentityService, ConversationStore, MessageStore};
use crate::shared::error::Result;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct LocalIdentityRepairReport {
    pub scanned_conversations: u64,
    pub rewritten_conversations: u64,
    pub moved_messages: u64,
}

impl LocalIdentityRepairReport {
    fn record_rewrite(&mut self, moved_messages: u64) {
        self.rewritten_conversations += 1;
        self.moved_messages += moved_messages;
    }

    pub(crate) fn has_changes(self) -> bool {
        self.rewritten_conversations > 0 || self.moved_messages > 0
    }
}

pub(crate) struct LocalIdentityRepairService {
    messages: Arc<dyn MessageStore>,
    conversations: Arc<dyn ConversationStore>,
}

impl LocalIdentityRepairService {
    pub(crate) fn new(
        messages: Arc<dyn MessageStore>,
        conversations: Arc<dyn ConversationStore>,
    ) -> Self {
        Self {
            messages,
            conversations,
        }
    }

    pub(crate) async fn repair_single_chat_identities(
        &self,
        current_user_id: &str,
    ) -> Result<LocalIdentityRepairReport> {
        let current_user_id = current_user_id.trim();
        if current_user_id.is_empty() {
            return Ok(LocalIdentityRepairReport::default());
        }

        let mut report = LocalIdentityRepairReport::default();
        let conversations = self.conversations.list().await?;
        for conversation in conversations {
            report.scanned_conversations += 1;
            let mut canonical = conversation;
            let Some(rewrite) = ConversationIdentityService::canonicalize_single_chat_conversation(
                &mut canonical,
                current_user_id,
            ) else {
                continue;
            };

            let moved_messages = self
                .messages
                .rewrite_conversation_id(&rewrite.from, &rewrite.to)
                .await?;
            self.conversations
                .merge_conversation_identity(&rewrite.from, &rewrite.to)
                .await?;
            report.record_rewrite(moved_messages);
            tracing::info!(
                target: "flare_sdk.identity",
                from = %rewrite.from,
                to = %rewrite.to,
                moved_messages,
                "repaired local single chat conversation identity"
            );
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use flare_proto::common::{ConversationType as ProtoConversationType, Message as ProtoMessage};

    use super::LocalIdentityRepairService;
    use crate::domain::conversation::generate_single_chat_conversation_id;
    use crate::domain::{ConversationReader, ConversationWriter, MessageReader, MessageWriter};
    use crate::infrastructure::persistence::memory_im::{
        MemoryConversationStore, MemoryMessageStore,
    };
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
    async fn repairs_existing_single_chat_rows_without_new_incoming_message() {
        let current_user = "me";
        let peer = "peer";
        let stale_id = peer;
        let canonical_id = generate_single_chat_conversation_id(current_user, peer);
        let messages = Arc::new(MemoryMessageStore::new());
        let conversations = Arc::new(MemoryConversationStore::new());

        messages
            .save_one(&single_message("old-server", stale_id, peer, 7))
            .await
            .unwrap();

        let mut canonical = Conversation::from_conversation_id(canonical_id.clone());
        canonical.conversation_type = ConversationType::Single;
        canonical.channel_id = peer.to_string();
        canonical.max_seq = 1;
        conversations.save_one(&canonical).await.unwrap();

        let mut stale = Conversation::from_conversation_id(stale_id.to_string());
        stale.conversation_type = ConversationType::Single;
        stale.channel_id = peer.to_string();
        stale.max_seq = 7;
        stale.unread_count = 3;
        stale.is_pinned = true;
        conversations.save_one(&stale).await.unwrap();

        let service = LocalIdentityRepairService::new(messages.clone(), conversations.clone());
        let report = service
            .repair_single_chat_identities(current_user)
            .await
            .unwrap();

        assert_eq!(report.scanned_conversations, 2);
        assert_eq!(report.rewritten_conversations, 1);
        assert_eq!(report.moved_messages, 1);
        assert!(
            messages
                .get_by_conversation(stale_id, 0, 20)
                .await
                .unwrap()
                .is_empty()
        );
        let moved = messages
            .get_by_conversation(&canonical_id, 0, 20)
            .await
            .unwrap();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].server_id, "old-server");
        assert!(conversations.get(stale_id).await.unwrap().is_none());
        let repaired = conversations
            .get(&canonical_id)
            .await
            .unwrap()
            .expect("canonical conversation");
        assert_eq!(repaired.max_seq, 7);
        assert_eq!(repaired.unread_count, 3);
        assert!(repaired.is_pinned);
    }
}
