use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use tokio::sync::Mutex;

const DEFAULT_DEDUPE_CAPACITY: usize = 4096;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum EventDedupeKey {
    EventId(String),
    EventSeq {
        conversation_id: String,
        event_type: i32,
        seq: u64,
    },
    Seq {
        conversation_id: String,
        event_type: i32,
        seq: u64,
    },
    RequestId {
        conversation_id: String,
        event_type: i32,
        request_id: String,
    },
}

#[derive(Default)]
struct EventDedupeState {
    order: VecDeque<EventDedupeKey>,
    seen: HashSet<EventDedupeKey>,
}

impl EventDedupeState {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            order: VecDeque::with_capacity(capacity),
            seen: HashSet::with_capacity(capacity),
        }
    }
}

#[derive(Clone)]
pub(crate) struct EventDeduper {
    capacity: usize,
    state: Arc<Mutex<EventDedupeState>>,
}

impl EventDeduper {
    pub(crate) fn new(capacity: Option<usize>) -> Self {
        let capacity = capacity.unwrap_or(DEFAULT_DEDUPE_CAPACITY).max(1);
        Self {
            capacity,
            state: Arc::new(Mutex::new(EventDedupeState::with_capacity(capacity))),
        }
    }

    pub(crate) async fn record_if_new(&self, event: &flare_proto::common::Event) -> bool {
        let Some(key) = dedupe_key_for_event(event) else {
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

    pub(crate) async fn forget(&self, event: &flare_proto::common::Event) {
        let Some(key) = dedupe_key_for_event(event) else {
            return;
        };
        let mut state = self.state.lock().await;
        state.seen.remove(&key);
        state.order.retain(|stored| stored != &key);
    }
}

fn dedupe_key_for_event(event: &flare_proto::common::Event) -> Option<EventDedupeKey> {
    if !event.event_id.trim().is_empty() {
        return Some(EventDedupeKey::EventId(event.event_id.clone()));
    }
    if let Some(event_seq) = event.event_seq
        && event_seq > 0
    {
        return Some(EventDedupeKey::EventSeq {
            conversation_id: event.conversation_id.clone(),
            event_type: event.r#type,
            seq: event_seq,
        });
    }
    if event.seq > 0 {
        return Some(EventDedupeKey::Seq {
            conversation_id: event.conversation_id.clone(),
            event_type: event.r#type,
            seq: event.seq,
        });
    }
    if let Some(request_id) = &event.request_id
        && !request_id.trim().is_empty()
    {
        return Some(EventDedupeKey::RequestId {
            conversation_id: event.conversation_id.clone(),
            event_type: event.r#type,
            request_id: request_id.clone(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::EventDeduper;

    #[tokio::test]
    async fn duplicate_event_id_is_deduped() {
        let deduper = EventDeduper::new(Some(16));
        let event = flare_proto::common::Event {
            event_id: "evt-1".to_string(),
            ..Default::default()
        };

        assert!(deduper.record_if_new(&event).await);
        assert!(!deduper.record_if_new(&event).await);
    }
}
