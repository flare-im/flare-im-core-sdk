use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::model::IMMessage;

const DEFAULT_DEDUPE_CAPACITY: usize = 8192;

#[derive(Default)]
struct MessageDedupeState {
    order: VecDeque<String>,
    seen: HashSet<String>,
}

#[derive(Clone)]
pub(crate) struct MessageDeduper {
    capacity: usize,
    state: Arc<Mutex<MessageDedupeState>>,
}

impl MessageDeduper {
    pub(crate) fn new(capacity: Option<usize>) -> Self {
        Self {
            capacity: capacity.unwrap_or(DEFAULT_DEDUPE_CAPACITY).max(1),
            state: Arc::new(Mutex::new(MessageDedupeState::default())),
        }
    }

    pub(crate) async fn record_if_new(&self, message: &IMMessage) -> bool {
        let Some(key) = dedupe_key_for_message(message) else {
            return true;
        };

        let mut state = self.state.lock().await;
        if state.seen.contains(&key) {
            return false;
        }
        state.seen.insert(key.clone());
        state.order.push_back(key);

        while state.order.len() > self.capacity {
            if let Some(removed) = state.order.pop_front() {
                state.seen.remove(&removed);
            }
        }
        true
    }
}

fn dedupe_key_for_message(message: &IMMessage) -> Option<String> {
    if !message.server_id.trim().is_empty() {
        return Some(format!("server_id:{}", message.server_id));
    }
    if !message.client_msg_id.trim().is_empty() {
        return Some(format!("client_msg_id:{}", message.client_msg_id));
    }
    if !message.conversation_id.trim().is_empty() && message.seq > 0 {
        return Some(format!(
            "conversation_seq:{}:{}:{}",
            message.conversation_id, message.sender_id, message.seq
        ));
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
