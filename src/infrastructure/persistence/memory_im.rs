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

    fn storage_key(message: &IMMessage) -> String {
        if !message.server_id.is_empty() {
            message.server_id.clone()
        } else {
            message.client_msg_id.clone()
        }
    }

    fn remove_conflicting_client_msg_rows(
        data: &mut HashMap<String, IMMessage>,
        client_msg_id: &str,
        keep_key: &str,
    ) {
        let client_msg_id = client_msg_id.trim();
        if client_msg_id.is_empty() {
            return;
        }
        data.retain(|key, stored| key == keep_key || stored.client_msg_id != client_msg_id);
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
                .filter(|m| m.conversation_id == conversation_id && m.conversation_seq < bound)
                .cloned()
                .collect()
        } else {
            data.values()
                .filter(|m| {
                    m.conversation_id == conversation_id
                        && m.conversation_seq > 0
                        && m.conversation_seq < bound
                })
                .cloned()
                .collect()
        };
        if is_latest {
            msgs.sort_by(IMMessage::compare_for_latest_window_desc);
        } else {
            msgs.sort_by(|a, b| b.conversation_seq.cmp(&a.conversation_seq));
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
                    .attributes
                    .get("contentText")
                    .is_some_and(|t| t.to_lowercase().contains(&kw));
                let from_preview = m
                    .text_for_storage()
                    .is_some_and(|t| t.to_lowercase().contains(&kw));
                from_extra || from_preview
            })
            .cloned()
            .collect();
        results.sort_by(|a, b| b.conversation_seq.cmp(&a.conversation_seq));
        results.truncate(limit as usize);
        Ok(results)
    }
}

#[async_trait]
impl MessageWriter for MemoryMessageStore {
    async fn save_batch(&self, messages: &[IMMessage]) -> Result<()> {
        let mut data = self.data.write().await;
        for msg in messages {
            let key = Self::storage_key(msg);
            Self::remove_conflicting_client_msg_rows(&mut data, &msg.client_msg_id, &key);
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
            msg.encoded_content = new_content.clone();
            msg.is_edited = true;
            msg.content = decode_content_bytes(&msg.encoded_content)
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
        let mut message = message.clone();
        let ack_client_msg_id = client_msg_id.trim();
        if message.client_msg_id.trim().is_empty() && !ack_client_msg_id.is_empty() {
            message.client_msg_id = ack_client_msg_id.to_string();
        }
        let key = Self::storage_key(&message);
        Self::remove_conflicting_client_msg_rows(&mut data, ack_client_msg_id, &key);
        Self::remove_conflicting_client_msg_rows(&mut data, &message.client_msg_id, &key);
        data.retain(|stored_key, _| stored_key == &key || stored_key != ack_client_msg_id);
        data.insert(key, message);
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

#[cfg(test)]
mod tests {
    use super::MemoryMessageStore;
    use crate::domain::{MessageReader, MessageWriter};
    use crate::model::IMMessage;
    use flare_proto::common::MessageStatus;

    fn local_message(server_id: &str, client_msg_id: &str) -> IMMessage {
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = server_id.to_string();
        message.client_msg_id = client_msg_id.to_string();
        message.conversation_id = "conv-memory-dupe".to_string();
        message.sender_id = "u1".to_string();
        message.local_state.sending = true;
        message.local_state.is_local = true;
        message
    }

    #[tokio::test]
    async fn save_batch_collapses_existing_client_msg_id() {
        let store = MemoryMessageStore::new();
        let pending = local_message("client-memory-1", "client-memory-1");
        store.save_batch(&[pending.clone()]).await.unwrap();

        let mut echoed = pending;
        echoed.server_id = "server-memory-1".to_string();
        echoed.conversation_seq = 10;
        echoed.status = MessageStatus::Persisted as i32;
        echoed.local_state.sending = false;
        echoed.local_state.is_local = false;
        store.save_batch(&[echoed]).await.unwrap();

        assert!(store.get("client-memory-1").await.unwrap().is_none());
        let stored = store
            .get_by_client_msg_id("client-memory-1")
            .await
            .unwrap()
            .expect("canonical message");
        assert_eq!(stored.server_id, "server-memory-1");

        let timeline = store
            .get_by_conversation("conv-memory-dupe", 0, 10)
            .await
            .unwrap();
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0].server_id, "server-memory-1");
    }

    #[tokio::test]
    async fn update_after_ack_collapses_existing_client_msg_id() {
        let store = MemoryMessageStore::new();
        let stale = local_message("stale-memory-1", "client-memory-ack-1");
        store.save_batch(&[stale.clone()]).await.unwrap();

        let mut acked = stale;
        acked.server_id = "server-memory-ack-1".to_string();
        acked.conversation_seq = 20;
        acked.status = MessageStatus::Sent as i32;
        store
            .update_after_ack("client-memory-ack-1", &acked)
            .await
            .unwrap();

        assert!(store.get("stale-memory-1").await.unwrap().is_none());
        let stored = store.get("server-memory-ack-1").await.unwrap().unwrap();
        assert_eq!(stored.client_msg_id, "client-memory-ack-1");

        let timeline = store
            .get_by_conversation("conv-memory-dupe", 0, 10)
            .await
            .unwrap();
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0].server_id, "server-memory-ack-1");
    }
}
