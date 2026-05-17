use std::sync::Arc;

use crate::domain::{ConversationStore, UserReader};
use crate::error::Result;
use crate::model::Conversation;

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
        if let Some(last) = conversation.last_message() {
            if !last.sender_id.is_empty() {
                if let Ok(Some(profile)) = self.profile_reader.get(&last.sender_id).await {
                    conversation =
                        conversation.with_last_sender(profile.display_name(), &profile.avatar_url);
                }
            }
        }
        conversation
    }

    pub async fn list(&self) -> Result<Vec<Conversation>> {
        let list = self.store.list().await?;
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
    ) -> Result<Vec<Conversation>> {
        let list = self.list().await?;
        let skip = cursor
            .and_then(|value| {
                list.iter()
                    .position(|conversation| conversation.conversation_id == value)
            })
            .map(|index| index + 1)
            .unwrap_or(0);
        let take = limit.map(|value| value as usize).unwrap_or(usize::MAX);
        Ok(list.into_iter().skip(skip).take(take).collect())
    }

    pub async fn list_raw(&self) -> Result<Vec<Conversation>> {
        self.store.list().await
    }
}
