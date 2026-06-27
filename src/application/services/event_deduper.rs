use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use tokio::sync::Mutex;

const DEFAULT_DEDUPE_CAPACITY: usize = 4096;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum EventDedupeKey {
    EventId(String),
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
    if event.conversation_seq > 0 {
        return Some(EventDedupeKey::Seq {
            conversation_id: event.conversation_id.clone(),
            event_type: event.r#type,
            seq: event.conversation_seq,
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

    fn evt(id: u64) -> flare_proto::common::Event {
        flare_proto::common::Event {
            event_id: format!("e{id}"),
            ..Default::default()
        }
    }

    /// 窗口满后淘汰最旧 key:被淘汰的 key 再次出现应视为"新"。
    #[tokio::test]
    async fn evicts_oldest_when_window_full() {
        let cap = 4;
        let deduper = EventDeduper::new(Some(cap));
        for i in 0..cap as u64 {
            assert!(deduper.record_if_new(&evt(i)).await, "first sight of e{i}");
        }
        // e0 仍在窗口 [e0..e3] → 重复被拒(且不重排)。
        assert!(!deduper.record_if_new(&evt(0)).await, "e0 still in window");
        // e4 为新 → 窗口满 → 淘汰最旧 e0 → 窗口 [e1..e4]。
        assert!(deduper.record_if_new(&evt(4)).await, "e4 is new");
        // e0 已被淘汰 → 再次视为新。
        assert!(
            deduper.record_if_new(&evt(0)).await,
            "e0 was evicted, new again"
        );
        // e4 仍在窗口 → 重复被拒。
        assert!(!deduper.record_if_new(&evt(4)).await, "e4 still in window");
    }

    /// TEST-1:对拍独立参考实现(保留最近 cap 个不同 key 的 FIFO 窗口),
    /// 小 key 空间强制重复+淘汰,逐步断言去重判定一致。
    #[tokio::test]
    async fn prop_dedupe_window_matches_independent_reference() {
        struct RefDedup {
            cap: usize,
            order: Vec<u64>, // 当前窗口内不同 key,插入序;超容从头淘汰
        }
        impl RefDedup {
            fn record(&mut self, k: u64) -> bool {
                if self.order.contains(&k) {
                    return false;
                }
                self.order.push(k);
                if self.order.len() > self.cap {
                    self.order.remove(0);
                }
                true
            }
        }

        // 确定性 LCG(避免引入 rand/proptest 依赖)。
        let mut s: u64 = 0xC0FF_EE12_3456_789A;
        let mut next = || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            s
        };

        let cap = 8usize;
        let deduper = EventDeduper::new(Some(cap));
        let mut reference = RefDedup {
            cap,
            order: Vec::new(),
        };

        for step in 0..6000 {
            let k = next() % 20; // key 空间 20 > cap 8 → 必经淘汰
            let got = deduper.record_if_new(&evt(k)).await;
            let want = reference.record(k);
            assert_eq!(
                got, want,
                "dedupe decision mismatch at step {step}, key {k}"
            );
        }
    }
}
