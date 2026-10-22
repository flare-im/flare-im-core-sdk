//! 升级前缓存的插件卡片补拉共享层。
//!
//! 共享层由 Reader 读回消息时附在 extra（`pluginSharedState*`），之后的变化靠
//! `EVENT_MESSAGE_PLUGIN_STATE` 经实时与同步送达。客户端升级前就缓存了、当时又错过了事件的卡片，
//! 本地没有共享层，会一直显示旧状态。呈现这样一张卡片时（贡献声明了 `entityPath`、本地共享层版本
//! 为 0、消息已有服务端 id 与会话序号），在后台按会话序号向服务端补读这一条消息（同步协议的历史
//! 回溯页，`limit=1`，与翻历史同一条路径），取出它的共享层按版本单调写入；写入了就发
//! `PluginStateChanged`，宿主照常重绘。服务端也没有状态时什么都不写。
//!
//! 预算：
//! - 只有实际被呈现的卡片触发；同一进程每条消息最多补读一次，失败也算，不重试；
//! - 同时最多 [`MAX_IN_FLIGHT`] 条在途，排队最多 [`MAX_PENDING`] 条，排不下的不记账，下次呈现再来；
//! - 记账最多 [`MAX_TRACKED`] 条，满了不再补读，内存有界。
//!
//! 取舍：也可以在同步游标层面补发（把关键事件游标退回去重放类型 17），但那要为每个会话重放全部
//! 状态事件（多数卡片从不被看到），受事件保留期限制，还要一个持久的升级标记；这里只读被看到的卡片，
//! 拿到的是侧表里的最新快照。

use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use crate::domain::{MessageStore, OperationApplyResult};
use crate::kernel::CurrentUserIdStore;
use crate::kernel::event::{EventBus, MessageEvent, SdkEvent};
use crate::model::IMMessage;
use crate::shared::error::Result;

/// 同时在途的补读。
pub(crate) const MAX_IN_FLIGHT: usize = 2;
/// 等待补读的卡片。
pub(crate) const MAX_PENDING: usize = 32;
/// 本进程记账的消息（补读过或在排队）。
pub(crate) const MAX_TRACKED: usize = 4096;

pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 按会话序号从服务端读一条消息；不存在或不可见时为 `None`。
pub(crate) trait RemoteMessageLookup: Send + Sync {
    fn message_at_seq<'a>(
        &'a self,
        conversation_id: &'a str,
        seq: u64,
    ) -> BoxFuture<'a, Result<Option<IMMessage>>>;
}

struct Target {
    conversation_id: String,
    server_id: String,
    seq: u64,
    entity: String,
    /// 呈现时的登录用户；会话换了就不再写。
    user: String,
}

#[derive(Default)]
struct Queue {
    tracked: HashSet<String>,
    pending: VecDeque<Target>,
    workers: usize,
}

pub(crate) struct PluginStateRefetch {
    lookup: Arc<dyn RemoteMessageLookup>,
    messages: Arc<dyn MessageStore>,
    bus: EventBus,
    current_user: CurrentUserIdStore,
    queue: Mutex<Queue>,
}

impl PluginStateRefetch {
    pub(crate) fn new(
        lookup: Arc<dyn RemoteMessageLookup>,
        messages: Arc<dyn MessageStore>,
        bus: EventBus,
        current_user: CurrentUserIdStore,
    ) -> Self {
        Self {
            lookup,
            messages,
            bus,
            current_user,
            queue: Mutex::new(Queue::default()),
        }
    }

    /// 呈现一张插件卡片时调用：`stored` 是它所在的本地消息，`entity` 是卡片载荷在 `entityPath`
    /// 处的实体，`user` 是当前登录用户。缺共享层时安排一次后台补读，立即返回。
    pub(crate) fn on_present(self: &Arc<Self>, stored: &IMMessage, entity: &str, user: &str) {
        let server_id = stored.server_id.trim();
        if stored.plugin_shared_state_version() > 0
            || server_id.is_empty()
            || stored.conversation_seq == 0
            || user.is_empty()
        {
            return;
        }
        let mut queue = self
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if queue.tracked.len() >= MAX_TRACKED
            || queue.pending.len() >= MAX_PENDING
            || !queue.tracked.insert(server_id.to_owned())
        {
            return;
        }
        queue.pending.push_back(Target {
            conversation_id: stored.conversation_id.clone(),
            server_id: server_id.to_owned(),
            seq: stored.conversation_seq,
            entity: entity.to_owned(),
            user: user.to_owned(),
        });
        if queue.workers >= MAX_IN_FLIGHT {
            return;
        }
        queue.workers += 1;
        drop(queue);
        let this = self.clone();
        crate::shared::util::background::spawn_background(async move { this.drain().await });
    }

    async fn drain(self: Arc<Self>) {
        loop {
            let target = {
                let mut queue = self
                    .queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                match queue.pending.pop_front() {
                    Some(target) => target,
                    None => {
                        queue.workers -= 1;
                        return;
                    }
                }
            };
            self.refetch(&target).await;
        }
    }

    async fn same_session(&self, target: &Target) -> bool {
        self.current_user.read().await.trim() == target.user
    }

    async fn refetch(&self, target: &Target) {
        if !self.same_session(target).await {
            return;
        }
        let remote = match self
            .lookup
            .message_at_seq(&target.conversation_id, target.seq)
            .await
        {
            Ok(Some(remote)) if remote.server_id == target.server_id => remote,
            Ok(_) => {
                tracing::debug!(
                    target: "flare_sdk.plugin",
                    server_id = %target.server_id,
                    seq = target.seq,
                    "plugin card refetch: the server has no such message at this seq"
                );
                return;
            }
            Err(error) => {
                tracing::debug!(
                    target: "flare_sdk.plugin",
                    server_id = %target.server_id,
                    %error,
                    "plugin card refetch failed; not retried in this process"
                );
                return;
            }
        };
        let Some(state) = remote
            .plugin_shared_state(Some(&target.entity))
            .filter(|state| state.entity == target.entity)
        else {
            return;
        };
        if !self.same_session(target).await {
            return;
        }
        match self
            .messages
            .apply_plugin_shared_state(&target.server_id, &state)
            .await
        {
            Ok(OperationApplyResult::Applied) => {
                self.bus
                    .publish(SdkEvent::Message(MessageEvent::PluginStateChanged {
                        conversation_id: target.conversation_id.clone(),
                        message_id: target.server_id.clone(),
                        version: state.version,
                    }));
            }
            // An event got there first, or the message is gone locally.
            Ok(_) => {}
            Err(error) => tracing::warn!(
                target: "flare_sdk.plugin",
                server_id = %target.server_id,
                %error,
                "plugin card refetch: write shared state failed"
            ),
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::domain::{MessageReader, MessageWriter};
    use crate::infrastructure::persistence::memory_im::MemoryMessageStore;
    use crate::kernel::event::EventReceiver;
    use crate::model::message::{PLUGIN_SHARED_STATE_JSON_KEY, PLUGIN_SHARED_STATE_VERSION_KEY};
    use flare_plugin_protocol::state::SharedState;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::sync::{RwLock, Semaphore};

    /// Answers every lookup with `remote` (or an error), counting calls and the most in flight.
    struct FakeLookup {
        remote: Mutex<Option<IMMessage>>,
        fail: bool,
        gate: Semaphore,
        calls: AtomicUsize,
        in_flight: AtomicUsize,
        max_in_flight: AtomicUsize,
    }
    impl FakeLookup {
        fn new(remote: Option<IMMessage>, fail: bool, open: bool) -> Arc<Self> {
            Arc::new(Self {
                remote: Mutex::new(remote),
                fail,
                gate: Semaphore::new(if open { Semaphore::MAX_PERMITS } else { 0 }),
                calls: AtomicUsize::new(0),
                in_flight: AtomicUsize::new(0),
                max_in_flight: AtomicUsize::new(0),
            })
        }
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }
    impl RemoteMessageLookup for FakeLookup {
        fn message_at_seq<'a>(
            &'a self,
            _conversation_id: &'a str,
            seq: u64,
        ) -> BoxFuture<'a, Result<Option<IMMessage>>> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                self.max_in_flight.fetch_max(now, Ordering::SeqCst);
                let _permit = self.gate.acquire().await.unwrap();
                self.in_flight.fetch_sub(1, Ordering::SeqCst);
                if self.fail {
                    return Err(crate::shared::error::FlareError::system("offline"));
                }
                Ok(self.remote.lock().unwrap().clone().map(|mut remote| {
                    remote.conversation_seq = seq;
                    remote
                }))
            })
        }
    }

    fn card(server_id: &str, seq: u64) -> IMMessage {
        let mut card = IMMessage::new(flare_proto::common::Message::default());
        card.server_id = server_id.into();
        card.client_msg_id = format!("client-{server_id}");
        card.conversation_id = "group-1".into();
        card.conversation_seq = seq;
        card.content = Some(crate::model::Elem::Custom(
            crate::content::message_elem::CustomElem {
                r#type: "flare.plugin.message.v1".into(),
                payload: serde_json::to_vec(&serde_json::json!({
                    "pluginId": "example.poll", "contributionId": "example.poll.card",
                    "schemaVersion": 1, "summary": "[Poll] Lunch?",
                    "payload": {"pollId": "poll-1", "counts": [0, 0]}
                }))
                .unwrap(),
                description: String::new(),
                metadata: Default::default(),
            },
        ));
        card
    }

    /// The card as Reader returns it now: the shared layer on its extra.
    fn served(server_id: &str, version: u64) -> IMMessage {
        let mut remote = card(server_id, 7);
        remote.attributes.insert(
            PLUGIN_SHARED_STATE_JSON_KEY.into(),
            r#"{"counts":[2,1],"voterCount":3}"#.into(),
        );
        remote
            .attributes
            .insert(PLUGIN_SHARED_STATE_VERSION_KEY.into(), version.to_string());
        remote
    }

    struct Fixture {
        refetch: Arc<PluginStateRefetch>,
        store: Arc<MemoryMessageStore>,
        events: EventReceiver,
    }
    fn fixture(lookup: Arc<FakeLookup>) -> Fixture {
        let store = Arc::new(MemoryMessageStore::new());
        let bus = EventBus::new();
        let events = bus.subscribe();
        let refetch = Arc::new(PluginStateRefetch::new(
            lookup,
            store.clone(),
            bus,
            Arc::new(RwLock::new("u1".into())),
        ));
        Fixture {
            refetch,
            store,
            events,
        }
    }
    impl Fixture {
        fn state_changes(&mut self) -> Vec<(String, u64)> {
            let mut out = Vec::new();
            while let Ok(event) = self.events.try_recv() {
                if let SdkEvent::Message(MessageEvent::PluginStateChanged {
                    message_id,
                    version,
                    ..
                }) = event
                {
                    out.push((message_id, version));
                }
            }
            out
        }
        fn idle(&self) -> bool {
            let queue = self.refetch.queue.lock().unwrap();
            queue.workers == 0 && queue.pending.is_empty()
        }
        async fn settle(&self) {
            for _ in 0..200 {
                if self.idle() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            panic!("refetch did not settle");
        }
    }

    #[tokio::test]
    async fn a_card_missing_its_shared_layer_is_refetched_once() {
        let lookup = FakeLookup::new(Some(served("server-1", 4)), false, true);
        let mut f = fixture(lookup.clone());
        let stored = card("server-1", 7);
        f.store.save_one(&stored).await.unwrap();

        f.refetch.on_present(&stored, "poll-1", "u1");
        f.settle().await;
        assert_eq!(lookup.calls(), 1);
        let healed = f.store.get("server-1").await.unwrap().unwrap();
        assert_eq!(healed.plugin_shared_state_version(), 4);
        assert_eq!(
            healed.plugin_shared_state(Some("poll-1")).unwrap().values["voterCount"],
            3
        );
        assert_eq!(f.state_changes(), vec![("server-1".to_string(), 4)]);

        // Presented again from the stale copy a host still holds: never a second read.
        f.refetch.on_present(&stored, "poll-1", "u1");
        f.settle().await;
        assert_eq!(lookup.calls(), 1, "at most once per message and process");
        assert!(f.state_changes().is_empty());
    }

    #[tokio::test]
    async fn a_card_with_a_shared_layer_or_no_server_identity_is_not_refetched() {
        let lookup = FakeLookup::new(Some(served("server-1", 4)), false, true);
        let f = fixture(lookup.clone());
        let mut with_layer = card("server-1", 7);
        f.store.save_one(&with_layer).await.unwrap();
        f.store
            .apply_plugin_shared_state(
                "server-1",
                &SharedState {
                    entity: "poll-1".into(),
                    version: 2,
                    values: serde_json::json!({"counts": [1, 0]})
                        .as_object()
                        .unwrap()
                        .clone(),
                },
            )
            .await
            .unwrap();
        with_layer = f.store.get("server-1").await.unwrap().unwrap();
        f.refetch.on_present(&with_layer, "poll-1", "u1");

        let mut pending = card("", 0);
        pending.client_msg_id = "client-pending".into();
        f.refetch.on_present(&pending, "poll-1", "u1");
        f.refetch.on_present(&card("server-2", 0), "poll-1", "u1");
        f.refetch.on_present(&card("server-3", 9), "poll-1", "");
        f.settle().await;
        assert_eq!(lookup.calls(), 0);
    }

    #[tokio::test]
    async fn a_failed_refetch_is_not_retried_and_in_flight_reads_are_capped() {
        let lookup = FakeLookup::new(None, true, false);
        let mut f = fixture(lookup.clone());
        let cards: Vec<_> = (1..=10).map(|n| card(&format!("server-{n}"), n)).collect();
        for stored in &cards {
            f.store.save_one(stored).await.unwrap();
            f.refetch.on_present(stored, "poll-1", "u1");
        }
        // Hold the reads: only MAX_IN_FLIGHT start, the rest wait their turn.
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(lookup.calls(), MAX_IN_FLIGHT);
        lookup.gate.add_permits(Semaphore::MAX_PERMITS / 2);
        f.settle().await;
        assert_eq!(lookup.calls(), 10);
        assert_eq!(lookup.max_in_flight.load(Ordering::SeqCst), MAX_IN_FLIGHT);

        // Every card failed; presenting them again (a re-render storm) reads nothing more.
        for _ in 0..3 {
            for stored in &cards {
                f.refetch.on_present(stored, "poll-1", "u1");
            }
        }
        f.settle().await;
        assert_eq!(
            lookup.calls(),
            10,
            "a failure is not retried in this process"
        );
        assert!(f.state_changes().is_empty());
        assert_eq!(
            f.store
                .get("server-1")
                .await
                .unwrap()
                .unwrap()
                .plugin_shared_state_version(),
            0
        );
    }

    #[tokio::test]
    async fn the_queue_is_bounded_and_overflow_waits_for_the_next_presentation() {
        let lookup = FakeLookup::new(None, false, false);
        let f = fixture(lookup.clone());
        let cards: Vec<_> = (1..=(MAX_IN_FLIGHT + MAX_PENDING + 5) as u64)
            .map(|n| card(&format!("server-{n}"), n))
            .collect();
        for stored in &cards {
            f.refetch.on_present(stored, "poll-1", "u1");
            // Let the workers take their first card before the queue fills up.
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        {
            let queue = f.refetch.queue.lock().unwrap();
            assert_eq!(queue.pending.len(), MAX_PENDING);
            assert_eq!(queue.tracked.len(), MAX_IN_FLIGHT + MAX_PENDING);
        }
        lookup.gate.add_permits(Semaphore::MAX_PERMITS / 2);
        f.settle().await;
        let calls = lookup.calls();
        assert_eq!(calls, MAX_IN_FLIGHT + MAX_PENDING);
        // A card left out while the queue was full is read when presented again.
        f.refetch.on_present(cards.last().unwrap(), "poll-1", "u1");
        f.settle().await;
        assert_eq!(lookup.calls(), calls + 1);
    }

    #[tokio::test]
    async fn a_changed_session_writes_nothing() {
        let lookup = FakeLookup::new(Some(served("server-1", 4)), false, true);
        let store = Arc::new(MemoryMessageStore::new());
        let user = Arc::new(RwLock::new("u1".to_string()));
        let refetch = Arc::new(PluginStateRefetch::new(
            lookup.clone(),
            store.clone(),
            EventBus::new(),
            user.clone(),
        ));
        let stored = card("server-1", 7);
        store.save_one(&stored).await.unwrap();
        *user.write().await = "u2".into();
        refetch.on_present(&stored, "poll-1", "u1");
        for _ in 0..50 {
            if refetch.queue.lock().unwrap().workers == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(lookup.calls(), 0);
        assert_eq!(
            store
                .get("server-1")
                .await
                .unwrap()
                .unwrap()
                .plugin_shared_state_version(),
            0
        );
    }
}
