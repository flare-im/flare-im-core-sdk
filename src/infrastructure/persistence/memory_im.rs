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
    OperationApplyResult, merge_incoming_conversation_summary, merge_message_event_attributes,
    message_attribute_seq,
};
use crate::model::Conversation;
use crate::model::IMMessage;
use crate::model::message::ReactionEntry;
use crate::model::{decode_content_bytes, decoded_content_to_elem};
use crate::shared::error::{ErrorCode, FlareError, Result};
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

    fn get_mut_by_any_message_id<'a>(
        data: &'a mut HashMap<String, IMMessage>,
        message_id: &str,
    ) -> Option<&'a mut IMMessage> {
        if data.contains_key(message_id) {
            return data.get_mut(message_id);
        }
        data.values_mut()
            .find(|message| message.server_id == message_id || message.client_msg_id == message_id)
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
            let existing_attributes = data
                .get(&key)
                .or_else(|| {
                    data.values().find(|stored| {
                        (!msg.server_id.trim().is_empty() && stored.server_id == msg.server_id)
                            || (!msg.client_msg_id.trim().is_empty()
                                && stored.client_msg_id == msg.client_msg_id)
                    })
                })
                .map(|stored| stored.attributes.clone());
            let mut next = msg.clone();
            if let Some(existing_attributes) = existing_attributes {
                next.attributes =
                    merge_message_event_attributes(next.attributes, existing_attributes);
            }
            Self::remove_conflicting_client_msg_rows(&mut data, &msg.client_msg_id, &key);
            data.insert(key, next);
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

    async fn rewrite_conversation_id(
        &self,
        from_conversation_id: &str,
        to_conversation_id: &str,
    ) -> Result<u64> {
        let from = from_conversation_id.trim();
        let to = to_conversation_id.trim();
        if from.is_empty() || to.is_empty() || from == to {
            return Ok(0);
        }
        let mut count = 0;
        for message in self.data.write().await.values_mut() {
            if message.conversation_id == from {
                message.conversation_id = to.to_string();
                count += 1;
            }
        }
        Ok(count)
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

#[async_trait]
impl MessageStore for MemoryMessageStore {
    async fn mark_outgoing_read_upto_seq(
        &self,
        conversation_id: &str,
        sender_user_id: &str,
        read_seq: u64,
    ) -> Result<()> {
        let conversation_id = conversation_id.trim();
        let sender_user_id = sender_user_id.trim();
        if conversation_id.is_empty() || sender_user_id.is_empty() || read_seq == 0 {
            return Ok(());
        }

        let created = MessageStatus::Created as i32;
        let sent = MessageStatus::Sent as i32;
        let persisted = MessageStatus::Persisted as i32;
        let mut data = self.data.write().await;
        for message in data.values_mut() {
            if message.conversation_id == conversation_id
                && message.sender_id == sender_user_id
                && message.conversation_seq > 0
                && message.conversation_seq <= read_seq
                && matches!(message.status, status if status == created || status == sent || status == persisted)
            {
                if message.status == created {
                    message.status = sent;
                }
                message.is_read = true;
            }
        }
        Ok(())
    }

    async fn reconcile_outgoing_read_by_peer_seq(
        &self,
        conversation_id: &str,
        sender_user_id: &str,
        peer_read_seq: u64,
    ) -> Result<()> {
        let conversation_id = conversation_id.trim();
        let sender_user_id = sender_user_id.trim();
        if conversation_id.is_empty() || sender_user_id.is_empty() {
            return Ok(());
        }
        if peer_read_seq > 0 {
            self.mark_outgoing_read_upto_seq(conversation_id, sender_user_id, peer_read_seq)
                .await?;
        }

        let mut data = self.data.write().await;
        for message in data.values_mut() {
            if message.conversation_id == conversation_id
                && message.sender_id == sender_user_id
                && message.conversation_seq > peer_read_seq
                && message.is_read
            {
                message.is_read = false;
            }
        }
        Ok(())
    }

    async fn apply_reaction(
        &self,
        _conversation_id: &str,
        message_server_id: &str,
        user_id: &str,
        emoji: &str,
        action: i32,
    ) -> Result<()> {
        let target = message_server_id.trim();
        if target.is_empty() || user_id.trim().is_empty() || emoji.trim().is_empty() {
            return Ok(());
        }
        let mut data = self.data.write().await;
        if let Some(message) = data.get_mut(target) {
            message.apply_reaction_change(user_id.trim(), emoji.trim(), action);
            return Ok(());
        }
        for message in data.values_mut() {
            if message.server_id == target || message.client_msg_id == target {
                message.apply_reaction_change(user_id.trim(), emoji.trim(), action);
                break;
            }
        }
        Ok(())
    }

    async fn list_reactions(
        &self,
        message_server_ids: &[String],
    ) -> Result<HashMap<String, Vec<ReactionEntry>>> {
        let data = self.data.read().await;
        let mut out = HashMap::new();
        for id in message_server_ids {
            let target = id.trim();
            if target.is_empty() || out.contains_key(target) {
                continue;
            }
            let message = data.get(target).or_else(|| {
                data.values()
                    .find(|message| message.server_id == target || message.client_msg_id == target)
            });
            let Some(message) = message else {
                continue;
            };
            if message.reactions.is_empty() {
                continue;
            }
            if !message.server_id.trim().is_empty() {
                out.insert(message.server_id.clone(), message.reactions.clone());
            }
            if !message.client_msg_id.trim().is_empty() {
                out.insert(message.client_msg_id.clone(), message.reactions.clone());
            }
        }
        Ok(out)
    }

    async fn set_message_flag(
        &self,
        message_id: &str,
        flag_key: &str,
        enabled: bool,
    ) -> Result<()> {
        let target = message_id.trim();
        let key = flag_key.trim();
        if target.is_empty() || key.is_empty() {
            return Ok(());
        }
        let value = if enabled { "true" } else { "false" }.to_string();
        let mut data = self.data.write().await;
        if let Some(message) = Self::get_mut_by_any_message_id(&mut data, target) {
            message.attributes.insert(key.to_string(), value);
        }
        Ok(())
    }

    async fn apply_pin_event(
        &self,
        message_id: &str,
        enabled: bool,
        event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        let target = message_id.trim();
        if target.is_empty() {
            return Ok(OperationApplyResult::NotFound);
        }
        let mut data = self.data.write().await;
        let Some(message) = Self::get_mut_by_any_message_id(&mut data, target) else {
            return Ok(OperationApplyResult::NotFound);
        };
        if let Some(seq) = event_seq {
            let current_seq = message_attribute_seq(&message.attributes, "lastPinEventSeq");
            if seq < current_seq {
                return Ok(OperationApplyResult::IgnoredStale);
            }
            message
                .attributes
                .insert("lastPinEventSeq".to_string(), seq.to_string());
        }
        message.attributes.insert(
            "pinned".to_string(),
            if enabled { "true" } else { "false" }.to_string(),
        );
        Ok(OperationApplyResult::Applied)
    }
}

pub struct MemoryConversationStore {
    data: RwLock<HashMap<String, Conversation>>,
    materialized_max_seq: RwLock<HashMap<String, u64>>,
}

impl MemoryConversationStore {
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
            materialized_max_seq: RwLock::new(HashMap::new()),
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
        for conversation in conversations {
            if conversation.conversation_id.trim().is_empty() {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "conversationId 不能为空",
                ));
            }
        }

        let mut data = self.data.write().await;
        for conv in conversations {
            let merged = merge_incoming_conversation_summary(data.get(&conv.conversation_id), conv);
            data.insert(merged.conversation_id.clone(), merged);
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
        self.materialized_max_seq
            .write()
            .await
            .remove(conversation_id);
        Ok(())
    }

    async fn merge_conversation_identity(
        &self,
        from_conversation_id: &str,
        to_conversation_id: &str,
    ) -> Result<()> {
        let from = from_conversation_id.trim();
        let to = to_conversation_id.trim();
        if from.is_empty() || to.is_empty() || from == to {
            return Ok(());
        }
        {
            let mut data = self.data.write().await;
            let Some(mut source) = data.remove(from) else {
                return Ok(());
            };
            source.conversation_id = to.to_string();
            match data.get_mut(to) {
                Some(target) => {
                    if target.channel_id.trim().is_empty() {
                        target.channel_id = source.channel_id;
                    }
                    if target.display_name.trim().is_empty() {
                        target.display_name = source.display_name;
                    }
                    if target.avatar_url.trim().is_empty() {
                        target.avatar_url = source.avatar_url;
                    }
                    if target.remark.is_none() {
                        target.remark = source.remark;
                    }
                    if target.draft.is_none() {
                        target.draft = source.draft;
                    }
                    if source.max_seq > target.max_seq {
                        target.max_seq = source.max_seq;
                        target.last_message_id = source.last_message_id;
                        target.last_sender_id = source.last_sender_id;
                        target.last_message_at = source.last_message_at;
                        target.last_message_preview = source.last_message_preview;
                    }
                    target.unread_count = target.unread_count.max(source.unread_count);
                    target.last_read_seq = target.last_read_seq.max(source.last_read_seq);
                    target.visible_after_seq =
                        target.visible_after_seq.max(source.visible_after_seq);
                    target.is_pinned |= source.is_pinned;
                    target.is_muted |= source.is_muted;
                    target.is_archived |= source.is_archived;
                    target.version = target.version.max(source.version);
                    target.updated_at = target.updated_at.max(source.updated_at);
                    target.updated_at_ts = target.updated_at_ts.max(source.updated_at_ts);
                    for (key, value) in source.ext {
                        target.ext.entry(key).or_insert(value);
                    }
                }
                None => {
                    data.insert(to.to_string(), source);
                }
            }
        }
        let mut max_seq = self.materialized_max_seq.write().await;
        let source_seq = max_seq.remove(from).unwrap_or_default();
        if source_seq > 0 {
            let target_seq = max_seq.entry(to.to_string()).or_default();
            *target_seq = (*target_seq).max(source_seq);
        }
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
        self.materialized_max_seq
            .write()
            .await
            .remove(conversation_id);
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
            let current_seq = c.max_seq;
            let current_time = c.last_message_at.unwrap_or(0);
            let should_replace = max_seq >= current_seq || last_message_at >= current_time;
            if should_replace {
                c.last_message_id = Some(last_message_id.to_string());
                c.last_sender_id = Some(last_sender_id.to_string());
                c.last_message_at = Some(last_message_at);
                c.last_message_preview = last_message_preview.map(String::from);
            }
            c.max_seq = c.max_seq.max(max_seq);
        }
        if max_seq > 0 {
            let mut materialized = self.materialized_max_seq.write().await;
            let current = materialized.entry(conversation_id.to_string()).or_default();
            *current = (*current).max(max_seq);
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
            .materialized_max_seq
            .read()
            .await
            .get(conversation_id)
            .copied()
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
    use super::{MemoryConversationStore, MemoryMessageStore};
    use crate::domain::{
        ConversationReader, ConversationWriter, MessageReader, MessageStore, MessageWriter,
        OperationApplyResult,
    };
    use crate::model::{Conversation, IMMessage};
    use crate::shared::error::ErrorCode;
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

    #[tokio::test]
    async fn read_receipt_marks_outgoing_messages_read() {
        let store = MemoryMessageStore::new();
        let mut first = local_message("server-memory-read-1", "client-memory-read-1");
        first.conversation_id = "conv-memory-read-receipt".to_string();
        first.conversation_seq = 1;
        first.status = MessageStatus::Created as i32;
        let mut second = local_message("server-memory-read-2", "client-memory-read-2");
        second.conversation_id = "conv-memory-read-receipt".to_string();
        second.conversation_seq = 2;
        second.status = MessageStatus::Persisted as i32;
        let mut unread_tail = local_message("server-memory-read-3", "client-memory-read-3");
        unread_tail.conversation_id = "conv-memory-read-receipt".to_string();
        unread_tail.conversation_seq = 3;
        unread_tail.status = MessageStatus::Sent as i32;

        store
            .save_batch(&[first, second, unread_tail])
            .await
            .unwrap();
        store
            .mark_outgoing_read_upto_seq("conv-memory-read-receipt", "u1", 2)
            .await
            .unwrap();

        let first = store.get("server-memory-read-1").await.unwrap().unwrap();
        let second = store.get("server-memory-read-2").await.unwrap().unwrap();
        let tail = store.get("server-memory-read-3").await.unwrap().unwrap();
        assert!(first.is_read);
        assert_eq!(first.status, MessageStatus::Sent as i32);
        assert!(second.is_read);
        assert_eq!(second.status, MessageStatus::Persisted as i32);
        assert!(!tail.is_read);
    }

    #[tokio::test]
    async fn reconcile_outgoing_read_by_peer_seq_downgrades_polluted_tail() {
        let store = MemoryMessageStore::new();
        let mut first = local_message("server-memory-peer-read-1", "client-memory-peer-read-1");
        first.conversation_id = "conv-memory-peer-read".to_string();
        first.conversation_seq = 1;
        first.status = MessageStatus::Sent as i32;
        first.is_read = true;
        let mut tail = local_message("server-memory-peer-read-2", "client-memory-peer-read-2");
        tail.conversation_id = "conv-memory-peer-read".to_string();
        tail.conversation_seq = 2;
        tail.status = MessageStatus::Sent as i32;
        tail.is_read = true;

        store.save_batch(&[first, tail]).await.unwrap();
        store
            .reconcile_outgoing_read_by_peer_seq("conv-memory-peer-read", "u1", 1)
            .await
            .unwrap();

        let first = store
            .get("server-memory-peer-read-1")
            .await
            .unwrap()
            .unwrap();
        let tail = store
            .get("server-memory-peer-read-2")
            .await
            .unwrap()
            .unwrap();
        assert!(first.is_read);
        assert!(!tail.is_read);
    }

    #[tokio::test]
    async fn reaction_updates_are_visible_by_server_and_client_message_id() {
        let store = MemoryMessageStore::new();
        let message = local_message("server-memory-reaction-1", "client-memory-reaction-1");
        store.save_one(&message).await.unwrap();

        store
            .apply_reaction(
                "conv-memory-dupe",
                "server-memory-reaction-1",
                "u2",
                "thumbsup",
                flare_proto::common::ReactionAction::Add as i32,
            )
            .await
            .unwrap();

        let reactions = store
            .list_reactions(&[
                "server-memory-reaction-1".to_string(),
                "client-memory-reaction-1".to_string(),
            ])
            .await
            .unwrap();
        let by_server = reactions
            .get("server-memory-reaction-1")
            .expect("server id reaction");
        let by_client = reactions
            .get("client-memory-reaction-1")
            .expect("client id reaction");
        assert_eq!(by_server[0].emoji, "thumbsup");
        assert_eq!(by_server[0].user_ids, vec!["u2".to_string()]);
        assert_eq!(by_client[0].count, 1);

        store
            .apply_reaction(
                "conv-memory-dupe",
                "server-memory-reaction-1",
                "u2",
                "thumbsup",
                flare_proto::common::ReactionAction::Remove as i32,
            )
            .await
            .unwrap();

        let reactions = store
            .list_reactions(&["server-memory-reaction-1".to_string()])
            .await
            .unwrap();
        assert!(reactions.is_empty());
    }

    #[tokio::test]
    async fn save_batch_preserves_pin_event_attributes() {
        let store = MemoryMessageStore::new();
        let message = local_message("server-memory-pin-1", "client-memory-pin-1");
        store.save_one(&message).await.unwrap();

        let applied = store
            .apply_pin_event("server-memory-pin-1", true, Some(10))
            .await
            .unwrap();
        assert_eq!(applied, OperationApplyResult::Applied);

        let mut snapshot = message.clone();
        snapshot.attributes.clear();
        store.save_one(&snapshot).await.unwrap();

        let after_snapshot = store
            .get("server-memory-pin-1")
            .await
            .unwrap()
            .expect("message");
        assert_eq!(
            after_snapshot.attributes.get("pinned").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            after_snapshot
                .attributes
                .get("lastPinEventSeq")
                .map(String::as_str),
            Some("10")
        );

        let stale = store
            .apply_pin_event("server-memory-pin-1", false, Some(9))
            .await
            .unwrap();
        assert_eq!(stale, OperationApplyResult::IgnoredStale);
        let after_stale = store
            .get("server-memory-pin-1")
            .await
            .unwrap()
            .expect("message");
        assert_eq!(
            after_stale.attributes.get("pinned").map(String::as_str),
            Some("true")
        );

        let newer = store
            .apply_pin_event("server-memory-pin-1", false, Some(11))
            .await
            .unwrap();
        assert_eq!(newer, OperationApplyResult::Applied);
        store.save_one(&snapshot).await.unwrap();
        let after_unpin_snapshot = store
            .get("server-memory-pin-1")
            .await
            .unwrap()
            .expect("message");
        assert_eq!(
            after_unpin_snapshot
                .attributes
                .get("pinned")
                .map(String::as_str),
            Some("false")
        );
        assert_eq!(
            after_unpin_snapshot
                .attributes
                .get("lastPinEventSeq")
                .map(String::as_str),
            Some("11")
        );
    }

    #[tokio::test]
    async fn conversation_save_batch_rejects_blank_conversation_id() {
        let store = MemoryConversationStore::new();
        let conversation = Conversation::from_conversation_id("   ".to_string());

        let err = store
            .save_batch(&[conversation])
            .await
            .expect_err("blank conversation id must be rejected");

        assert_eq!(err.code(), Some(ErrorCode::InvalidParameter));
    }

    #[tokio::test]
    async fn conversation_save_batch_does_not_roll_back_local_read_position() {
        let store = MemoryConversationStore::new();
        let mut local = Conversation::from_conversation_id("conv-memory-read".to_string());
        local.max_seq = 310;
        local.last_read_seq = 310;
        local.unread_count = 0;
        store.save_one(&local).await.unwrap();

        let mut stale = Conversation::from_conversation_id("conv-memory-read".to_string());
        stale.max_seq = 310;
        stale.last_read_seq = 20;
        stale.unread_count = 17;
        store.save_one(&stale).await.unwrap();

        let stored = store
            .get("conv-memory-read")
            .await
            .unwrap()
            .expect("conversation");
        assert_eq!(stored.max_seq, 310);
        assert_eq!(stored.last_read_seq, 310);
        assert_eq!(stored.unread_count, 0);
    }

    #[tokio::test]
    async fn conversation_save_batch_does_not_clear_local_latest_message_with_empty_summary() {
        let store = MemoryConversationStore::new();
        let mut local = Conversation::from_conversation_id("conv-memory-latest".to_string());
        local.max_seq = 100;
        local.last_message_id = Some("msg-100".to_string());
        local.last_sender_id = Some("u2".to_string());
        local.last_message_at = Some(12_000);
        local.last_message_preview = Some("latest".to_string());
        store.save_one(&local).await.unwrap();

        let mut stale = Conversation::from_conversation_id("conv-memory-latest".to_string());
        stale.max_seq = 100;
        stale.last_read_seq = 90;
        stale.unread_count = 10;
        store.save_one(&stale).await.unwrap();

        let stored = store
            .get("conv-memory-latest")
            .await
            .unwrap()
            .expect("conversation");
        assert_eq!(stored.last_message_id.as_deref(), Some("msg-100"));
        assert_eq!(stored.last_sender_id.as_deref(), Some("u2"));
        assert_eq!(stored.last_message_at, Some(12_000));
        assert_eq!(stored.last_message_preview.as_deref(), Some("latest"));
    }

    #[tokio::test]
    async fn local_max_seq_tracks_materialized_messages_not_remote_summary() {
        let store = MemoryConversationStore::new();
        let mut summary = Conversation::from_conversation_id("conv-memory-seq".to_string());
        summary.max_seq = 8;
        summary.unread_count = 8;
        store.save_batch(&[summary]).await.unwrap();

        assert_eq!(
            store.get("conv-memory-seq").await.unwrap().unwrap().max_seq,
            8
        );
        assert_eq!(store.get_local_max_seq("conv-memory-seq").await.unwrap(), 0);

        store
            .update_last_message(
                "conv-memory-seq",
                "server-memory-seq-3",
                "alice",
                1_000,
                Some("latest"),
                3,
            )
            .await
            .unwrap();

        assert_eq!(
            store.get("conv-memory-seq").await.unwrap().unwrap().max_seq,
            8
        );
        assert_eq!(store.get_local_max_seq("conv-memory-seq").await.unwrap(), 3);
    }

    #[tokio::test]
    async fn local_zero_seq_message_updates_preview_without_rolling_back_server_seq() {
        let store = MemoryConversationStore::new();
        let mut conversation =
            Conversation::from_conversation_id("conv-memory-preview".to_string());
        conversation.max_seq = 8;
        conversation.last_message_at = Some(1_000);
        conversation.last_message_preview = Some("server-latest".to_string());
        store.save_one(&conversation).await.unwrap();

        store
            .update_last_message(
                "conv-memory-preview",
                "client-local-1",
                "alice",
                2_000,
                Some("local pending"),
                0,
            )
            .await
            .unwrap();

        let updated = store
            .get("conv-memory-preview")
            .await
            .unwrap()
            .expect("conversation");
        assert_eq!(updated.max_seq, 8);
        assert_eq!(updated.last_message_id.as_deref(), Some("client-local-1"));
        assert_eq!(
            updated.last_message_preview.as_deref(),
            Some("local pending")
        );

        store
            .update_last_message(
                "conv-memory-preview",
                "client-stale-1",
                "alice",
                1_500,
                Some("stale local"),
                0,
            )
            .await
            .unwrap();

        let updated = store
            .get("conv-memory-preview")
            .await
            .unwrap()
            .expect("conversation");
        assert_eq!(updated.last_message_id.as_deref(), Some("client-local-1"));
        assert_eq!(
            updated.last_message_preview.as_deref(),
            Some("local pending")
        );
    }

    #[tokio::test]
    async fn update_last_message_accepts_newer_time_when_summary_max_seq_is_ahead() {
        let store = MemoryConversationStore::new();
        let mut summary =
            Conversation::from_conversation_id("conv-memory-summary-ahead".to_string());
        summary.max_seq = 99;
        summary.last_message_id = Some("msg-11".to_string());
        summary.last_sender_id = Some("u1".to_string());
        summary.last_message_at = Some(11_000);
        summary.last_message_preview = Some("stale-summary".to_string());
        store.save_one(&summary).await.unwrap();

        store
            .update_last_message(
                "conv-memory-summary-ahead",
                "msg-12",
                "u2",
                12_345,
                Some("111"),
                12,
            )
            .await
            .unwrap();

        let loaded = store
            .get("conv-memory-summary-ahead")
            .await
            .unwrap()
            .expect("conversation");
        assert_eq!(loaded.max_seq, 99);
        assert_eq!(loaded.last_message_id.as_deref(), Some("msg-12"));
        assert_eq!(loaded.last_sender_id.as_deref(), Some("u2"));
        assert_eq!(loaded.last_message_at, Some(12_345));
        assert_eq!(loaded.last_message_preview.as_deref(), Some("111"));
    }
}
