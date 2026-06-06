//! 浏览器 / 联调用内存 IM 仓储（Message + Conversation + PendingSend）。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::StoreProvider;
use super::empty_stores::MemorySyncCursorStore;
use super::memory::{MemoryPendingSendStore, MemoryUserProfileStore};
use crate::domain::{
    ConversationReader, ConversationWriter, MessageReader, MessageStore, MessageWriter,
};
use crate::model::Conversation;
use crate::model::IMMessage;
use crate::model::{decode_content_bytes, decoded_content_to_elem};
use crate::shared::error::Result;
use flare_proto::common::MessageStatus;

pub struct MemoryMessageStore {
    data: RwLock<HashMap<String, IMMessage>>,
}

impl MemoryMessageStore {
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryMessageStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MessageReader for MemoryMessageStore {
    async fn get(&self, message_id: &str) -> Result<Option<IMMessage>> {
        Ok(self.data.read().await.get(message_id).cloned())
    }

    async fn get_by_client_msg_id(&self, client_msg_id: &str) -> Result<Option<IMMessage>> {
        Ok(self
            .data
            .read()
            .await
            .values()
            .find(|m| m.client_msg_id == client_msg_id)
            .cloned())
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<IMMessage>> {
        let data = self.data.read().await;
        let bound = if before_seq == 0 {
            u64::MAX
        } else {
            before_seq
        };
        let is_latest = before_seq == 0 || before_seq >= i64::MAX as u64;
        let mut msgs: Vec<_> = if is_latest {
            data.values()
                .filter(|m| m.conversation_id == conversation_id && m.seq < bound)
                .cloned()
                .collect()
        } else {
            data.values()
                .filter(|m| m.conversation_id == conversation_id && m.seq > 0 && m.seq < bound)
                .cloned()
                .collect()
        };
        if is_latest {
            let key = |m: &IMMessage| {
                m.local_state
                    .sort_ts
                    .max(m.timestamp)
                    .max(m.client_timestamp)
            };
            msgs.sort_by(|a, b| key(b).cmp(&key(a)).then_with(|| b.seq.cmp(&a.seq)));
        } else {
            msgs.sort_by(|a, b| b.seq.cmp(&a.seq));
        }
        msgs.truncate(limit as usize);
        Ok(msgs)
    }

    async fn search(&self, _keyword: &str, limit: u32) -> Result<Vec<IMMessage>> {
        Ok(self
            .data
            .read()
            .await
            .values()
            .take(limit as usize)
            .cloned()
            .collect())
    }

    async fn search_in_conversation(
        &self,
        conversation_id: &str,
        keyword: &str,
        limit: u32,
    ) -> Result<Vec<IMMessage>> {
        let kw = keyword.trim().to_lowercase();
        let data = self.data.read().await;
        let mut results: Vec<_> = data
            .values()
            .filter(|m| {
                if m.conversation_id != conversation_id {
                    return false;
                }
                let from_extra = m
                    .extra
                    .get("contentText")
                    .is_some_and(|t| t.to_lowercase().contains(&kw));
                let from_preview = m
                    .text_for_storage()
                    .is_some_and(|t| t.to_lowercase().contains(&kw));
                from_extra || from_preview
            })
            .cloned()
            .collect();
        results.sort_by(|a, b| b.seq.cmp(&a.seq));
        results.truncate(limit as usize);
        Ok(results)
    }
}

#[async_trait]
impl MessageWriter for MemoryMessageStore {
    async fn save_batch(&self, messages: &[IMMessage]) -> Result<()> {
        let mut data = self.data.write().await;
        for msg in messages {
            let key = if !msg.server_id.is_empty() {
                msg.server_id.clone()
            } else {
                msg.client_msg_id.clone()
            };
            data.insert(key, msg.clone());
        }
        Ok(())
    }

    async fn save_one(&self, message: &IMMessage) -> Result<()> {
        MessageWriter::save_batch(self, std::slice::from_ref(message)).await
    }

    async fn update_status(&self, message_id: &str, status: i32) -> Result<()> {
        let mut data = self.data.write().await;
        let recalled = MessageStatus::Recalled as i32;
        let apply = |msg: &mut IMMessage| {
            msg.status = status;
            if status == recalled {
                msg.is_recalled = true;
            }
        };
        if let Some(msg) = data.get_mut(message_id) {
            apply(msg);
            return Ok(());
        }
        for msg in data.values_mut() {
            if msg.server_id == message_id || msg.client_msg_id == message_id {
                apply(msg);
                break;
            }
        }
        Ok(())
    }

    async fn update_content(&self, message_id: &str, new_content: Vec<u8>) -> Result<bool> {
        let mut data = self.data.write().await;
        let mut hit = false;
        let mut apply = |msg: &mut IMMessage| {
            msg.content_bytes = new_content.clone();
            msg.is_edited = true;
            msg.content = decode_content_bytes(&msg.content_bytes)
                .ok()
                .and_then(|d| decoded_content_to_elem(&d));
            hit = true;
        };
        if let Some(msg) = data.get_mut(message_id) {
            apply(msg);
            return Ok(hit);
        }
        for msg in data.values_mut() {
            if msg.server_id == message_id || msg.client_msg_id == message_id {
                apply(msg);
                break;
            }
        }
        Ok(hit)
    }

    async fn delete(&self, message_id: &str) -> Result<()> {
        self.data.write().await.remove(message_id);
        Ok(())
    }

    async fn update_after_ack(&self, client_msg_id: &str, message: &IMMessage) -> Result<()> {
        let mut data = self.data.write().await;
        data.remove(client_msg_id);
        let key = if !message.server_id.is_empty() {
            message.server_id.clone()
        } else {
            message.client_msg_id.clone()
        };
        data.insert(key, message.clone());
        Ok(())
    }
}

impl MessageStore for MemoryMessageStore {}

pub struct MemoryConversationStore {
    data: RwLock<HashMap<String, Conversation>>,
}

impl MemoryConversationStore {
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryConversationStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ConversationReader for MemoryConversationStore {
    async fn get(&self, conversation_id: &str) -> Result<Option<Conversation>> {
        Ok(self.data.read().await.get(conversation_id).cloned())
    }

    async fn list(&self) -> Result<Vec<Conversation>> {
        let mut list: Vec<Conversation> = self.data.read().await.values().cloned().collect();
        list.sort_by(|a, b| {
            match (
                b.is_pinned.cmp(&a.is_pinned),
                b.last_message_at.cmp(&a.last_message_at),
            ) {
                (std::cmp::Ordering::Equal, o) => o,
                (p, _) => p,
            }
        });
        Ok(list)
    }
}

#[async_trait]
impl ConversationWriter for MemoryConversationStore {
    async fn save_batch(&self, conversations: &[Conversation]) -> Result<()> {
        let mut data = self.data.write().await;
        for conv in conversations {
            data.insert(conv.conversation_id.clone(), conv.clone());
        }
        Ok(())
    }

    async fn save_one(&self, conversation: &Conversation) -> Result<()> {
        ConversationWriter::save_batch(self, std::slice::from_ref(conversation)).await
    }

    async fn update_unread(
        &self,
        conversation_id: &str,
        unread_count: u32,
        last_read_seq: u64,
    ) -> Result<()> {
        let mut data = self.data.write().await;
        if let Some(conv) = data.get(conversation_id) {
            let mut updated = conv.clone();
            updated.unread_count = unread_count;
            updated.last_read_seq = last_read_seq;
            data.insert(conversation_id.to_string(), updated);
        }
        Ok(())
    }

    async fn set_pinned(&self, conversation_id: &str, pinned: bool) -> Result<()> {
        let mut data = self.data.write().await;
        if let Some(conv) = data.get(conversation_id) {
            let mut updated = conv.clone();
            updated.is_pinned = pinned;
            data.insert(conversation_id.to_string(), updated);
        }
        Ok(())
    }

    async fn set_muted(&self, conversation_id: &str, muted: bool) -> Result<()> {
        let mut data = self.data.write().await;
        if let Some(conv) = data.get(conversation_id) {
            let mut updated = conv.clone();
            updated.is_muted = muted;
            data.insert(conversation_id.to_string(), updated);
        }
        Ok(())
    }

    async fn set_archived(&self, conversation_id: &str, archived: bool) -> Result<()> {
        let mut data = self.data.write().await;
        if let Some(conv) = data.get(conversation_id) {
            let mut updated = conv.clone();
            updated.is_archived = archived;
            data.insert(conversation_id.to_string(), updated);
        }
        Ok(())
    }

    async fn mark_unread(&self, conversation_id: &str) -> Result<u32> {
        let mut data = self.data.write().await;
        if let Some(conv) = data.get(conversation_id) {
            let mut updated = conv.clone();
            if updated.max_seq > 0 {
                updated.last_read_seq = updated.max_seq.saturating_sub(1);
            }
            updated.unread_count = 1;
            data.insert(conversation_id.to_string(), updated.clone());
            return Ok(updated.unread_count);
        }
        Ok(0)
    }

    async fn update_draft(&self, conversation_id: &str, draft: Option<&str>) -> Result<()> {
        let mut data = self.data.write().await;
        if let Some(conv) = data.get(conversation_id) {
            let mut updated = conv.clone();
            updated.draft = draft.map(String::from);
            data.insert(conversation_id.to_string(), updated);
        }
        Ok(())
    }

    async fn delete(&self, conversation_id: &str) -> Result<()> {
        self.data.write().await.remove(conversation_id);
        Ok(())
    }

    async fn clear_local_chat_history(
        &self,
        conversation_id: &str,
        cleared_through_seq: u64,
    ) -> Result<()> {
        let mut data = self.data.write().await;
        if let Some(conv) = data.get_mut(conversation_id) {
            conv.last_message_id = None;
            conv.last_message_preview = None;
            conv.last_message_at = None;
            conv.unread_count = 0;
            if cleared_through_seq > 0 {
                crate::domain::set_local_cleared_through_seq(&mut conv.ext, cleared_through_seq);
            }
        }
        Ok(())
    }

    async fn update_last_message(
        &self,
        conversation_id: &str,
        last_message_id: &str,
        last_sender_id: &str,
        last_message_at: u64,
        last_message_preview: Option<&str>,
        max_seq: u64,
    ) -> Result<()> {
        let mut data = self.data.write().await;
        if let Some(c) = data.get_mut(conversation_id) {
            c.last_message_id = Some(last_message_id.to_string());
            c.last_sender_id = Some(last_sender_id.to_string());
            c.last_message_at = Some(last_message_at);
            c.last_message_preview = last_message_preview.map(String::from);
            c.max_seq = max_seq;
        }
        Ok(())
    }

    async fn recompute_unread_for_user(
        &self,
        _conversation_id: &str,
        _current_user_id: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn get_local_max_seq(&self, conversation_id: &str) -> Result<u64> {
        Ok(self
            .data
            .read()
            .await
            .get(conversation_id)
            .map(|conv| conv.max_seq)
            .unwrap_or_default())
    }
}

/// 完整内存 IM 仓储（Web production runtime 默认路径）。
pub fn in_memory_im_provider() -> StoreProvider {
    let pending = Arc::new(MemoryPendingSendStore::new());
    let user_profiles = Arc::new(MemoryUserProfileStore::new());
    StoreProvider {
        messages: Arc::new(MemoryMessageStore::new()),
        conversations: Arc::new(MemoryConversationStore::new()),
        conversation_participants: None,
        cursors: Arc::new(MemorySyncCursorStore::new()),
        pending_send_reader: Some(pending.clone()),
        pending_send_writer: Some(pending),
        upload_manifest_store: None,
        media_cache_store: None,
        media_cache_admin: None,
        user_file_download_store: None,
        user_profiles_reader: Some(user_profiles.clone()),
        user_profiles_writer: Some(user_profiles),
    }
}
