use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::model::IMMessage;

const DEFAULT_DEDUPE_CAPACITY: usize = 8192;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum MessageDedupeKey {
    ServerId(String),
    ClientMsgId(String),
    ConversationSeq {
        conversation_id: String,
        sender_id: String,
        seq: u64,
    },
}

#[derive(Default)]
struct MessageDedupeState {
    order: VecDeque<MessageDedupeKey>,
    seen: HashSet<MessageDedupeKey>,
}

impl MessageDedupeState {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            order: VecDeque::with_capacity(capacity),
            seen: HashSet::with_capacity(capacity),
        }
    }
}

#[derive(Clone)]
pub(crate) struct MessageDeduper {
    capacity: usize,
    state: Arc<Mutex<MessageDedupeState>>,
}

impl MessageDeduper {
    pub(crate) fn new(capacity: Option<usize>) -> Self {
        let capacity = capacity.unwrap_or(DEFAULT_DEDUPE_CAPACITY).max(1);
        Self {
            capacity,
            state: Arc::new(Mutex::new(MessageDedupeState::with_capacity(capacity))),
        }
    }

    pub(crate) async fn record_if_new(&self, message: &IMMessage) -> bool {
        let Some(key) = dedupe_key_for_message(message) else {
            return true;
        };

        let mut state = self.state.lock().await;
        if !state.seen.insert(key.clone()) {
            return false;
        }
        state.order.push_back(key);

        while state.order.len() > self.capacity {
            if let Some(removed) = state.order.pop_front() {
                state.seen.remove(&removed);
            }
        }
        true
    }
}

fn dedupe_key_for_message(message: &IMMessage) -> Option<MessageDedupeKey> {
    if !message.server_id.trim().is_empty() {
        return Some(MessageDedupeKey::ServerId(message.server_id.clone()));
    }
    if !message.client_msg_id.trim().is_empty() {
        return Some(MessageDedupeKey::ClientMsgId(message.client_msg_id.clone()));
    }
    if !message.conversation_id.trim().is_empty() && message.conversation_seq > 0 {
        return Some(MessageDedupeKey::ConversationSeq {
            conversation_id: message.conversation_id.clone(),
            sender_id: message.sender_id.clone(),
            seq: message.conversation_seq,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::MessageDeduper;
    use crate::model::IMMessage;

    #[tokio::test]
    async fn duplicate_server_id_is_deduped() {
        let deduper = MessageDeduper::new(Some(16));
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-1".to_string();

        assert!(deduper.record_if_new(&message).await);
        assert!(!deduper.record_if_new(&message).await);
    }
}
