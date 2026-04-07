use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use tokio::sync::Mutex;

const DEFAULT_DEDUPE_CAPACITY: usize = 4096;

#[derive(Default)]
struct EventDedupeState {
    order: VecDeque<String>,
    seen: HashSet<String>,
}

#[derive(Clone)]
pub(crate) struct EventDeduper {
    capacity: usize,
    state: Arc<Mutex<EventDedupeState>>,
}

impl EventDeduper {
    pub(crate) fn new(capacity: Option<usize>) -> Self {
        Self {
            capacity: capacity.unwrap_or(DEFAULT_DEDUPE_CAPACITY).max(1),
            state: Arc::new(Mutex::new(EventDedupeState::default())),
        }
    }

    pub(crate) async fn record_if_new(&self, event: &flare_proto::common::Event) -> bool {
        let Some(key) = dedupe_key_for_event(event) else {
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

fn dedupe_key_for_event(event: &flare_proto::common::Event) -> Option<String> {
    if !event.event_id.trim().is_empty() {
        return Some(format!("event_id:{}", event.event_id));
    }
    if let Some(event_seq) = event.event_seq {
        if event_seq > 0 {
            return Some(format!(
                "event_seq:{}:{}:{}",
                event.conversation_id, event.r#type, event_seq
            ));
        }
    }
    if event.seq > 0 {
        return Some(format!(
            "seq:{}:{}:{}",
            event.conversation_id, event.r#type, event.seq
        ));
    }
    if let Some(request_id) = &event.request_id {
        if !request_id.trim().is_empty() {
            return Some(format!(
                "request_id:{}:{}:{}",
                event.conversation_id, event.r#type, request_id
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::EventDeduper;

    #[tokio::test]
    async fn duplicate_event_id_is_deduped() {
        let deduper = EventDeduper::new(Some(16));
        let mut event = flare_proto::common::Event::default();
        event.event_id = "evt-1".to_string();

        assert!(deduper.record_if_new(&event).await);
        assert!(!deduper.record_if_new(&event).await);
    }
}
