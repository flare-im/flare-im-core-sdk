use std::sync::Arc;

use crate::domain::{ConversationStore, UserReader};
use crate::model::{Conversation, ConversationListQuery};
use crate::shared::error::Result;
use tracing::warn;

pub struct ConversationViewAssembler {
    store: Arc<dyn ConversationStore>,
    profile_reader: Arc<dyn UserReader>,
}

impl ConversationViewAssembler {
    pub fn new(store: Arc<dyn ConversationStore>, profile_reader: Arc<dyn UserReader>) -> Self {
        Self {
            store,
            profile_reader,
        }
    }

    pub async fn hydrate_conversation(&self, mut conversation: Conversation) -> Conversation {
        let should_persist = conversation.normalize_channel_id_for_wire();
        if let Some(last) = conversation.last_message()
            && !last.sender_id.is_empty()
            && let Ok(Some(profile)) = self.profile_reader.get(&last.sender_id).await
        {
            conversation =
                conversation.with_last_sender(profile.display_name(), &profile.avatar_url);
        }
        if should_persist && let Err(error) = self.store.save_one(&conversation).await {
            warn!(
                conversation_id = %conversation.conversation_id,
                error = %error,
                "failed to persist repaired conversation channel_id"
            );
        }
        conversation
    }

    pub async fn list(&self, include_archived: bool) -> Result<Vec<Conversation>> {
        self.list_by_query(&ConversationListQuery {
            include_archived,
            ..ConversationListQuery::default()
        })
        .await
    }

    pub async fn list_by_query(&self, query: &ConversationListQuery) -> Result<Vec<Conversation>> {
        let mut list = self.store.list_by_query(query).await?;
        if let Some(cursor) = query
            .cursor
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let skip = list
                .iter()
                .position(|conversation| conversation.conversation_id == cursor)
                .map(|index| index + 1)
                .unwrap_or(0);
            list = list.into_iter().skip(skip).collect();
        }
        if let Some(limit) = query.normalized_limit() {
            list.truncate(limit as usize);
        }
        let mut views = Vec::with_capacity(list.len());
        for conversation in list {
            views.push(self.hydrate_conversation(conversation).await);
        }
        Ok(views)
    }

    pub async fn get(&self, conversation_id: &str) -> Result<Option<Conversation>> {
        let conversation = self.store.get(conversation_id).await?;
        let Some(conversation) = conversation else {
            return Ok(None);
        };
        Ok(Some(self.hydrate_conversation(conversation).await))
    }

    pub async fn get_multiple(&self, conversation_ids: &[String]) -> Result<Vec<Conversation>> {
        let mut out = Vec::with_capacity(conversation_ids.len());
        for conversation_id in conversation_ids {
            let existing = self.store.get(conversation_id).await?;
            if let Some(conversation) = existing {
                out.push(self.hydrate_conversation(conversation).await);
            }
        }
        Ok(out)
    }

    pub async fn list_paginated(
        &self,
        cursor: Option<&str>,
        limit: Option<u32>,
        include_archived: bool,
    ) -> Result<Vec<Conversation>> {
        self.list_by_query(&ConversationListQuery {
            include_archived,
            cursor: cursor.map(str::to_string),
            limit,
            ..ConversationListQuery::default()
        })
        .await
    }

    pub async fn list_raw(&self) -> Result<Vec<Conversation>> {
        self.store.list().await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::domain::{ConversationReader, ConversationWriter};
    use crate::infrastructure::persistence::MemoryUserProfileStore;
    use crate::model::conversation::ConversationType;
    use crate::storage::MemoryConversationStore;

    use super::*;

    #[tokio::test]
    async fn list_repairs_and_persists_blank_channel_id_before_wire_decode() {
        let store = Arc::new(MemoryConversationStore::new());
        let profile_reader = Arc::new(MemoryUserProfileStore::new());
        store
            .save_one(&Conversation {
                conversation_id: "2AGROUPCIDVALUE01".to_string(),
                conversation_type: ConversationType::Group,
                channel_id: String::new(),
                display_name: "Group".to_string(),
                ..Default::default()
            })
            .await
            .expect("seed broken local conversation");

        let assembler = ConversationViewAssembler::new(store.clone(), profile_reader);
        let list = assembler.list(false).await.expect("list conversations");

        assert_eq!(list.len(), 1);
        assert_eq!(list[0].channel_id, "2AGROUPCIDVALUE01");

        let persisted = store
            .get("2AGROUPCIDVALUE01")
            .await
            .expect("load repaired conversation")
            .expect("conversation exists");
        assert_eq!(persisted.channel_id, "2AGROUPCIDVALUE01");
    }
}
