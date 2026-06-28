//! Router：下行载荷 → EventBus。与 flare-proto 对齐：消息=MessagePush，事件=Event/EventEnvelope，回执=SendAck；同步=`DataPacket.sync_response`→`SyncRes`，扩展=`DataPacket.user_custom`/`CustomData`。

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use flare_proto::common::MessageDeleteEvent;
use flare_proto::common::MessageStatus;
use flare_proto::common::event::Payload as EventPayload;
use prost::Message;
use tokio::sync::{Mutex as AsyncMutex, RwLock, mpsc};
use tracing::warn;

use crate::application::notification::{
    NotificationInboundPipeline, partition_notification_durability,
};
use crate::application::projections::ConversationProjectionApplier;
use crate::application::services::EventDeduper;
use crate::application::services::IncomingMessageConverger;
use crate::application::services::MessageDeduper;
use crate::domain::{DEFAULT_SYNC_LIMIT, SyncCursorVo, local_cleared_through_seq};
use crate::infrastructure::persistence::StoreProvider;
use crate::infrastructure::protocol::DownlinkPayload;
use crate::kernel::event::{ConversationEvent, EventBus, ExtensionEvent, MessageEvent, SdkEvent};
use crate::kernel::{ReliableSendQueuePort, SessionSyncRunner, SyncResponseHandler};
use crate::model::IMMessage;
use crate::runtime::ReliableSendQueue;
use crate::shared::util::spawn_background;
use crate::spi::metrics::{MetricLabel, MetricsRecorder};

const SEQ_REPAIR_BASE_BACKOFF_MS: u64 = 1_000;
const SEQ_REPAIR_MAX_BACKOFF_MS: u64 = 60_000;
const SEQ_REPAIR_IDLE_TTL_MS: u64 = 10 * 60 * 1_000;
const SEQ_REPAIR_MAX_TRACKED_CONVERSATIONS: usize = 2_048;
const WATERLINE_CONTROL_TYPES: &[&str] = &[
    "sync.waterline",
    "conversation.waterline",
    "conversation_waterline",
    "waterline",
];
const WATERLINE_ATTR_KIND: &str = "kind";
const WATERLINE_ATTR_CONVERSATION_ID: &str = "conversation_id";
const WATERLINE_ATTR_CONVERSATION_ID_CAMEL: &str = "conversationId";
const WATERLINE_ATTR_MAX_SEQ: &str = "max_conversation_seq";
const WATERLINE_ATTR_MAX_SEQ_CAMEL: &str = "maxConversationSeq";

#[derive(Debug, Clone, Default)]
struct SeqRepairState {
    in_flight: bool,
    last_gap_after: u64,
    last_progress_seq: u64,
    attempts: u32,
    next_attempt_at_ms: u64,
    updated_at_ms: u64,
}

#[derive(Debug, Default)]
struct WaterlinePullState {
    in_flight: bool,
    pending: bool,
    pending_target_seq: u64,
}

/// 持久化 worker 的有界队列容量：视图已先行投递（A2 win#1），此队列只为「socket 读循环 vs 落盘 I/O」
/// 解耦 + 背压。满时 `dispatch` 在 `send().await` 上自然背压（只压上行读取，视图已先行更新）。
const PERSIST_QUEUE_CAP: usize = 256;

/// 后台串行持久化任务：一批 durable 消息 + 当时的用户 id。
struct PersistJob {
    messages: Vec<IMMessage>,
    user_id: String,
}

pub struct Dispatcher {
    bus: EventBus,
    reliable_queue: Option<Arc<ReliableSendQueue>>,
    sync_response_handler: Option<Arc<dyn SyncResponseHandler>>,
    session_sync: Option<Arc<dyn SessionSyncRunner>>,
    /// 用于推送消息落库，使双端/查库与事件一致
    stores: Option<StoreProvider>,
    current_user_id: Arc<RwLock<String>>,
    event_deduper: EventDeduper,
    notification_pipeline: NotificationInboundPipeline,
    incoming_message_converger: Option<IncomingMessageConverger>,
    conversation_projection_applier: Option<ConversationProjectionApplier>,
    seq_repair_state: Arc<AsyncMutex<HashMap<String, SeqRepairState>>>,
    waterline_pull_state: Arc<AsyncMutex<HashMap<String, WaterlinePullState>>>,
    /// win#2：有界串行持久化 worker 的入队端。生产路径经 `start_persist_worker` 装载；
    /// 测试 harness 不装载 → `dispatch` 回退内联持久化（保持同步语义，既有断言不受影响）。
    persist_tx: OnceLock<mpsc::Sender<PersistJob>>,
    metrics: MetricsRecorder,
}

impl Dispatcher {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        bus: EventBus,
        reliable_queue: Option<Arc<ReliableSendQueue>>,
        sync_response_handler: Option<Arc<dyn SyncResponseHandler>>,
        session_sync: Option<Arc<dyn SessionSyncRunner>>,
        stores: Option<StoreProvider>,
        current_user_id: Arc<RwLock<String>>,
        event_deduper: EventDeduper,
        notification_pipeline: NotificationInboundPipeline,
        metrics: MetricsRecorder,
    ) -> Self {
        let reliable_queue_port: Option<Arc<dyn ReliableSendQueuePort>> = reliable_queue
            .clone()
            .map(|queue| -> Arc<dyn ReliableSendQueuePort> { queue });
        let incoming_message_converger = stores.as_ref().map(|provider| {
            IncomingMessageConverger::new(
                provider.messages.clone(),
                provider.conversations.clone(),
                bus.clone(),
                reliable_queue_port.clone(),
            )
        });
        let conversation_projection_applier = stores
            .as_ref()
            .map(|provider| ConversationProjectionApplier::new(provider.clone(), bus.clone()));
        Self {
            bus,
            reliable_queue,
            sync_response_handler,
            session_sync,
            stores,
            current_user_id,
            event_deduper,
            notification_pipeline,
            incoming_message_converger,
            conversation_projection_applier,
            seq_repair_state: Arc::new(AsyncMutex::new(HashMap::new())),
            waterline_pull_state: Arc::new(AsyncMutex::new(HashMap::new())),
            persist_tx: OnceLock::new(),
            metrics,
        }
    }

    pub fn bus(&self) -> &EventBus {
        &self.bus
    }

    /// 启动有界**串行**持久化 worker（A2 win#2）。生产路径在建连后调用一次；测试 harness 不调用
    /// → `dispatch` 走内联持久化分支。单 worker 串行消费，保持 `save_batch` + 游标推进无竞态
    /// （与既有 inline 时序一致，只是搬到独立任务，使 socket 读循环不被落盘 I/O 头阻塞）。
    /// worker 持 `Weak<Dispatcher>`，仅按任务短暂 `upgrade`，不跨 `recv().await` 持 `Arc`；
    /// Dispatcher 释放 → `persist_tx`(Sender) 释放 → channel 关闭 → worker 自然退出（无需 Drop-abort）。
    pub(crate) fn start_persist_worker(self: &Arc<Self>) {
        if self.stores.is_none() {
            return;
        }
        let (tx, mut rx) = mpsc::channel::<PersistJob>(PERSIST_QUEUE_CAP);
        if self.persist_tx.set(tx).is_err() {
            return; // 已启动，幂等返回
        }
        let weak = Arc::downgrade(self);
        spawn_background(async move {
            while let Some(job) = rx.recv().await {
                let Some(this) = weak.upgrade() else {
                    break;
                };
                this.persist_durable_batch(&job.messages, &job.user_id)
                    .await;
                drop(this); // 不跨下一次 recv().await 持 Arc，确保 Dispatcher 可释放
            }
        });
    }
}

fn operation_seq(event: &flare_proto::common::Event) -> Option<u64> {
    (event.conversation_seq > 0).then_some(event.conversation_seq)
}

fn max_contiguous_seq(known_seq: u64, seqs: &[u64]) -> u64 {
    let mut sorted = seqs
        .iter()
        .copied()
        .filter(|seq| *seq > known_seq)
        .collect::<Vec<_>>();
    sorted.sort_unstable();
    sorted.dedup();

    let mut cursor = known_seq;
    for seq in sorted {
        if seq == cursor + 1 {
            cursor = seq;
            continue;
        }
        if seq > cursor + 1 {
            break;
        }
    }
    cursor
}

fn first_gap_after(known_seq: u64, seqs: &[u64]) -> Option<u64> {
    let contiguous = max_contiguous_seq(known_seq, seqs);
    let has_later = seqs.iter().any(|seq| *seq > contiguous + 1);
    has_later.then_some(contiguous)
}

fn first_internal_gap_after(seqs: &[u64]) -> Option<u64> {
    let mut sorted = seqs
        .iter()
        .copied()
        .filter(|seq| *seq > 0)
        .collect::<Vec<_>>();
    sorted.sort_unstable();
    sorted.dedup();
    for pair in sorted.windows(2) {
        if pair[1] > pair[0] + 1 {
            return Some(pair[0]);
        }
    }
    None
}

fn first_internal_gap_after_from(base_seq: u64, seqs: &[u64]) -> Option<u64> {
    let mut sorted = seqs
        .iter()
        .copied()
        .filter(|seq| *seq >= base_seq)
        .collect::<Vec<_>>();
    sorted.sort_unstable();
    sorted.dedup();
    first_internal_gap_after(&sorted)
}

fn seq_repair_backoff_ms(attempt: u32) -> u64 {
    let shift = attempt.saturating_sub(1).min(6);
    SEQ_REPAIR_BASE_BACKOFF_MS
        .saturating_mul(1_u64 << shift)
        .min(SEQ_REPAIR_MAX_BACKOFF_MS)
}

fn prune_seq_repair_state(states: &mut HashMap<String, SeqRepairState>, now_ms: u64) {
    states.retain(|_, state| {
        state.in_flight
            || state.updated_at_ms.saturating_add(SEQ_REPAIR_IDLE_TTL_MS) > now_ms
            || state.next_attempt_at_ms > now_ms
    });

    if states.len() <= SEQ_REPAIR_MAX_TRACKED_CONVERSATIONS {
        return;
    }

    let mut idle_entries = states
        .iter()
        .filter(|(_, state)| !state.in_flight)
        .map(|(conversation_id, state)| (conversation_id.clone(), state.updated_at_ms))
        .collect::<Vec<_>>();
    idle_entries.sort_unstable_by_key(|(_, updated_at_ms)| *updated_at_ms);

    let excess = states
        .len()
        .saturating_sub(SEQ_REPAIR_MAX_TRACKED_CONVERSATIONS);
    for (conversation_id, _) in idle_entries.into_iter().take(excess) {
        states.remove(&conversation_id);
    }
}

async fn local_waterline_reached_with_stores(
    stores: &Option<StoreProvider>,
    conversation_id: &str,
    target_seq: u64,
) -> bool {
    if target_seq == 0 {
        return false;
    }
    let Some(stores) = stores else {
        return false;
    };
    if stores
        .conversations
        .get_local_max_seq(conversation_id)
        .await
        .map(|seq| seq >= target_seq)
        .unwrap_or(false)
    {
        return true;
    }

    stores
        .conversations
        .get(conversation_id)
        .await
        .map(|conversation| {
            conversation
                .map(|conversation| {
                    local_cleared_through_seq(&conversation.ext).max(conversation.visible_after_seq)
                        >= target_seq
                })
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

fn is_waterline_ping(kind: &str, attributes: &HashMap<String, String>) -> bool {
    WATERLINE_CONTROL_TYPES
        .iter()
        .any(|candidate| kind.eq_ignore_ascii_case(candidate))
        || attr_non_empty(attributes, WATERLINE_ATTR_KIND).is_some_and(|value| {
            WATERLINE_CONTROL_TYPES
                .iter()
                .any(|candidate| value.eq_ignore_ascii_case(candidate))
        })
}

fn attr_non_empty<'a>(attributes: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    attributes
        .get(key)
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn attr_u64(attributes: &HashMap<String, String>, key: &str) -> Option<u64> {
    attr_non_empty(attributes, key)?.parse().ok()
}

fn now_ms() -> u64 {
    crate::shared::util::now_millis()
}

mod dispatch;
mod seq_repair;
#[cfg(test)]
mod tests;
mod waterline;
