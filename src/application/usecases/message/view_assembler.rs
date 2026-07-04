use std::sync::Arc;

use crate::domain::{MessageStore, UserReader};
use crate::model::{IMMessage, MessageSearchQuery};
use crate::shared::error::Result;

pub struct MessageViewAssembler {
    store: Arc<dyn MessageStore>,
    profile_reader: Arc<dyn UserReader>,
}

impl MessageViewAssembler {
    pub fn new(store: Arc<dyn MessageStore>, profile_reader: Arc<dyn UserReader>) -> Self {
        Self {
            store,
            profile_reader,
        }
    }

    pub async fn get(&self, message_id: &str) -> Result<Option<IMMessage>> {
        let msg = match self.store.get_by_client_msg_id(message_id).await? {
            Some(message) => Some(message),
            None => self.store.get(message_id).await?,
        };
        let Some(mut view) = msg else {
            return Ok(None);
        };
        self.hydrate_reactions_for_messages(std::slice::from_mut(&mut view))
            .await?;
        self.fill_sender_profile(&mut view).await;
        Ok(Some(view))
    }

    pub async fn get_raw(&self, message_id: &str) -> Result<Option<IMMessage>> {
        let mut msg = match self.store.get_by_client_msg_id(message_id).await? {
            Some(message) => Some(message),
            None => self.store.get(message_id).await?,
        };
        if let Some(view) = msg.as_mut() {
            self.hydrate_reactions_for_messages(std::slice::from_mut(view))
                .await?;
        }
        Ok(msg)
    }

    pub async fn list(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<IMMessage>> {
        let mut views = self
            .store
            .get_by_conversation(conversation_id, before_seq, limit)
            .await?;
        self.hydrate_messages_for_view(&mut views).await?;
        Ok(views)
    }

    pub async fn hydrate_messages_for_view(&self, views: &mut [IMMessage]) -> Result<()> {
        self.hydrate_reactions_for_messages(views).await?;
        self.fill_sender_profiles_for_messages(views).await?;
        Ok(())
    }

    /// 批量填充发送者资料：去重所有 sender_id → **单次** `get_many` → 按 map 回填。
    /// 取代此前的逐条 `fill_sender_profile`（N+1：每条消息一次 SQLite 查询），是打开大时间线慢/超时的主因。
    async fn fill_sender_profiles_for_messages(&self, views: &mut [IMMessage]) -> Result<()> {
        let mut sender_ids: Vec<String> = views
            .iter()
            .map(|v| v.sender_id().trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        sender_ids.sort();
        sender_ids.dedup();
        if sender_ids.is_empty() {
            return Ok(());
        }
        let profiles = self.profile_reader.get_many(&sender_ids).await?;
        for view in views.iter_mut() {
            if let Some(profile) = profiles.get(view.sender_id().trim()) {
                *view = view.clone().with_sender_profile(profile.display_name());
            }
        }
        Ok(())
    }

    pub async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<IMMessage>> {
        self.search_by_query(&MessageSearchQuery::text(keyword, limit))
            .await
    }

    pub async fn search_in_conversation(
        &self,
        conversation_id: &str,
        keyword: &str,
        limit: u32,
    ) -> Result<Vec<IMMessage>> {
        self.search_by_query(&MessageSearchQuery::in_conversation(
            conversation_id,
            keyword,
            limit,
        ))
        .await
    }

    pub async fn search_by_query(&self, query: &MessageSearchQuery) -> Result<Vec<IMMessage>> {
        let mut views = self.store.search_by_query(query).await?;
        self.hydrate_messages_for_view(&mut views).await?;
        Ok(views)
    }

    async fn hydrate_reactions_for_messages(&self, views: &mut [IMMessage]) -> Result<()> {
        let mut message_ids: Vec<String> = Vec::with_capacity(views.len() * 2);
        for message in views.iter() {
            let sid = message.server_id.trim();
            if !sid.is_empty() {
                message_ids.push(sid.to_string());
            }
            let cid = message.client_msg_id.trim();
            if !cid.is_empty() {
                message_ids.push(cid.to_string());
            }
        }
        message_ids.sort();
        message_ids.dedup();
        if message_ids.is_empty() {
            return Ok(());
        }
        let reaction_map = self.store.list_reactions(&message_ids).await?;
        for view in views.iter_mut() {
            let sid = view.server_id.trim();
            let cid = view.client_msg_id.trim();
            let reactions = if !sid.is_empty() {
                reaction_map.get(sid)
            } else {
                None
            }
            .or_else(|| {
                if cid.is_empty() {
                    None
                } else {
                    reaction_map.get(cid)
                }
            });
            if let Some(reactions) = reactions {
                view.reactions = reactions.clone();
            }
        }
        Ok(())
    }

    async fn fill_sender_profile(&self, view: &mut IMMessage) {
        if let Ok(Some(profile)) = self.profile_reader.get(view.sender_id()).await {
            *view = view.clone().with_sender_profile(profile.display_name());
        }
    }
}
