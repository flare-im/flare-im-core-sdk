use crate::model::{Message, SessionSummary, SyncCursor};
use crate::storage::storage_trait::{StorageBackend, SessionFilter, SessionUpdate, LastMessageUpdate, MessageState, StorageSyncBounds};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::cell::RefCell;
use std::rc::Rc;

pub struct MemoryStorage {
    messages: Rc<RefCell<HashMap<String, Message>>>,
    sessions: Rc<RefCell<HashMap<String, SessionSummary>>>,
    cursors: Rc<RefCell<HashMap<String, SyncCursor>>>,
    states: Rc<RefCell<HashMap<(String, String), MessageState>>>,
}

impl StorageSyncBounds for MemoryStorage {}

impl MemoryStorage {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            messages: Rc::new(RefCell::new(HashMap::new())),
            sessions: Rc::new(RefCell::new(HashMap::new())),
            cursors: Rc::new(RefCell::new(HashMap::new())),
            states: Rc::new(RefCell::new(HashMap::new())),
        })
    }
}

#[async_trait(?Send)]
impl StorageBackend for MemoryStorage {
    async fn save_message(&self, message: &Message) -> Result<()> {
        self.messages.borrow_mut().insert(message.id.clone(), message.clone());
        Ok(())
    }

    async fn get_message(&self, message_id: &str) -> Result<Option<Message>> {
        Ok(self.messages.borrow().get(message_id).cloned())
    }

    async fn get_messages(&self, session_id: &str, limit: usize, _cursor: Option<String>) -> Result<Vec<Message>> {
        let mut msgs: Vec<_> = self.messages.borrow().values().filter(|m| m.session_id == session_id).cloned().collect();
        msgs.sort_by_key(|m| {
            let ts = m.timeline.as_ref()
                .and_then(|t| t.persisted_time.as_ref().or(t.delivered_time.as_ref()).or(t.ingestion_time.as_ref()))
                .map(|ts| (ts.seconds as i64) * 1000 + (ts.nanos as i64) / 1_000_000)
                .unwrap_or(0);
            std::cmp::Reverse(ts)
        });
        msgs.truncate(limit);
        Ok(msgs)
    }

    async fn get_messages_by_seq(&self, session_id: &str, _after_seq: i64, limit: usize) -> Result<Vec<Message>> {
        self.get_messages(session_id, limit, None).await
    }

    async fn get_max_seq(&self, _session_id: &str) -> Result<Option<i64>> {
        Ok(None)
    }

    async fn delete_message(&self, message_id: &str) -> Result<()> {
        self.messages.borrow_mut().remove(message_id);
        Ok(())
    }

    async fn save_session(&self, session: &SessionSummary) -> Result<()> {
        self.sessions.borrow_mut().insert(session.session_id.clone(), session.clone());
        Ok(())
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<SessionSummary>> {
        Ok(self.sessions.borrow().get(session_id).cloned())
    }

    async fn get_sessions(&self, filter: SessionFilter) -> Result<Vec<SessionSummary>> {
        let mut list: Vec<_> = self.sessions.borrow().values().cloned().collect();
        list.retain(|s| {
            (filter.session_type.is_none() || filter.session_type.as_ref() == Some(&s.session_type)) &&
            (filter.business_type.is_none() || filter.business_type.as_ref() == Some(&s.business_type)) &&
            (!filter.unread_only || s.unread_count > 0)
        });
        list.sort_by_key(|s| std::cmp::Reverse(s.last_message_time));
        if let Some(limit) = filter.limit { list.truncate(limit); }
        Ok(list)
    }

    async fn update_session(&self, session_id: &str, updates: SessionUpdate) -> Result<()> {
        if let Some(s) = self.sessions.borrow_mut().get_mut(session_id) {
            if let Some(u) = updates.unread_count { s.unread_count = u; }
            if let Some(name) = updates.display_name { s.display_name = Some(name); }
            if let Some(meta) = updates.metadata { s.metadata = meta; }
            if let Some(last) = updates.last_message {
                s.last_message_id = Some(last.message_id);
                s.last_message_time = Some(last.message_time);
                if let Some(sender) = last.sender_id { s.last_sender_id = Some(sender); }
                s.last_message_type = last.message_type;
                s.last_content_type = last.content_type;
            }
        }
        Ok(())
    }

    async fn delete_session(&self, session_id: &str) -> Result<()> {
        self.sessions.borrow_mut().remove(session_id);
        Ok(())
    }

    async fn save_sync_cursor(&self, session_id: &str, cursor: &SyncCursor) -> Result<()> {
        self.cursors.borrow_mut().insert(session_id.to_string(), cursor.clone());
        Ok(())
    }

    async fn get_sync_cursor(&self, session_id: &str) -> Result<Option<SyncCursor>> {
        Ok(self.cursors.borrow().get(session_id).cloned())
    }

    async fn get_all_sync_cursors(&self) -> Result<Vec<SyncCursor>> {
        Ok(self.cursors.borrow().values().cloned().collect())
    }

    async fn save_message_state(&self, user_id: &str, message_id: &str, state: MessageState) -> Result<()> {
        self.states.borrow_mut().insert((user_id.to_string(), message_id.to_string()), state);
        Ok(())
    }

    async fn get_message_state(&self, user_id: &str, message_id: &str) -> Result<Option<MessageState>> {
        Ok(self.states.borrow().get(&(user_id.to_string(), message_id.to_string())).cloned())
    }

    async fn batch_check_deleted(&self, user_id: &str, message_ids: &[String]) -> Result<Vec<String>> {
        let mut deleted = Vec::new();
        for id in message_ids {
            if let Some(state) = self.get_message_state(user_id, id).await? {
                if state.is_deleted { deleted.push(id.clone()); }
            }
        }
        Ok(deleted)
    }
}
