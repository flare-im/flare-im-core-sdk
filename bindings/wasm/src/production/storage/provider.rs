//! WASM store provider: memory hot path + optional JS IndexedDB persistence.

use std::sync::Arc;

use async_trait::async_trait;
use flare_im_core_sdk::domain::{
    ConversationReader, ConversationWriter, MessageReader, MessageStore, MessageWriter,
    PendingSendReader, PendingSendVo, PendingSendWriter, SyncCursorReader, SyncCursorWriter,
};
use flare_im_core_sdk::infrastructure::persistence::memory::{
    MemoryPendingSendStore, MemoryUserProfileStore,
};
use flare_im_core_sdk::infrastructure::persistence::memory_im::{
    MemoryConversationStore, MemoryMessageStore,
};
use flare_im_core_sdk::infrastructure::persistence::{
    MemorySyncCursorStore, StoreProvider, in_memory_im_provider,
};
use flare_im_core_sdk::model::Conversation;
use flare_im_core_sdk::model::IMMessage;
use flare_im_core_sdk::shared::error::Result;

use super::host::{
    delete_conversation, delete_message, delete_pending_send, load_snapshot, persist_conversation,
    persist_cursor, persist_message, persist_pending_send, storage_host_configured,
};

fn spawn_persist<F>(future: F)
where
    F: std::future::Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(future);
}

fn persist_conversation_spawn(user_id: &str, conversation: Conversation) {
    let user_id = user_id.to_string();
    spawn_persist(async move {
        let _ = persist_conversation(&user_id, &conversation).await;
    });
}

fn persist_cursor_spawn(user_id: &str, key: String, value: String) {
    let user_id = user_id.to_string();
    spawn_persist(async move {
        let _ = persist_cursor(&user_id, &key, &value).await;
    });
}

struct PersistingMessageStore {
    user_id: String,
    inner: Arc<MemoryMessageStore>,
}

struct PersistingConversationStore {
    user_id: String,
    inner: Arc<MemoryConversationStore>,
}

struct PersistingSyncCursorStore {
    user_id: String,
    inner: Arc<MemorySyncCursorStore>,
}

struct PersistingPendingSendStore {
    user_id: String,
    inner: Arc<MemoryPendingSendStore>,
}

impl PersistingMessageStore {
    fn new(user_id: String, inner: Arc<MemoryMessageStore>) -> Self {
        Self { user_id, inner }
    }
}

impl PersistingConversationStore {
    fn new(user_id: String, inner: Arc<MemoryConversationStore>) -> Self {
        Self { user_id, inner }
    }
}

impl PersistingSyncCursorStore {
    fn new(user_id: String, inner: Arc<MemorySyncCursorStore>) -> Self {
        Self { user_id, inner }
    }
}

impl PersistingPendingSendStore {
    fn new(user_id: String, inner: Arc<MemoryPendingSendStore>) -> Self {
        Self { user_id, inner }
    }
}

#[async_trait]
impl MessageReader for PersistingMessageStore {
    async fn get(&self, message_id: &str) -> Result<Option<IMMessage>> {
        self.inner.get(message_id).await
    }

    async fn get_by_client_msg_id(&self, client_msg_id: &str) -> Result<Option<IMMessage>> {
        self.inner.get_by_client_msg_id(client_msg_id).await
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<IMMessage>> {
        self.inner
            .get_by_conversation(conversation_id, before_seq, limit)
            .await
    }

    async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<IMMessage>> {
        self.inner.search(keyword, limit).await
    }

    async fn search_in_conversation(
        &self,
        conversation_id: &str,
        keyword: &str,
        limit: u32,
    ) -> Result<Vec<IMMessage>> {
        self.inner
            .search_in_conversation(conversation_id, keyword, limit)
            .await
    }
}

#[async_trait]
impl MessageWriter for PersistingMessageStore {
    async fn save_batch(&self, messages: &[IMMessage]) -> Result<()> {
        self.inner.save_batch(messages).await?;
        for message in messages {
            let user_id = self.user_id.clone();
            let message = message.clone();
            spawn_persist(async move {
                let _ = persist_message(&user_id, &message).await;
            });
        }
        Ok(())
    }

    async fn save_one(&self, message: &IMMessage) -> Result<()> {
        self.inner.save_one(message).await?;
        let user_id = self.user_id.clone();
        let message = message.clone();
        spawn_persist(async move {
            let _ = persist_message(&user_id, &message).await;
        });
        Ok(())
    }

    async fn update_status(&self, message_id: &str, status: i32) -> Result<()> {
        self.inner.update_status(message_id, status).await?;
        if let Some(message) = self.inner.get(message_id).await? {
            let user_id = self.user_id.clone();
            spawn_persist(async move {
                let _ = persist_message(&user_id, &message).await;
            });
        }
        Ok(())
    }

    async fn update_content(&self, message_id: &str, new_content: Vec<u8>) -> Result<bool> {
        let updated = self.inner.update_content(message_id, new_content).await?;
        if updated && let Some(message) = self.inner.get(message_id).await? {
            let user_id = self.user_id.clone();
            spawn_persist(async move {
                let _ = persist_message(&user_id, &message).await;
            });
        }
        Ok(updated)
    }

    async fn delete(&self, message_id: &str) -> Result<()> {
        self.inner.delete(message_id).await?;
        let user_id = self.user_id.clone();
        let message_id = message_id.to_string();
        spawn_persist(async move {
            let _ = delete_message(&user_id, &message_id).await;
        });
        Ok(())
    }

    async fn update_after_ack(&self, client_msg_id: &str, message: &IMMessage) -> Result<()> {
        self.inner.update_after_ack(client_msg_id, message).await?;
        let user_id = self.user_id.clone();
        let client_msg_id = client_msg_id.to_string();
        let message = message.clone();
        spawn_persist(async move {
            let _ = delete_message(&user_id, &client_msg_id).await;
            let _ = persist_message(&user_id, &message).await;
        });
        Ok(())
    }
}

impl MessageStore for PersistingMessageStore {}

#[async_trait]
impl ConversationReader for PersistingConversationStore {
    async fn get(&self, conversation_id: &str) -> Result<Option<Conversation>> {
        self.inner.get(conversation_id).await
    }

    async fn list(&self) -> Result<Vec<Conversation>> {
        self.inner.list().await
    }
}

#[async_trait]
impl ConversationWriter for PersistingConversationStore {
    async fn save_batch(&self, conversations: &[Conversation]) -> Result<()> {
        self.inner.save_batch(conversations).await?;
        for conversation in conversations {
            let user_id = self.user_id.clone();
            let conversation = conversation.clone();
            spawn_persist(async move {
                let _ = persist_conversation(&user_id, &conversation).await;
            });
        }
        Ok(())
    }

    async fn save_one(&self, conversation: &Conversation) -> Result<()> {
        self.inner.save_one(conversation).await?;
        let user_id = self.user_id.clone();
        let conversation = conversation.clone();
        spawn_persist(async move {
            let _ = persist_conversation(&user_id, &conversation).await;
        });
        Ok(())
    }

    async fn update_unread(
        &self,
        conversation_id: &str,
        unread_count: u32,
        last_read_seq: u64,
    ) -> Result<()> {
        self.inner
            .update_unread(conversation_id, unread_count, last_read_seq)
            .await?;
        if let Some(conversation) = self.inner.get(conversation_id).await? {
            persist_conversation_spawn(&self.user_id, conversation);
        }
        Ok(())
    }

    async fn set_pinned(&self, conversation_id: &str, pinned: bool) -> Result<()> {
        self.inner.set_pinned(conversation_id, pinned).await?;
        if let Some(conversation) = self.inner.get(conversation_id).await? {
            persist_conversation_spawn(&self.user_id, conversation);
        }
        Ok(())
    }

    async fn set_muted(&self, conversation_id: &str, muted: bool) -> Result<()> {
        self.inner.set_muted(conversation_id, muted).await?;
        if let Some(conversation) = self.inner.get(conversation_id).await? {
            persist_conversation_spawn(&self.user_id, conversation);
        }
        Ok(())
    }

    async fn set_archived(&self, conversation_id: &str, archived: bool) -> Result<()> {
        self.inner.set_archived(conversation_id, archived).await?;
        if let Some(conversation) = self.inner.get(conversation_id).await? {
            persist_conversation_spawn(&self.user_id, conversation);
        }
        Ok(())
    }

    async fn mark_unread(&self, conversation_id: &str) -> Result<u32> {
        let unread = self.inner.mark_unread(conversation_id).await?;
        if let Some(conversation) = self.inner.get(conversation_id).await? {
            persist_conversation_spawn(&self.user_id, conversation);
        }
        Ok(unread)
    }

    async fn update_draft(&self, conversation_id: &str, draft: Option<&str>) -> Result<()> {
        self.inner.update_draft(conversation_id, draft).await?;
        if let Some(conversation) = self.inner.get(conversation_id).await? {
            persist_conversation_spawn(&self.user_id, conversation);
        }
        Ok(())
    }

    async fn delete(&self, conversation_id: &str) -> Result<()> {
        self.inner.delete(conversation_id).await?;
        let user_id = self.user_id.clone();
        let conversation_id = conversation_id.to_string();
        spawn_persist(async move {
            let _ = delete_conversation(&user_id, &conversation_id).await;
        });
        Ok(())
    }

    async fn clear_local_chat_history(
        &self,
        conversation_id: &str,
        cleared_through_seq: u64,
    ) -> Result<()> {
        self.inner
            .clear_local_chat_history(conversation_id, cleared_through_seq)
            .await?;
        if let Some(conversation) = self.inner.get(conversation_id).await? {
            persist_conversation_spawn(&self.user_id, conversation);
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
        self.inner
            .update_last_message(
                conversation_id,
                last_message_id,
                last_sender_id,
                last_message_at,
                last_message_preview,
                max_seq,
            )
            .await?;
        if let Some(conversation) = self.inner.get(conversation_id).await? {
            persist_conversation_spawn(&self.user_id, conversation);
        }
        Ok(())
    }

    async fn recompute_unread_for_user(
        &self,
        conversation_id: &str,
        current_user_id: &str,
    ) -> Result<()> {
        self.inner
            .recompute_unread_for_user(conversation_id, current_user_id)
            .await?;
        if let Some(conversation) = self.inner.get(conversation_id).await? {
            persist_conversation_spawn(&self.user_id, conversation);
        }
        Ok(())
    }

    async fn get_local_max_seq(&self, conversation_id: &str) -> Result<u64> {
        self.inner.get_local_max_seq(conversation_id).await
    }
}

#[async_trait]
impl SyncCursorReader for PersistingSyncCursorStore {
    async fn get_raw(&self, key: &str) -> Result<Option<String>> {
        self.inner.get_raw(key).await
    }

    async fn get_conversation_cursor(
        &self,
        user_id: &str,
        conversation_id: &str,
    ) -> Result<Option<flare_im_core_sdk::domain::SyncCursorVo>> {
        self.inner
            .get_conversation_cursor(user_id, conversation_id)
            .await
    }
}

#[async_trait]
impl SyncCursorWriter for PersistingSyncCursorStore {
    async fn save_raw(&self, key: &str, cursor: &str) -> Result<()> {
        self.inner.save_raw(key, cursor).await?;
        persist_cursor_spawn(&self.user_id, key.to_string(), cursor.to_string());
        Ok(())
    }

    async fn save_conversation_cursor(
        &self,
        cursor: &flare_im_core_sdk::domain::SyncCursorVo,
    ) -> Result<()> {
        self.inner.save_conversation_cursor(cursor).await?;
        let key = format!("{}:{}", cursor.user_id, cursor.conversation_id);
        let value = format!("{}:{}", cursor.last_seq, cursor.synced_at);
        persist_cursor_spawn(&self.user_id, key, value);
        Ok(())
    }
}

#[async_trait]
impl PendingSendReader for PersistingPendingSendStore {
    async fn get(&self, client_msg_id: &str) -> Result<Option<PendingSendVo>> {
        self.inner.get(client_msg_id).await
    }

    async fn list(&self) -> Result<Vec<PendingSendVo>> {
        self.inner.list().await
    }

    async fn take_oldest(&self) -> Result<Option<PendingSendVo>> {
        self.inner.take_oldest().await
    }
}

#[async_trait]
impl PendingSendWriter for PersistingPendingSendStore {
    async fn push(&self, entry: PendingSendVo) -> Result<()> {
        self.inner.push(entry.clone()).await?;
        let user_id = self.user_id.clone();
        spawn_persist(async move {
            let _ = persist_pending_send(&user_id, &entry).await;
        });
        Ok(())
    }

    async fn pop(&self, client_msg_id: &str) -> Result<Option<PendingSendVo>> {
        let removed = self.inner.pop(client_msg_id).await?;
        if removed.is_some() {
            let user_id = self.user_id.clone();
            let client_msg_id = client_msg_id.to_string();
            spawn_persist(async move {
                let _ = delete_pending_send(&user_id, &client_msg_id).await;
            });
        }
        Ok(removed)
    }
}

pub async fn build_web_store_provider(user_id: &str) -> StoreProvider {
    if !storage_host_configured() {
        return in_memory_im_provider();
    }

    let messages = Arc::new(MemoryMessageStore::new());
    let conversations = Arc::new(MemoryConversationStore::new());
    let cursors = Arc::new(MemorySyncCursorStore::new());
    let pending_inner = Arc::new(MemoryPendingSendStore::new());

    if let Ok(snapshot) = load_snapshot(user_id).await {
        if !snapshot.messages.is_empty() {
            let _ = MessageWriter::save_batch(messages.as_ref(), &snapshot.messages).await;
        }
        if !snapshot.conversations.is_empty() {
            let _ = ConversationWriter::save_batch(conversations.as_ref(), &snapshot.conversations)
                .await;
        }
        for (key, value) in snapshot.cursors {
            let _ = SyncCursorWriter::save_raw(cursors.as_ref(), &key, &value).await;
        }
        for entry in snapshot.pending_sends {
            let _ = PendingSendWriter::push(pending_inner.as_ref(), entry).await;
        }
    }

    let user_id = user_id.to_string();
    let pending = Arc::new(PersistingPendingSendStore::new(
        user_id.clone(),
        pending_inner,
    ));
    let user_profiles = Arc::new(MemoryUserProfileStore::new());

    StoreProvider {
        messages: Arc::new(PersistingMessageStore::new(user_id.clone(), messages)),
        conversations: Arc::new(PersistingConversationStore::new(
            user_id.clone(),
            conversations,
        )),
        conversation_participants: None,
        cursors: Arc::new(PersistingSyncCursorStore::new(user_id, cursors)),
        pending_send_reader: Some(pending.clone()),
        pending_send_writer: Some(pending),
        upload_manifest_store: None,
        // Browser WASM: rely on gateway presigned URLs + HTTP cache; skip blob IDB cache.
        media_cache_store: None,
        media_cache_admin: None,
        user_file_download_store: None,
        user_profiles_reader: Some(user_profiles.clone()),
        user_profiles_writer: Some(user_profiles),
    }
}
