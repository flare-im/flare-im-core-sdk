use std::sync::Arc;

use crate::domain::{ConversationStore, UserReader};
use crate::model::{Conversation, ConversationListQuery};
use crate::shared::error::Result;

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
        if let Some(last) = conversation.last_message()
            && !last.sender_id.is_empty()
            && let Ok(Some(profile)) = self.profile_reader.get(&last.sender_id).await
        {
            conversation =
                conversation.with_last_sender(profile.display_name(), &profile.avatar_url);
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
