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

    async fn local_tail_seqs_before_incoming(
        &self,
        conversation_id: &str,
        min_incoming_seq: u64,
    ) -> Vec<u64> {
        let Some(stores) = &self.stores else {
            return Vec::new();
        };
        if min_incoming_seq <= 1 {
            return Vec::new();
        }
        match stores
            .messages
            .get_by_conversation(conversation_id, min_incoming_seq, 100)
            .await
        {
            Ok(messages) => {
                let mut seqs = messages
                    .into_iter()
                    .map(|message| message.conversation_seq)
                    .filter(|seq| *seq > 0)
                    .collect::<Vec<_>>();
                seqs.sort_unstable();
                seqs.dedup();
                seqs
            }
            Err(error) => {
                warn!(
                    conversation_id = %conversation_id,
                    min_incoming_seq,
                    error = %error,
                    "读取实时消息前置本地 seq 失败，降级使用同步游标"
                );
                Vec::new()
            }
        }
    }

    /// 持久化一批 durable 消息（save_batch → 会话投影），成功后推进同步游标。
    /// A2：已与视图投递（finish_batch）解耦；win#2 将把对本方法的调用移到有界串行后台 worker，
    /// 使 socket 读循环不再被持久化 I/O 头阻塞。单 worker 串行可保持游标推进无竞态。
    async fn persist_durable_batch(&self, durable_messages: &[IMMessage], current_user_id: &str) {
        let Some(stores) = self.stores.as_ref() else {
            return;
        };
        if durable_messages.is_empty() {
            return;
        }
        let mut durable = true;
        if let Err(e) = stores.messages.save_batch(durable_messages).await {
            warn!(error = %e, "MessagePush save_batch failed");
            durable = false;
        } else if let Some(applier) = &self.conversation_projection_applier
            && let Err(e) = applier
                .apply_messages(durable_messages, current_user_id)
                .await
        {
            warn!(error = %e, "MessagePush conversation projection failed");
            durable = false;
        }
        if durable {
            self.repair_message_seq_after_persist(durable_messages)
                .await;
        }
    }

    async fn repair_message_seq_after_persist(&self, messages: &[IMMessage]) {
        let Some(stores) = &self.stores else {
            return;
        };
        let user_id = self.current_user_id.read().await.clone();
        if user_id.trim().is_empty() {
            return;
        }

        let mut by_conversation = HashMap::<String, Vec<u64>>::new();
        for message in messages {
            if message.conversation_id.trim().is_empty() || message.conversation_seq == 0 {
                continue;
            }
            by_conversation
                .entry(message.conversation_id.clone())
                .or_default()
                .push(message.conversation_seq);
        }

        for (conversation_id, mut seqs) in by_conversation {
            seqs.sort_unstable();
            seqs.dedup();
            let cursor_seq = match stores
                .cursors
                .get_conversation_cursor(&user_id, &conversation_id)
                .await
            {
                Ok(cursor) => cursor.map(|c| c.last_seq).unwrap_or(0),
                Err(error) => {
                    warn!(
                        conversation_id = %conversation_id,
                        error = %error,
                        "读取会话消息游标失败，跳过实时消息 seq 补偿"
                    );
                    continue;
                }
            };
            let min_incoming_seq = seqs.first().copied().unwrap_or_default();
            let local_tail_seqs = self
                .local_tail_seqs_before_incoming(&conversation_id, min_incoming_seq)
                .await;
            let local_before_seq = local_tail_seqs.last().copied().unwrap_or(0);
            let base_seq = if local_before_seq > 0 {
                local_before_seq
            } else {
                cursor_seq
            };
            let mut continuity_window = local_tail_seqs;
            continuity_window.extend(seqs.iter().copied());
            continuity_window.sort_unstable();
            continuity_window.dedup();
            let window_gap_after = first_internal_gap_after_from(base_seq, &continuity_window)
                .or_else(|| first_gap_after(base_seq, &seqs));
            let contiguous_seq = if window_gap_after.is_some() {
                base_seq.min(window_gap_after.unwrap_or(base_seq))
            } else {
                max_contiguous_seq(base_seq, &seqs)
            };
            if contiguous_seq > cursor_seq
                && let Err(error) = stores
                    .cursors
                    .save_conversation_cursor(&SyncCursorVo {
                        user_id: user_id.clone(),
                        conversation_id: conversation_id.clone(),
                        last_seq: contiguous_seq,
                        synced_at: now_ms(),
                    })
                    .await
            {
                warn!(
                    conversation_id = %conversation_id,
                    contiguous_seq,
                    error = %error,
                    "保存实时消息连续游标失败"
                );
            }

            if let Some(first_gap_after) = window_gap_after {
                warn!(
                    conversation_id = %conversation_id,
                    cursor_seq,
                    local_before_seq,
                    base_seq,
                    first_gap_after,
                    max_incoming_seq = seqs.last().copied().unwrap_or_default(),
                    "实时消息 seq 出现缺口，后台触发单会话补拉"
                );
                if let Some(sync) = self.session_sync.clone() {
                    let now = now_ms();
                    let mut guard = self.seq_repair_state.lock().await;
                    prune_seq_repair_state(&mut guard, now);
                    if !guard.contains_key(&conversation_id)
                        && guard.len() >= SEQ_REPAIR_MAX_TRACKED_CONVERSATIONS
                    {
                        warn!(
                            conversation_id = %conversation_id,
                            tracked = guard.len(),
                            limit = SEQ_REPAIR_MAX_TRACKED_CONVERSATIONS,
                            "实时消息缺口补拉状态已达上限，跳过本次新会话补拉触发"
                        );
                        continue;
                    }
                    let state = guard.entry(conversation_id.clone()).or_default();
                    state.updated_at_ms = now;
                    let has_progress = contiguous_seq > state.last_progress_seq;
                    if state.in_flight {
                        continue;
                    }
                    if state.last_gap_after == first_gap_after
                        && !has_progress
                        && now < state.next_attempt_at_ms
                    {
                        warn!(
                            conversation_id = %conversation_id,
                            first_gap_after,
                            next_attempt_at_ms = state.next_attempt_at_ms,
                            "实时消息缺口补拉处于退避窗口，跳过本次重复触发"
                        );
                        continue;
                    }
                    if state.last_gap_after != first_gap_after || has_progress {
                        state.attempts = 0;
                    }
                    state.in_flight = true;
                    state.last_gap_after = first_gap_after;
                    state.last_progress_seq = state.last_progress_seq.max(contiguous_seq);
                    state.attempts = state.attempts.saturating_add(1);
                    let attempt = state.attempts;
                    state.next_attempt_at_ms = now + seq_repair_backoff_ms(attempt);
                    drop(guard);

                    let repair_state = self.seq_repair_state.clone();
                    spawn_background(async move {
                        let repair_result = sync
                            .request_message_sync_from_seq(
                                &conversation_id,
                                first_gap_after,
                                DEFAULT_SYNC_LIMIT,
                            )
                            .await;
                        if let Err(error) = &repair_result {
                            warn!(
                                conversation_id = %conversation_id,
                                first_gap_after,
                                error = %error,
                                "实时消息缺口补拉失败"
                            );
                        }
                        let mut guard = repair_state.lock().await;
                        if repair_result.is_ok() {
                            guard.remove(&conversation_id);
                        } else if let Some(state) = guard.get_mut(&conversation_id) {
                            state.in_flight = false;
                            state.updated_at_ms = now_ms();
                        }
                    });
                } else {
                    warn!(
                        conversation_id = %conversation_id,
                        first_gap_after,
                        "实时消息出现 seq 缺口，但 SDK 未配置单会话同步器，无法自动补拉"
                    );
                }
            } else {
                self.seq_repair_state.lock().await.remove(&conversation_id);
            }
        }
    }

    async fn should_apply_delete_for_current_user(&self, delete: &MessageDeleteEvent) -> bool {
        let scope = delete.scope.unwrap_or(1);
        if scope != 1 {
            return true;
        }

        let target_user_id = delete.target_user_id.as_deref().unwrap_or_default();
        if target_user_id.is_empty() {
            return true;
        }

        let current = self.current_user_id.read().await.clone();
        !current.is_empty() && current == target_user_id
    }

    async fn maybe_trigger_waterline_pull(
        &self,
        source: &str,
        kind: &str,
        conversation_id: Option<&str>,
        attributes: &HashMap<String, String>,
    ) {
        if !is_waterline_ping(kind, attributes) {
            return;
        }
        let conversation_id = conversation_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .or_else(|| attr_non_empty(attributes, WATERLINE_ATTR_CONVERSATION_ID))
            .or_else(|| attr_non_empty(attributes, WATERLINE_ATTR_CONVERSATION_ID_CAMEL));
        let Some(conversation_id) = conversation_id else {
            warn!(source, kind, "waterline ping missing conversation_id");
            return;
        };
        let target_seq = attr_u64(attributes, WATERLINE_ATTR_MAX_SEQ)
            .or_else(|| attr_u64(attributes, WATERLINE_ATTR_MAX_SEQ_CAMEL))
            .unwrap_or(0);
        if local_waterline_reached_with_stores(&self.stores, conversation_id, target_seq).await {
            tracing::debug!(
                source,
                kind,
                conversation_id,
                target_seq,
                "waterline already reached locally, skip single conversation pull"
            );
            return;
        }
        let Some(sync) = self.session_sync.clone() else {
            warn!(
                source,
                kind,
                conversation_id,
                target_seq,
                "waterline ping received but SDK has no single conversation sync runner"
            );
            return;
        };

        let mut states = self.waterline_pull_state.lock().await;
        let state = states.entry(conversation_id.to_string()).or_default();
        if state.in_flight {
            state.pending = true;
            state.pending_target_seq = state.pending_target_seq.max(target_seq);
            tracing::debug!(
                source,
                kind,
                conversation_id,
                target_seq,
                "waterline pull already in flight, coalescing ping"
            );
            return;
        }
        state.in_flight = true;
        state.pending = false;
        state.pending_target_seq = 0;
        drop(states);

        let states = self.waterline_pull_state.clone();
        let stores = self.stores.clone();
        let conversation_id = conversation_id.to_string();
        let source = source.to_string();
        let kind = kind.to_string();
        spawn_background(async move {
            let mut active_target_seq = target_seq;
            loop {
                let result = sync.request_message_sync(&conversation_id).await;
                if let Err(error) = &result {
                    warn!(
                        source = %source,
                        kind = %kind,
                        conversation_id = %conversation_id,
                        target_seq = active_target_seq,
                        error = %error,
                        "waterline-triggered single conversation pull failed"
                    );
                }

                let pending_target_seq = {
                    let mut states = states.lock().await;
                    let Some(state) = states.get_mut(&conversation_id) else {
                        break;
                    };
                    if !state.pending {
                        states.remove(&conversation_id);
                        break;
                    }
                    state.pending = false;
                    let pending_target_seq = state.pending_target_seq;
                    state.pending_target_seq = 0;
                    pending_target_seq
                };

                if local_waterline_reached_with_stores(
                    &stores,
                    &conversation_id,
                    pending_target_seq,
                )
                .await
                {
                    let mut states = states.lock().await;
                    if let Some(state) = states.get(&conversation_id)
                        && !state.pending
                    {
                        states.remove(&conversation_id);
                        break;
                    }
                }
                active_target_seq = pending_target_seq;
            }
        });
    }

    async fn maybe_trigger_event_envelope_waterline(
        &self,
        envelope: &flare_proto::common::EventEnvelope,
    ) {
        let conversation_id = envelope.conversation_id.trim();
        let target_seq = envelope.max_conversation_seq;
        if conversation_id.is_empty() || target_seq == 0 {
            return;
        }
        let attributes =
            HashMap::from([(WATERLINE_ATTR_MAX_SEQ.to_string(), target_seq.to_string())]);
        self.maybe_trigger_waterline_pull(
            "event_envelope",
            "conversation.waterline",
            Some(conversation_id),
            &attributes,
        )
        .await;
    }

    async fn maybe_trigger_event_waterline(&self, event: &flare_proto::common::Event) {
        let conversation_id = Self::event_conversation_id(event);
        let target_seq = event.conversation_seq;
        if conversation_id.is_empty() || target_seq == 0 {
            return;
        }
        let attributes =
            HashMap::from([(WATERLINE_ATTR_MAX_SEQ.to_string(), target_seq.to_string())]);
        self.maybe_trigger_waterline_pull(
            "event",
            "conversation.waterline",
            Some(conversation_id),
            &attributes,
        )
        .await;
    }

    async fn local_waterline_reached(&self, conversation_id: &str, target_seq: u64) -> bool {
        local_waterline_reached_with_stores(&self.stores, conversation_id, target_seq).await
    }

    fn event_conversation_id(event: &flare_proto::common::Event) -> &str {
        let outer = event.conversation_id.trim();
        if !outer.is_empty() {
            return outer;
        }
        match event.payload.as_ref() {
            Some(EventPayload::Conversation(conversation)) => conversation.conversation_id.trim(),
            _ => "",
        }
    }

    async fn message_operation_conversation_id(
        &self,
        event_conversation_id: &str,
        server_msg_id: &str,
    ) -> String {
        let event_conversation_id = event_conversation_id.trim();
        if !event_conversation_id.is_empty() {
            return event_conversation_id.to_string();
        }
        let server_msg_id = server_msg_id.trim();
        if server_msg_id.is_empty() {
            return String::new();
        }
        let Some(stores) = self.stores.as_ref() else {
            return String::new();
        };
        match stores.messages.get(server_msg_id).await {
            Ok(Some(message)) => message.conversation_id.trim().to_string(),
            Ok(None) => String::new(),
            Err(error) => {
                warn!(error = %error, server_msg_id, "resolve message operation conversation_id failed");
                String::new()
            }
        }
    }

    pub async fn dispatch(
        &self,
        payload: DownlinkPayload,
    ) -> flare_core::common::error::Result<()> {
        match payload {
            DownlinkPayload::MessagePush(push) => {
                self.metrics.counter_with_labels(
                    "dispatcher.payload_total",
                    &[MetricLabel::new("type", "message_push")],
                    1,
                );
                // payload 已按值传入，直接 move 入站 proto，避免在接收热路径上深拷贝整批消息。
                let mut all = Vec::with_capacity(push.messages.len() + push.notifications.len());
                all.extend(push.messages);
                all.extend(push.notifications);
                self.metrics
                    .counter("dispatcher.message_push_items_total", all.len() as u64);
                let mut messages: Vec<IMMessage> = all.into_iter().map(IMMessage::new).collect();
                let current_user_id = self.current_user_id.read().await.clone();
                if let Some(converger) = &self.incoming_message_converger {
                    messages = converger
                        .converge_messages(&current_user_id, messages)
                        .await
                        .map_err(|e| {
                            flare_core::common::error::FlareError::system(e.to_string())
                        })?;
                }
                if !messages.is_empty() {
                    let (durable_messages, ephemeral_messages): (Vec<IMMessage>, Vec<IMMessage>) =
                        partition_notification_durability(messages);
                    // A2/BX-02：先投递到可观察视图（下一帧上屏），再持久化，使「收到→上屏」延迟脱离磁盘 I/O。
                    // 持久化失败时同步游标不前进，既有 seq-gap 补拉（repair_message_seq_after_persist）会重取，
                    // 无需额外对账路径。仅克隆 durable 子集供持久化借用；durable+ephemeral move 进 finish_batch。
                    let durable_for_persist = durable_messages.clone();
                    let mut inbound = durable_messages;
                    inbound.extend(ephemeral_messages);
                    if !inbound.is_empty() {
                        self.notification_pipeline.finish_batch(inbound).await;
                    }
                    // win#2：装载了 worker 则投递到有界串行后台 worker（满则在 send().await 背压，
                    // 只压上行读取——视图已先行投递）；未装载（测试 harness）则内联持久化，保持同步语义。
                    if let Some(tx) = self.persist_tx.get() {
                        let job = PersistJob {
                            messages: durable_for_persist,
                            user_id: current_user_id.clone(),
                        };
                        if let Err(err) = tx.send(job).await {
                            // worker 已退出（连接关闭）→ 回退内联，不丢持久化
                            let PersistJob { messages, user_id } = err.0;
                            self.persist_durable_batch(&messages, &user_id).await;
                        }
                    } else {
                        self.persist_durable_batch(&durable_for_persist, &current_user_id)
                            .await;
                    }
                }
            }
            DownlinkPayload::Event(ev) => {
                self.metrics.counter_with_labels(
                    "dispatcher.payload_total",
                    &[MetricLabel::new("type", "event")],
                    1,
                );
                self.dispatch_single_event(&ev).await;
            }
            DownlinkPayload::EventEnvelope(env) => {
                self.metrics.counter_with_labels(
                    "dispatcher.payload_total",
                    &[MetricLabel::new("type", "event_envelope")],
                    1,
                );
                self.metrics.counter(
                    "dispatcher.event_envelope_items_total",
                    env.events.len() as u64,
                );
                tracing::debug!(
                    event_count = env.events.len(),
                    conversation_id = %env.conversation_id,
                    max_conversation_seq = env.max_conversation_seq,
                    "dispatch EventEnvelope (push/sync)"
                );
                for ev in &env.events {
                    self.dispatch_single_event(ev).await;
                }
                self.maybe_trigger_event_envelope_waterline(&env).await;
            }
            DownlinkPayload::SendAck(ack) => {
                self.metrics.counter_with_labels(
                    "dispatcher.payload_total",
                    &[MetricLabel::new("type", "send_ack")],
                    1,
                );
                let mut ack = ack;
                if ack.conversation_id.trim().is_empty()
                    && let Some(ref stores) = self.stores
                {
                    match stores
                        .messages
                        .get_by_client_msg_id(&ack.client_msg_id)
                        .await
                    {
                        Ok(Some(local)) if !local.conversation_id.trim().is_empty() => {
                            ack.conversation_id = local.conversation_id.clone();
                        }
                        Ok(_) => {}
                        Err(e) => {
                            warn!(error = %e, client_msg_id = %ack.client_msg_id, "enrich send ack conversation_id failed");
                        }
                    }
                }
                if let Some(q) = &self.reliable_queue {
                    let _ = q.on_ack(ack.clone()).await;
                } else {
                    self.bus.publish(SdkEvent::Message(MessageEvent::SendAck {
                        ack: Box::new(ack),
                    }));
                }
            }
            DownlinkPayload::CustomData(data) => {
                self.metrics.counter_with_labels(
                    "dispatcher.payload_total",
                    &[MetricLabel::new("type", "custom_data")],
                    1,
                );
                self.maybe_trigger_waterline_pull(
                    "custom_data",
                    &data.r#type,
                    None,
                    &data.attributes,
                )
                .await;
                self.bus.publish(SdkEvent::Extension(ExtensionEvent {
                    source: "custom".to_string(),
                    event_type: data.r#type.clone(),
                    payload: data.payload.clone(),
                }));
            }
            DownlinkPayload::Capability(packet) => {
                self.metrics.counter_with_labels(
                    "dispatcher.payload_total",
                    &[MetricLabel::new("type", "capability")],
                    1,
                );
                let conversation_id = packet
                    .attributes
                    .get("conversation_id")
                    .cloned()
                    .unwrap_or_default();
                self.bus
                    .publish(SdkEvent::Message(MessageEvent::Capability {
                        conversation_id,
                        packet: Box::new(packet),
                    }));
            }
            DownlinkPayload::RealtimeControl(control) => {
                self.metrics.counter_with_labels(
                    "dispatcher.payload_total",
                    &[MetricLabel::new("type", "realtime_control")],
                    1,
                );
                let conversation_id = control.conversation_id.clone().unwrap_or_default();
                self.maybe_trigger_waterline_pull(
                    "realtime_control",
                    &control.control_type,
                    Some(&conversation_id),
                    &control.attributes,
                )
                .await;
                match control.payload {
                    Some(flare_proto::common::realtime_control_packet::Payload::Typing(typing)) => {
                        self.bus.publish(SdkEvent::Message(MessageEvent::Typing {
                            conversation_id,
                            event: typing,
                        }));
                    }
                    Some(flare_proto::common::realtime_control_packet::Payload::Presence(
                        presence,
                    )) => {
                        self.bus
                            .publish(SdkEvent::Message(MessageEvent::PresenceChanged {
                                conversation_id,
                                event: presence,
                            }));
                    }
                    Some(flare_proto::common::realtime_control_packet::Payload::Custom(data)) => {
                        self.bus.publish(SdkEvent::Extension(ExtensionEvent {
                            source: "realtime_control".to_string(),
                            event_type: data.r#type,
                            payload: data.payload,
                        }));
                    }
                    None => {}
                }
            }
            DownlinkPayload::SyncResp(resp) => {
                self.metrics.counter_with_labels(
                    "dispatcher.payload_total",
                    &[MetricLabel::new("type", "sync_response")],
                    1,
                );
                if let Some(h) = &self.sync_response_handler {
                    h.handle_sync_response(resp).await;
                }
            }
        }
        Ok(())
    }

    /// 分发单条 Event（从 EventEnvelope 或单条 Event 下行复用）
    async fn dispatch_single_event(&self, ev: &flare_proto::common::Event) {
        if !self.event_deduper.record_if_new(ev).await {
            return;
        }
        if !matches!(&ev.payload, Some(EventPayload::Message(_))) {
            self.maybe_trigger_event_waterline(ev).await;
        }
        let mut messages: Vec<IMMessage> = Vec::new();
        if let Some(EventPayload::Message(m)) = &ev.payload {
            messages.push(IMMessage::new(m.clone()));
        }
        if !messages.is_empty() {
            let current_user_id = self.current_user_id.read().await.clone();
            if let Some(converger) = &self.incoming_message_converger {
                match converger
                    .converge_messages(&current_user_id, messages)
                    .await
                {
                    Ok(converged) => messages = converged,
                    Err(error) => {
                        warn!(error = %error, "single event message converge failed");
                        self.event_deduper.forget(ev).await;
                        return;
                    }
                }
            }
            if let Some(ref stores) = self.stores {
                if let Err(e) = stores.messages.save_batch(&messages).await {
                    warn!(error = %e, "single event message save_batch failed");
                    self.event_deduper.forget(ev).await;
                    return;
                } else if let Some(applier) = &self.conversation_projection_applier
                    && let Err(e) = applier.apply_messages(&messages, &current_user_id).await
                {
                    warn!(error = %e, "single event conversation projection failed");
                    self.event_deduper.forget(ev).await;
                    return;
                }
                self.repair_message_seq_after_persist(&messages).await;
            }
            self.notification_pipeline.finish_batch(messages).await;
        }
        let conversation_id = Self::event_conversation_id(ev);
        if let Some(p) = &ev.payload {
            match p {
                EventPayload::Recall(recall) => {
                    if let Some(ref stores) = self.stores
                        && let Err(e) = stores
                            .messages
                            .update_status(&recall.server_msg_id, MessageStatus::Recalled as i32)
                            .await
                    {
                        warn!(
                            error = %e,
                            server_msg_id = %recall.server_msg_id,
                            "Recall: update_status failed; event will be retried"
                        );
                        self.event_deduper.forget(ev).await;
                        return;
                    }
                    let conversation_id = self
                        .message_operation_conversation_id(conversation_id, &recall.server_msg_id)
                        .await;
                    self.bus.publish(SdkEvent::Message(MessageEvent::Recalled {
                        conversation_id,
                        event: recall.clone(),
                    }));
                }
                EventPayload::Edit(edit) => {
                    let mut should_publish = true;
                    let Some(new_content) = edit.new_content.as_ref() else {
                        warn!(
                            server_msg_id = %edit.server_msg_id,
                            "Event Edit missing new_content; event will be retried"
                        );
                        self.event_deduper.forget(ev).await;
                        return;
                    };
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_edit_event(
                                &edit.server_msg_id,
                                new_content.encode_to_vec(),
                                edit.edit_version,
                            )
                            .await
                        {
                            Ok(crate::domain::EditApplyResult::Applied) => {}
                            Ok(crate::domain::EditApplyResult::IgnoredStale) => {
                                should_publish = false;
                            }
                            Ok(crate::domain::EditApplyResult::NotFound) => {
                                warn!(
                                    server_msg_id = %edit.server_msg_id,
                                    "Event Edit: no local row matched; event will be retried after message sync"
                                );
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                            Err(e) => {
                                warn!(error = %e, server_msg_id = %edit.server_msg_id, "Event Edit apply_edit_event failed");
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                        }
                    }
                    if should_publish {
                        self.bus.publish(SdkEvent::Message(MessageEvent::Edited {
                            conversation_id: conversation_id.to_string(),
                            server_msg_id: edit.server_msg_id.clone(),
                            edit_version: Some(edit.edit_version),
                        }));
                    }
                }
                EventPayload::Reaction(reaction) => {
                    let mut should_publish = true;
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_reaction_event(
                                conversation_id,
                                &reaction.server_msg_id,
                                &reaction.user_id,
                                &reaction.emoji,
                                reaction.action,
                                operation_seq(ev),
                            )
                            .await
                        {
                            Ok(crate::domain::OperationApplyResult::IgnoredStale) => {
                                should_publish = false;
                            }
                            Ok(crate::domain::OperationApplyResult::Applied) => {}
                            Ok(crate::domain::OperationApplyResult::NotFound) => {
                                warn!(
                                    server_msg_id = %reaction.server_msg_id,
                                    "Event Reaction: no local row matched; event will be retried after message sync"
                                );
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                            Err(error) => {
                                warn!(error = %error, server_msg_id = %reaction.server_msg_id, "Event Reaction apply failed");
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                        }
                    }
                    if should_publish {
                        self.bus
                            .publish(SdkEvent::Message(MessageEvent::ReactionChanged {
                                conversation_id: conversation_id.to_string(),
                                server_msg_id: reaction.server_msg_id.clone(),
                                user_id: reaction.user_id.clone(),
                                emoji: reaction.emoji.clone(),
                                action: reaction.action,
                            }));
                    }
                }
                EventPayload::Delete(delete) => {
                    if self.should_apply_delete_for_current_user(delete).await {
                        let mut should_publish = true;
                        if let Some(ref stores) = self.stores {
                            match stores
                                .messages
                                .apply_delete_event(&delete.server_msg_id, operation_seq(ev))
                                .await
                            {
                                Ok(crate::domain::OperationApplyResult::IgnoredStale) => {
                                    should_publish = false;
                                }
                                Ok(crate::domain::OperationApplyResult::Applied) => {}
                                Ok(crate::domain::OperationApplyResult::NotFound) => {
                                    warn!(
                                        server_msg_id = %delete.server_msg_id,
                                        "Event Delete: no local row matched; event will be retried after message sync"
                                    );
                                    self.event_deduper.forget(ev).await;
                                    return;
                                }
                                Err(error) => {
                                    warn!(error = %error, server_msg_id = %delete.server_msg_id, "Event Delete apply failed");
                                    self.event_deduper.forget(ev).await;
                                    return;
                                }
                            }
                        }
                        if should_publish {
                            self.bus.publish(SdkEvent::Message(MessageEvent::Deleted {
                                conversation_id: conversation_id.to_string(),
                                event: delete.clone(),
                            }));
                            self.bus.publish(SdkEvent::Extension(ExtensionEvent {
                                source: "event".to_string(),
                                event_type: "message_delete".to_string(),
                                payload: delete.encode_to_vec(),
                            }));
                        }
                    }
                }
                EventPayload::Read(read) => {
                    if let Some(ref stores) = self.stores {
                        let current_user_id = self.current_user_id.read().await.clone();
                        // 对方已读回执：将「自己发送且 seq<=read_seq」的消息落库为已读，
                        // 避免仅靠前端内存态导致会话切换/重启后双对号丢失。
                        if !current_user_id.is_empty()
                            && !read.user_id.is_empty()
                            && read.user_id != current_user_id
                            && read.read_seq > 0
                        {
                            let _ = stores
                                .messages
                                .mark_outgoing_read_upto_seq(
                                    conversation_id,
                                    &current_user_id,
                                    read.read_seq,
                                )
                                .await;
                        }
                    }
                    self.bus
                        .publish(SdkEvent::Message(MessageEvent::ReadReceipt {
                            conversation_id: conversation_id.to_string(),
                            event: read.clone(),
                        }));
                }
                EventPayload::RetentionScheduled(retention_scheduled) => {
                    let mut should_publish = true;
                    let (Some(policy), Some(state)) = (
                        retention_scheduled.policy.as_ref(),
                        retention_scheduled.state.as_ref(),
                    ) else {
                        warn!(
                            message_id = %retention_scheduled.server_msg_id,
                            "Event RetentionScheduled missing policy/state; event will be retried"
                        );
                        self.event_deduper.forget(ev).await;
                        return;
                    };
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_retention_scheduled_event(
                                &retention_scheduled.server_msg_id,
                                policy,
                                state,
                                retention_scheduled.scheduled_at,
                                operation_seq(ev),
                            )
                            .await
                        {
                            Ok(crate::domain::OperationApplyResult::IgnoredStale) => {
                                should_publish = false;
                            }
                            Ok(crate::domain::OperationApplyResult::Applied) => {}
                            Ok(crate::domain::OperationApplyResult::NotFound) => {
                                warn!(
                                    message_id = %retention_scheduled.server_msg_id,
                                    "Event RetentionScheduled: no local row matched; event will be retried after message sync"
                                );
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                            Err(error) => {
                                warn!(error = %error, message_id = %retention_scheduled.server_msg_id, "Event RetentionScheduled apply failed");
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                        }
                    }
                    if should_publish {
                        self.bus
                            .publish(SdkEvent::Message(MessageEvent::RetentionScheduled {
                                conversation_id: conversation_id.to_string(),
                                event: retention_scheduled.clone(),
                            }));
                    }
                }
                EventPayload::RetentionExpired(retention_expired) => {
                    let mut should_publish = true;
                    let Some(state) = retention_expired.state.as_ref() else {
                        warn!(
                            message_id = %retention_expired.server_msg_id,
                            "Event RetentionExpired missing state; event will be retried"
                        );
                        self.event_deduper.forget(ev).await;
                        return;
                    };
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_retention_expired_event(
                                &retention_expired.server_msg_id,
                                state,
                                retention_expired.expired_at,
                                operation_seq(ev),
                            )
                            .await
                        {
                            Ok(crate::domain::OperationApplyResult::IgnoredStale) => {
                                should_publish = false;
                            }
                            Ok(crate::domain::OperationApplyResult::Applied) => {}
                            Ok(crate::domain::OperationApplyResult::NotFound) => {
                                warn!(
                                    message_id = %retention_expired.server_msg_id,
                                    "Event RetentionExpired: no local row matched; event will be retried after message sync"
                                );
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                            Err(error) => {
                                warn!(error = %error, message_id = %retention_expired.server_msg_id, "Event RetentionExpired apply failed");
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                        }
                    }
                    if should_publish {
                        self.bus
                            .publish(SdkEvent::Message(MessageEvent::RetentionExpired {
                                conversation_id: conversation_id.to_string(),
                                event: retention_expired.clone(),
                            }));
                    }
                }
                EventPayload::RetentionPurged(retention_purged) => {
                    let mut should_publish = true;
                    let Some(state) = retention_purged.state.as_ref() else {
                        warn!(
                            message_id = %retention_purged.server_msg_id,
                            "Event RetentionPurged missing state; event will be retried"
                        );
                        self.event_deduper.forget(ev).await;
                        return;
                    };
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_retention_purged_event(
                                &retention_purged.server_msg_id,
                                state,
                                retention_purged.purged_at,
                                operation_seq(ev),
                            )
                            .await
                        {
                            Ok(crate::domain::OperationApplyResult::IgnoredStale) => {
                                should_publish = false;
                            }
                            Ok(crate::domain::OperationApplyResult::Applied) => {}
                            Ok(crate::domain::OperationApplyResult::NotFound) => {
                                warn!(
                                    message_id = %retention_purged.server_msg_id,
                                    "Event RetentionPurged: no local row matched; event will be retried after message sync"
                                );
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                            Err(error) => {
                                warn!(error = %error, message_id = %retention_purged.server_msg_id, "Event RetentionPurged apply failed");
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                        }
                    }
                    if should_publish {
                        self.bus
                            .publish(SdkEvent::Message(MessageEvent::RetentionPurged {
                                conversation_id: conversation_id.to_string(),
                                event: retention_purged.clone(),
                            }));
                    }
                }
                EventPayload::Pin(pin) => {
                    let mut should_publish = true;
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_pin_event(&pin.server_msg_id, true, operation_seq(ev))
                            .await
                        {
                            Ok(crate::domain::OperationApplyResult::IgnoredStale) => {
                                should_publish = false;
                            }
                            Ok(crate::domain::OperationApplyResult::Applied) => {}
                            Ok(crate::domain::OperationApplyResult::NotFound) => {
                                warn!(
                                    server_msg_id = %pin.server_msg_id,
                                    "Event Pin: no local row matched; event will be retried after message sync"
                                );
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                            Err(error) => {
                                warn!(error = %error, server_msg_id = %pin.server_msg_id, "Event Pin apply failed");
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                        }
                    }
                    if should_publish {
                        self.bus.publish(SdkEvent::Message(MessageEvent::Pinned {
                            conversation_id: conversation_id.to_string(),
                            event: pin.clone(),
                        }));
                    }
                }
                EventPayload::Unpin(unpin) => {
                    let mut should_publish = true;
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_pin_event(&unpin.server_msg_id, false, operation_seq(ev))
                            .await
                        {
                            Ok(crate::domain::OperationApplyResult::IgnoredStale) => {
                                should_publish = false;
                            }
                            Ok(crate::domain::OperationApplyResult::Applied) => {}
                            Ok(crate::domain::OperationApplyResult::NotFound) => {
                                warn!(
                                    server_msg_id = %unpin.server_msg_id,
                                    "Event Unpin: no local row matched; event will be retried after message sync"
                                );
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                            Err(error) => {
                                warn!(error = %error, server_msg_id = %unpin.server_msg_id, "Event Unpin apply failed");
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                        }
                    }
                    if should_publish {
                        self.bus.publish(SdkEvent::Message(MessageEvent::Unpinned {
                            conversation_id: conversation_id.to_string(),
                            event: unpin.clone(),
                        }));
                    }
                }
                EventPayload::Mark(mark) => {
                    let mut should_publish = true;
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_mark_event(
                                &mark.server_msg_id,
                                mark.mark_type,
                                if mark.color.trim().is_empty() {
                                    None
                                } else {
                                    Some(mark.color.as_str())
                                },
                                true,
                                operation_seq(ev),
                            )
                            .await
                        {
                            Ok(crate::domain::OperationApplyResult::IgnoredStale) => {
                                should_publish = false;
                            }
                            Ok(crate::domain::OperationApplyResult::Applied) => {}
                            Ok(crate::domain::OperationApplyResult::NotFound) => {
                                warn!(
                                    server_msg_id = %mark.server_msg_id,
                                    "Event Mark: no local row matched; event will be retried after message sync"
                                );
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                            Err(error) => {
                                warn!(error = %error, server_msg_id = %mark.server_msg_id, "Event Mark apply failed");
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                        }
                    }
                    if should_publish {
                        self.bus.publish(SdkEvent::Message(MessageEvent::Marked {
                            conversation_id: conversation_id.to_string(),
                            event: mark.clone(),
                        }));
                    }
                }
                EventPayload::Unmark(unmark) => {
                    let mut should_publish = true;
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_mark_event(
                                &unmark.server_msg_id,
                                unmark.mark_type,
                                None,
                                false,
                                operation_seq(ev),
                            )
                            .await
                        {
                            Ok(crate::domain::OperationApplyResult::IgnoredStale) => {
                                should_publish = false;
                            }
                            Ok(crate::domain::OperationApplyResult::Applied) => {}
                            Ok(crate::domain::OperationApplyResult::NotFound) => {
                                warn!(
                                    server_msg_id = %unmark.server_msg_id,
                                    "Event Unmark: no local row matched; event will be retried after message sync"
                                );
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                            Err(error) => {
                                warn!(error = %error, server_msg_id = %unmark.server_msg_id, "Event Unmark apply failed");
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                        }
                    }
                    if should_publish {
                        self.bus.publish(SdkEvent::Message(MessageEvent::Unmarked {
                            conversation_id: conversation_id.to_string(),
                            event: unmark.clone(),
                        }));
                    }
                }
                EventPayload::Custom(custom) => {
                    self.bus.publish(SdkEvent::Message(MessageEvent::Custom {
                        conversation_id: conversation_id.to_string(),
                        event: custom.clone(),
                    }));
                }
                EventPayload::Conversation(conversation) => {
                    if conversation.unread_count > 0 {
                        self.maybe_trigger_waterline_pull(
                            "conversation_update",
                            "conversation.waterline",
                            Some(conversation_id),
                            &HashMap::new(),
                        )
                        .await;
                    }
                    self.bus
                        .publish(SdkEvent::Conversation(ConversationEvent::Updated {
                            conversation_id: conversation_id.to_string(),
                        }));
                }
                EventPayload::ConversationDelete(_) => {
                    if let Some(stores) = self.stores.as_ref()
                        && let Err(error) = stores.conversations.delete(conversation_id).await
                    {
                        warn!(
                            conversation_id = %conversation_id,
                            error = %error,
                            "ConversationDelete push: local conversation purge failed"
                        );
                    }
                    self.bus
                        .publish(SdkEvent::Conversation(ConversationEvent::Deleted {
                            conversation_id: conversation_id.to_string(),
                        }));
                }
                _ => {}
            }
        }
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

#[cfg(test)]
mod tests {
    use super::{
        Dispatcher, first_gap_after, first_internal_gap_after, first_internal_gap_after_from,
        is_waterline_ping, max_contiguous_seq,
    };
    use super::{
        SEQ_REPAIR_IDLE_TTL_MS, SEQ_REPAIR_MAX_BACKOFF_MS, SEQ_REPAIR_MAX_TRACKED_CONVERSATIONS,
        SeqRepairState, WATERLINE_ATTR_CONVERSATION_ID_CAMEL, WATERLINE_ATTR_KIND,
        WATERLINE_ATTR_MAX_SEQ_CAMEL, attr_non_empty, attr_u64, prune_seq_repair_state,
        seq_repair_backoff_ms,
    };
    use crate::application::notification::{
        NotificationHandlerRegistry, NotificationInboundPipeline,
    };
    use crate::application::services::EventDeduper;
    use crate::application::services::MessageDeduper;
    use crate::application::usecases::SyncApplyUseCase;
    use crate::domain::{
        ConversationReader, ConversationWriter, MessageReader, MessageStore, MessageWriter,
        PendingSendReader, PendingSendVo, PendingSendWriter, SyncCursorReader, SyncCursorVo,
        SyncCursorWriter,
    };
    use crate::infrastructure::persistence::StoreProvider;
    use crate::infrastructure::protocol::DownlinkPayload;
    use crate::infrastructure::protocol::{Codec, PacketSender, ProtobufCodec};
    use crate::kernel::event::{EventBus, MessageEvent, SdkEvent};
    use crate::kernel::{CurrentUserIdStore, SessionSyncRunner};
    use crate::model::IMMessage;
    use crate::runtime::{ReliableSendQueue, ReliableSendQueueConfig};
    use crate::shared::error::Result;
    use crate::spi::metrics::MetricsRecorder;
    use async_trait::async_trait;
    use flare_proto::common::event::Payload as ProtoEventPayload;
    use flare_proto::common::{
        ConversationUpdateEvent, Event, EventEnvelope, MessageDeleteEvent, MessageRecallEvent,
        SendAccepted, SendAck, SendAckDurability, send_ack,
    };
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::{Mutex, Notify, RwLock};
    use tokio::time::{Duration, timeout};

    fn test_notification_pipeline(bus: EventBus) -> NotificationInboundPipeline {
        test_notification_pipeline_with_deduper(bus, MessageDeduper::new(Some(64)))
    }

    fn test_notification_pipeline_with_deduper(
        bus: EventBus,
        message_deduper: MessageDeduper,
    ) -> NotificationInboundPipeline {
        NotificationInboundPipeline::new(
            Arc::new(NotificationHandlerRegistry::new()),
            message_deduper,
            bus,
        )
    }

    fn accepted_ack(
        client_msg_id: &str,
        conversation_id: &str,
        server_msg_id: &str,
        conversation_seq: u64,
    ) -> SendAck {
        SendAck {
            client_msg_id: client_msg_id.to_string(),
            conversation_id: conversation_id.to_string(),
            result: Some(send_ack::Result::Accepted(SendAccepted {
                server_msg_id: server_msg_id.to_string(),
                conversation_seq,
                server_time: 0,
                durability: SendAckDurability::Persisted as i32,
            })),
            ..Default::default()
        }
    }

    fn accepted_server_msg_id(ack: &SendAck) -> Option<&str> {
        match ack.result.as_ref() {
            Some(send_ack::Result::Accepted(accepted)) => Some(accepted.server_msg_id.as_str()),
            _ => None,
        }
    }

    struct RecordingSessionSyncRunner {
        message_sync_calls: Mutex<Vec<String>>,
        notify: Notify,
    }

    impl RecordingSessionSyncRunner {
        fn new() -> Self {
            Self {
                message_sync_calls: Mutex::new(Vec::new()),
                notify: Notify::new(),
            }
        }

        async fn message_sync_calls(&self) -> Vec<String> {
            self.message_sync_calls.lock().await.clone()
        }
    }

    struct BlockingSessionSyncRunner {
        message_sync_calls: Mutex<Vec<String>>,
        notify: Notify,
        release_first: Notify,
    }

    impl BlockingSessionSyncRunner {
        fn new() -> Self {
            Self {
                message_sync_calls: Mutex::new(Vec::new()),
                notify: Notify::new(),
                release_first: Notify::new(),
            }
        }

        async fn message_sync_calls(&self) -> Vec<String> {
            self.message_sync_calls.lock().await.clone()
        }
    }

    impl SessionSyncRunner for BlockingSessionSyncRunner {
        fn request_message_sync(
            &self,
            conversation_id: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            let conversation_id = conversation_id.to_string();
            Box::pin(async move {
                let call_index = {
                    let mut calls = self.message_sync_calls.lock().await;
                    calls.push(conversation_id);
                    calls.len()
                };
                self.notify.notify_one();
                if call_index == 1 {
                    self.release_first.notified().await;
                }
                Ok(())
            })
        }

        fn request_message_sync_from_seq(
            &self,
            conversation_id: &str,
            last_seq: u64,
            _limit: i32,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            let call = format!("{conversation_id}:{last_seq}");
            Box::pin(async move {
                self.message_sync_calls.lock().await.push(call);
                self.notify.notify_one();
                Ok(())
            })
        }

        fn send_read_ack(
            &self,
            _conversation_id: &str,
            _read_seq: u64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn request_participants_sync(
            &self,
            _conversation_id: &str,
            _limit: i32,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Vec<crate::model::ConversationParticipant>>>
                    + Send
                    + '_,
            >,
        > {
            Box::pin(async { Ok(Vec::new()) })
        }
    }

    impl SessionSyncRunner for RecordingSessionSyncRunner {
        fn request_message_sync(
            &self,
            conversation_id: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            let conversation_id = conversation_id.to_string();
            Box::pin(async move {
                self.message_sync_calls.lock().await.push(conversation_id);
                self.notify.notify_one();
                Ok(())
            })
        }

        fn request_message_sync_from_seq(
            &self,
            conversation_id: &str,
            last_seq: u64,
            _limit: i32,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            let call = format!("{conversation_id}:{last_seq}");
            Box::pin(async move {
                self.message_sync_calls.lock().await.push(call);
                self.notify.notify_one();
                Ok(())
            })
        }

        fn send_read_ack(
            &self,
            _conversation_id: &str,
            _read_seq: u64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn request_participants_sync(
            &self,
            _conversation_id: &str,
            _limit: i32,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Vec<crate::model::ConversationParticipant>>>
                    + Send
                    + '_,
            >,
        > {
            Box::pin(async { Ok(Vec::new()) })
        }
    }

    struct MemoryPendingSendStore {
        data: Mutex<Vec<PendingSendVo>>,
    }

    impl MemoryPendingSendStore {
        fn new() -> Self {
            Self {
                data: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl PendingSendReader for MemoryPendingSendStore {
        async fn get(&self, client_msg_id: &str) -> Result<Option<PendingSendVo>> {
            let data = self.data.lock().await;
            Ok(data
                .iter()
                .find(|entry| entry.client_msg_id == client_msg_id)
                .cloned())
        }

        async fn list(&self) -> Result<Vec<PendingSendVo>> {
            Ok(self.data.lock().await.clone())
        }
    }

    #[async_trait]
    impl PendingSendWriter for MemoryPendingSendStore {
        async fn push(&self, entry: PendingSendVo) -> Result<()> {
            self.data.lock().await.push(entry);
            Ok(())
        }

        async fn pop(&self, client_msg_id: &str) -> Result<Option<PendingSendVo>> {
            let mut data = self.data.lock().await;
            let pos = data
                .iter()
                .position(|entry| entry.client_msg_id == client_msg_id);
            Ok(pos.map(|index| data.remove(index)))
        }
    }

    struct MemoryMessageStore {
        data: RwLock<HashMap<String, IMMessage>>,
        fail_saves: bool,
    }

    impl MemoryMessageStore {
        fn new() -> Self {
            Self {
                data: RwLock::new(HashMap::new()),
                fail_saves: false,
            }
        }

        /// 模拟持久化失败的消息存储：用于验证「先投递后持久化」在 save_batch 失败时仍投递视图。
        fn failing() -> Self {
            Self {
                data: RwLock::new(HashMap::new()),
                fail_saves: true,
            }
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
                .find(|message| message.client_msg_id == client_msg_id)
                .cloned())
        }

        async fn get_by_conversation(
            &self,
            _conversation_id: &str,
            _before_seq: u64,
            _limit: u32,
        ) -> Result<Vec<IMMessage>> {
            Ok(Vec::new())
        }

        async fn search(&self, _keyword: &str, _limit: u32) -> Result<Vec<IMMessage>> {
            Ok(Vec::new())
        }

        async fn search_in_conversation(
            &self,
            _conversation_id: &str,
            _keyword: &str,
            _limit: u32,
        ) -> Result<Vec<IMMessage>> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl MessageWriter for MemoryMessageStore {
        async fn save_batch(&self, messages: &[IMMessage]) -> Result<()> {
            if self.fail_saves {
                return Err(crate::shared::error::FlareError::general_error(
                    "simulated persist failure",
                ));
            }
            let mut data = self.data.write().await;
            for message in messages {
                let key = if !message.server_id.is_empty() {
                    message.server_id.clone()
                } else {
                    message.client_msg_id.clone()
                };
                data.insert(key, message.clone());
            }
            Ok(())
        }

        async fn save_one(&self, message: &IMMessage) -> Result<()> {
            self.save_batch(std::slice::from_ref(message)).await
        }

        async fn update_status(&self, message_id: &str, status: i32) -> Result<()> {
            if let Some(message) = self.data.write().await.get_mut(message_id) {
                message.status = status;
            }
            Ok(())
        }

        async fn update_content(&self, _message_id: &str, _new_content: Vec<u8>) -> Result<bool> {
            Ok(false)
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
            data.remove(client_msg_id);
            data.insert(message.server_id.clone(), message.clone());
            Ok(())
        }
    }

    impl MessageStore for MemoryMessageStore {}

    struct NoopConversationStore;
    struct NoopSyncCursorStore;
    struct CursorAtSeqStore {
        last_seq: u64,
    }

    #[async_trait]
    impl ConversationReader for NoopConversationStore {
        async fn get(&self, _conversation_id: &str) -> Result<Option<crate::model::Conversation>> {
            Ok(None)
        }

        async fn list(&self) -> Result<Vec<crate::model::Conversation>> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl ConversationWriter for NoopConversationStore {
        async fn save_batch(&self, _conversations: &[crate::model::Conversation]) -> Result<()> {
            Ok(())
        }

        async fn save_one(&self, _conversation: &crate::model::Conversation) -> Result<()> {
            Ok(())
        }

        async fn update_unread(
            &self,
            _conversation_id: &str,
            _unread_count: u32,
            _last_read_seq: u64,
        ) -> Result<()> {
            Ok(())
        }

        async fn set_pinned(&self, _conversation_id: &str, _pinned: bool) -> Result<()> {
            Ok(())
        }

        async fn set_muted(&self, _conversation_id: &str, _muted: bool) -> Result<()> {
            Ok(())
        }

        async fn set_archived(&self, _conversation_id: &str, _archived: bool) -> Result<()> {
            Ok(())
        }

        async fn mark_unread(&self, _conversation_id: &str) -> Result<u32> {
            Ok(1)
        }

        async fn update_draft(&self, _conversation_id: &str, _draft: Option<&str>) -> Result<()> {
            Ok(())
        }

        async fn delete(&self, _conversation_id: &str) -> Result<()> {
            Ok(())
        }

        async fn merge_conversation_identity(
            &self,
            _from_conversation_id: &str,
            _to_conversation_id: &str,
        ) -> Result<()> {
            Ok(())
        }

        async fn clear_local_chat_history(
            &self,
            _conversation_id: &str,
            _cleared_through_seq: u64,
        ) -> Result<()> {
            Ok(())
        }

        async fn update_last_message(
            &self,
            _conversation_id: &str,
            _last_message_id: &str,
            _last_sender_id: &str,
            _last_message_at: u64,
            _last_message_preview: Option<&str>,
            _max_seq: u64,
        ) -> Result<()> {
            Ok(())
        }

        async fn recompute_unread_for_user(
            &self,
            _conversation_id: &str,
            _current_user_id: &str,
        ) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl SyncCursorReader for NoopSyncCursorStore {
        async fn get_conversation_cursor(
            &self,
            _user_id: &str,
            _conversation_id: &str,
        ) -> Result<Option<SyncCursorVo>> {
            Ok(None)
        }

        async fn get_raw(&self, _key: &str) -> Result<Option<String>> {
            Ok(None)
        }
    }

    #[async_trait]
    impl SyncCursorWriter for NoopSyncCursorStore {
        async fn save_conversation_cursor(&self, _cursor: &SyncCursorVo) -> Result<()> {
            Ok(())
        }

        async fn save_raw(&self, _key: &str, _cursor: &str) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl SyncCursorReader for CursorAtSeqStore {
        async fn get_conversation_cursor(
            &self,
            user_id: &str,
            conversation_id: &str,
        ) -> Result<Option<SyncCursorVo>> {
            Ok(Some(SyncCursorVo {
                user_id: user_id.to_string(),
                conversation_id: conversation_id.to_string(),
                last_seq: self.last_seq,
                synced_at: 0,
            }))
        }

        async fn get_raw(&self, _key: &str) -> Result<Option<String>> {
            Ok(None)
        }
    }

    #[async_trait]
    impl SyncCursorWriter for CursorAtSeqStore {
        async fn save_conversation_cursor(&self, _cursor: &SyncCursorVo) -> Result<()> {
            Ok(())
        }

        async fn save_raw(&self, _key: &str, _cursor: &str) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn send_ack_is_published_once_when_reliable_queue_enabled() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let pending_store = Arc::new(MemoryPendingSendStore::new());
        let message_store = Arc::new(MemoryMessageStore::new());
        let sender = Arc::new(PacketSender::new(
            Arc::new(Mutex::new(None)),
            Arc::new(ProtobufCodec) as Arc<dyn Codec>,
        ));
        let reliable_queue = Arc::new(ReliableSendQueue::new(ReliableSendQueueConfig {
            pending_reader: pending_store.clone(),
            pending_writer: pending_store,
            sender,
            message_store,
            conversation_store: Arc::new(NoopConversationStore),
            current_user_id: current_user_id.clone(),
            bus: bus.clone(),
            timeout_secs: Some(60),
            max_retries: Some(3),
            max_in_flight: Some(32),
            metrics: MetricsRecorder::disabled(),
        }));

        let dispatcher = Dispatcher::new(
            bus.clone(),
            Some(reliable_queue.clone()),
            None,
            None,
            None,
            current_user_id,
            EventDeduper::new(Some(64)),
            test_notification_pipeline(bus.clone()),
            MetricsRecorder::disabled(),
        );

        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.client_msg_id = "client-1".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();
        reliable_queue.enqueue(message).await.unwrap();

        let optimistic = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected optimistic message event")
            .expect("bus closed");
        assert!(matches!(
            optimistic,
            SdkEvent::Message(MessageEvent::ReceivedBatch { .. })
        ));

        tokio::time::sleep(Duration::from_millis(20)).await;

        dispatcher
            .dispatch(DownlinkPayload::SendAck(accepted_ack(
                "client-1", "conv-1", "server-1", 1,
            )))
            .await
            .unwrap();

        let first = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected one send ack event")
            .expect("bus closed");

        match first {
            SdkEvent::Message(MessageEvent::SendAck { ack }) => {
                assert_eq!(ack.client_msg_id, "client-1");
                assert_eq!(accepted_server_msg_id(&ack), Some("server-1"));
            }
            other => panic!("unexpected event: {other:?}"),
        }

        let second = timeout(Duration::from_millis(80), receiver.recv()).await;
        assert!(second.is_err(), "send ack should not be published twice");
    }

    #[tokio::test]
    async fn self_sent_realtime_message_converges_to_send_ack_without_received() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let pending_store = Arc::new(MemoryPendingSendStore::new());
        let message_store = Arc::new(MemoryMessageStore::new());
        let stores = StoreProvider {
            messages: message_store.clone(),
            conversations: Arc::new(NoopConversationStore),
            conversation_participants: None,
            cursors: Arc::new(NoopSyncCursorStore),
            pending_send_reader: None,
            pending_send_writer: None,
            upload_manifest_store: None,
            media_cache_store: None,
            media_cache_admin: None,
            user_file_download_store: None,
            user_profiles_reader: None,
            user_profiles_writer: None,
        };
        let sender = Arc::new(PacketSender::new(
            Arc::new(Mutex::new(None)),
            Arc::new(ProtobufCodec) as Arc<dyn Codec>,
        ));
        let reliable_queue = Arc::new(ReliableSendQueue::new(ReliableSendQueueConfig {
            pending_reader: pending_store.clone(),
            pending_writer: pending_store,
            sender,
            message_store: message_store.clone(),
            conversation_store: Arc::new(NoopConversationStore),
            current_user_id: current_user_id.clone(),
            bus: bus.clone(),
            timeout_secs: Some(60),
            max_retries: Some(3),
            max_in_flight: Some(32),
            metrics: MetricsRecorder::disabled(),
        }));

        let dispatcher = Dispatcher::new(
            bus.clone(),
            Some(reliable_queue.clone()),
            None,
            None,
            Some(stores),
            current_user_id,
            EventDeduper::new(Some(64)),
            test_notification_pipeline(bus.clone()),
            MetricsRecorder::disabled(),
        );

        let mut optimistic = IMMessage::new(flare_proto::common::Message::default());
        optimistic.client_msg_id = "client-self-1".to_string();
        optimistic.conversation_id = "conv-1".to_string();
        optimistic.sender_id = "u1".to_string();
        reliable_queue.enqueue(optimistic).await.unwrap();

        // enqueue surfaces the optimistic message via ReceivedBatch (asserted in
        // reliable_queue tests); drain it so the assertions below target the
        // self-echo convergence, not the optimistic insert.
        let enqueue_optimistic = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected optimistic message event")
            .expect("bus closed");
        assert!(matches!(
            enqueue_optimistic,
            SdkEvent::Message(MessageEvent::ReceivedBatch { .. })
        ));

        tokio::time::sleep(Duration::from_millis(20)).await;

        let proto_message = flare_proto::common::Message {
            server_id: "server-self-1".to_string(),
            client_msg_id: "client-self-1".to_string(),
            conversation_id: "conv-1".to_string(),
            sender_id: "u1".to_string(),
            conversation_seq: 11,
            ..Default::default()
        };

        dispatcher
            .dispatch(DownlinkPayload::MessagePush(
                flare_proto::common::MessagePush {
                    messages: vec![proto_message],
                    notifications: Vec::new(),
                },
            ))
            .await
            .unwrap();

        let first = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected convergence event")
            .expect("bus closed");
        match first {
            SdkEvent::Message(MessageEvent::SendAck { ack }) => {
                assert_eq!(ack.client_msg_id, "client-self-1");
                assert_eq!(accepted_server_msg_id(&ack), Some("server-self-1"));
            }
            other => panic!("unexpected event: {other:?}"),
        }

        let second = timeout(Duration::from_millis(80), receiver.recv()).await;
        assert!(
            second.is_err(),
            "self-sent convergence should suppress Received callback"
        );
    }

    #[tokio::test]
    async fn out_of_order_ack_is_buffered_and_applied_once() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let pending_store = Arc::new(MemoryPendingSendStore::new());
        let message_store = Arc::new(MemoryMessageStore::new());
        let sender = Arc::new(PacketSender::new(
            Arc::new(Mutex::new(None)),
            Arc::new(ProtobufCodec) as Arc<dyn Codec>,
        ));
        let reliable_queue = Arc::new(ReliableSendQueue::new(ReliableSendQueueConfig {
            pending_reader: pending_store.clone(),
            pending_writer: pending_store,
            sender,
            message_store,
            conversation_store: Arc::new(NoopConversationStore),
            current_user_id: current_user_id.clone(),
            bus: bus.clone(),
            timeout_secs: Some(60),
            max_retries: Some(3),
            max_in_flight: Some(32),
            metrics: MetricsRecorder::disabled(),
        }));
        let dispatcher = Dispatcher::new(
            bus.clone(),
            Some(reliable_queue.clone()),
            None,
            None,
            None,
            current_user_id,
            EventDeduper::new(Some(64)),
            test_notification_pipeline(bus.clone()),
            MetricsRecorder::disabled(),
        );

        let mut message1 = IMMessage::new(flare_proto::common::Message::default());
        message1.client_msg_id = "client-1".to_string();
        message1.conversation_id = "conv-1".to_string();
        message1.sender_id = "u1".to_string();

        let mut message2 = IMMessage::new(flare_proto::common::Message::default());
        message2.client_msg_id = "client-2".to_string();
        message2.conversation_id = "conv-1".to_string();
        message2.sender_id = "u1".to_string();

        reliable_queue.enqueue(message1).await.unwrap();
        reliable_queue.enqueue(message2).await.unwrap();

        // Each enqueue surfaces its optimistic message via ReceivedBatch; drain
        // both so the assertions below target the buffered out-of-order acks.
        for _ in 0..2 {
            let optimistic = timeout(Duration::from_millis(200), receiver.recv())
                .await
                .expect("expected optimistic message event")
                .expect("bus closed");
            assert!(matches!(
                optimistic,
                SdkEvent::Message(MessageEvent::ReceivedBatch { .. })
            ));
        }

        tokio::time::sleep(Duration::from_millis(20)).await;

        dispatcher
            .dispatch(DownlinkPayload::SendAck(accepted_ack(
                "client-2", "conv-1", "server-2", 2,
            )))
            .await
            .unwrap();

        dispatcher
            .dispatch(DownlinkPayload::SendAck(accepted_ack(
                "client-1", "conv-1", "server-1", 1,
            )))
            .await
            .unwrap();

        let first = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected first ack event")
            .expect("bus closed");
        let second = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected second ack event")
            .expect("bus closed");

        let mut ack_ids = Vec::new();
        for event in [first, second] {
            match event {
                SdkEvent::Message(MessageEvent::SendAck { ack }) => {
                    ack_ids.push(ack.client_msg_id);
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
        ack_ids.sort();
        assert_eq!(
            ack_ids,
            vec!["client-1".to_string(), "client-2".to_string()]
        );

        let third = timeout(Duration::from_millis(80), receiver.recv()).await;
        assert!(
            third.is_err(),
            "acks should not be published more than once"
        );
    }

    #[tokio::test]
    async fn realtime_and_sync_replay_share_event_deduper() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let deduper = EventDeduper::new(Some(64));
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let message_store = Arc::new(MemoryMessageStore::new());
        let stores = StoreProvider {
            messages: message_store,
            conversations: Arc::new(NoopConversationStore),
            conversation_participants: None,
            cursors: Arc::new(NoopSyncCursorStore),
            pending_send_reader: None,
            pending_send_writer: None,
            upload_manifest_store: None,
            media_cache_store: None,
            media_cache_admin: None,
            user_file_download_store: None,
            user_profiles_reader: None,
            user_profiles_writer: None,
        };
        let dispatcher = Dispatcher::new(
            bus.clone(),
            None,
            None,
            None,
            Some(stores.clone()),
            current_user_id,
            deduper.clone(),
            test_notification_pipeline(bus.clone()),
            MetricsRecorder::disabled(),
        );
        let sync_apply = SyncApplyUseCase::new(
            stores,
            bus.clone(),
            deduper,
            test_notification_pipeline(bus.clone()),
        );

        let mut event = flare_proto::common::Event::default();
        event.event_id = "evt-delete-1".to_string();
        event.conversation_id = "conv-1".to_string();
        event.payload = Some(flare_proto::common::event::Payload::Delete(
            MessageDeleteEvent {
                server_msg_id: "server-1".to_string(),
                scope: Some(2),
                ..Default::default()
            },
        ));

        dispatcher
            .dispatch(DownlinkPayload::Event(event.clone()))
            .await
            .unwrap();
        sync_apply.apply_critical_events("u1", &[event]).await;

        let mut deleted_count = 0usize;
        let start = tokio::time::Instant::now();
        while start.elapsed() < Duration::from_millis(200) {
            match timeout(Duration::from_millis(30), receiver.recv()).await {
                Ok(Ok(SdkEvent::Message(MessageEvent::Deleted { event, .. }))) => {
                    assert_eq!(event.server_msg_id, "server-1");
                    deleted_count += 1;
                }
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert_eq!(
            deleted_count, 1,
            "duplicate replay should not emit second Deleted event"
        );
    }

    #[tokio::test]
    async fn realtime_and_sync_message_replay_share_message_deduper() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let deduper = EventDeduper::new(Some(64));
        let message_deduper = MessageDeduper::new(Some(64));
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let stores = StoreProvider {
            messages: Arc::new(MemoryMessageStore::new()),
            conversations: Arc::new(NoopConversationStore),
            conversation_participants: None,
            cursors: Arc::new(NoopSyncCursorStore),
            pending_send_reader: None,
            pending_send_writer: None,
            upload_manifest_store: None,
            media_cache_store: None,
            media_cache_admin: None,
            user_file_download_store: None,
            user_profiles_reader: None,
            user_profiles_writer: None,
        };
        let notification_pipeline =
            test_notification_pipeline_with_deduper(bus.clone(), message_deduper.clone());
        let dispatcher = Dispatcher::new(
            bus.clone(),
            None,
            None,
            None,
            Some(stores.clone()),
            current_user_id,
            deduper.clone(),
            notification_pipeline.clone(),
            MetricsRecorder::disabled(),
        );
        let sync_apply = SyncApplyUseCase::new(stores, bus.clone(), deduper, notification_pipeline);

        let proto_message = flare_proto::common::Message {
            server_id: "server-msg-1".to_string(),
            client_msg_id: "client-msg-1".to_string(),
            conversation_id: "conv-1".to_string(),
            sender_id: "u2".to_string(),
            conversation_seq: 10,
            ..Default::default()
        };

        dispatcher
            .dispatch(DownlinkPayload::MessagePush(
                flare_proto::common::MessagePush {
                    messages: vec![proto_message.clone()],
                    notifications: Vec::new(),
                },
            ))
            .await
            .unwrap();

        sync_apply
            .apply_single_conversation_page(
                "conv-1",
                "u1",
                0,
                &flare_proto::common::SingleConversationSyncRes {
                    conversation_id: "conv-1".to_string(),
                    items: vec![flare_proto::common::SyncSliceItem {
                        conversation_seq: 10,
                        created_at: 0,
                        payload: Some(flare_proto::common::sync_slice_item::Payload::Message(
                            proto_message.clone(),
                        )),
                    }],
                    max_conversation_seq: 10,
                    next_cursor: String::new(),
                    has_more: false,
                    hints: None,
                    stale: None,
                },
            )
            .await
            .unwrap();

        let mut received_count = 0usize;
        let start = tokio::time::Instant::now();
        while start.elapsed() < Duration::from_millis(200) {
            match timeout(Duration::from_millis(30), receiver.recv()).await {
                Ok(Ok(SdkEvent::Message(MessageEvent::Received { message }))) => {
                    assert_eq!(message.server_id, "server-msg-1");
                    received_count += 1;
                }
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert_eq!(
            received_count, 1,
            "duplicate replay should not emit second Received event"
        );
    }

    #[tokio::test]
    async fn message_push_delivers_to_view_even_when_persist_fails() {
        // A2/BX-02：先投递后持久化。save_batch 失败时消息仍应投递到视图（bus），
        // 由既有 seq-gap 补拉负责重取持久化，而不是从本批次丢弃。
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let deduper = EventDeduper::new(Some(64));
        let message_deduper = MessageDeduper::new(Some(64));
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let stores = StoreProvider {
            messages: Arc::new(MemoryMessageStore::failing()),
            conversations: Arc::new(NoopConversationStore),
            conversation_participants: None,
            cursors: Arc::new(NoopSyncCursorStore),
            pending_send_reader: None,
            pending_send_writer: None,
            upload_manifest_store: None,
            media_cache_store: None,
            media_cache_admin: None,
            user_file_download_store: None,
            user_profiles_reader: None,
            user_profiles_writer: None,
        };
        let notification_pipeline =
            test_notification_pipeline_with_deduper(bus.clone(), message_deduper.clone());
        let dispatcher = Dispatcher::new(
            bus.clone(),
            None,
            None,
            None,
            Some(stores),
            current_user_id,
            deduper,
            notification_pipeline,
            MetricsRecorder::disabled(),
        );

        dispatcher
            .dispatch(DownlinkPayload::MessagePush(
                flare_proto::common::MessagePush {
                    messages: vec![flare_proto::common::Message {
                        server_id: "server-fail-1".to_string(),
                        client_msg_id: "client-fail-1".to_string(),
                        conversation_id: "conv-1".to_string(),
                        sender_id: "u2".to_string(),
                        conversation_seq: 7,
                        ..Default::default()
                    }],
                    notifications: Vec::new(),
                },
            ))
            .await
            .unwrap();

        let mut received = false;
        let start = tokio::time::Instant::now();
        while start.elapsed() < Duration::from_millis(200) {
            match timeout(Duration::from_millis(30), receiver.recv()).await {
                Ok(Ok(SdkEvent::Message(MessageEvent::Received { message }))) => {
                    assert_eq!(message.server_id, "server-fail-1");
                    received = true;
                    break;
                }
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert!(
            received,
            "save_batch 失败时消息仍须投递到视图（deliver-then-persist），不得从本批次丢弃"
        );
    }

    #[tokio::test]
    async fn concurrent_message_push_dispatch_is_lossless_for_slow_subscriber() {
        const TASKS: usize = 8;
        const PER_TASK: usize = 250;
        const TOTAL: usize = TASKS * PER_TASK;

        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let message_store = Arc::new(MemoryMessageStore::new());
        let stores = StoreProvider {
            messages: message_store.clone(),
            conversations: Arc::new(NoopConversationStore),
            conversation_participants: None,
            cursors: Arc::new(NoopSyncCursorStore),
            pending_send_reader: None,
            pending_send_writer: None,
            upload_manifest_store: None,
            media_cache_store: None,
            media_cache_admin: None,
            user_file_download_store: None,
            user_profiles_reader: None,
            user_profiles_writer: None,
        };
        let dispatcher = Arc::new(Dispatcher::new(
            bus.clone(),
            None,
            None,
            None,
            Some(stores.clone()),
            current_user_id,
            EventDeduper::new(Some(TOTAL * 2)),
            test_notification_pipeline(bus.clone()),
            MetricsRecorder::disabled(),
        ));

        let mut tasks = Vec::with_capacity(TASKS);
        for task_id in 0..TASKS {
            let dispatcher = dispatcher.clone();
            tasks.push(tokio::spawn(async move {
                let mut messages = Vec::with_capacity(PER_TASK);
                for index in 0..PER_TASK {
                    let seq = (task_id * PER_TASK + index + 1) as u64;
                    messages.push(flare_proto::common::Message {
                        server_id: format!("server-{seq}"),
                        client_msg_id: format!("client-{seq}"),
                        conversation_id: format!("conv-{}", task_id % 4),
                        sender_id: "u2".to_string(),
                        conversation_seq: seq,
                        message_type: flare_proto::common::MessageType::Text as i32,
                        ..Default::default()
                    });
                }
                dispatcher
                    .dispatch(DownlinkPayload::MessagePush(
                        flare_proto::common::MessagePush {
                            messages,
                            notifications: Vec::new(),
                        },
                    ))
                    .await
                    .expect("message push dispatch");
            }));
        }

        for task in tasks {
            task.await.expect("dispatch task panicked");
        }

        let mut received_server_ids = std::collections::HashSet::with_capacity(TOTAL);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while received_server_ids.len() < TOTAL {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            assert!(
                !remaining.is_zero(),
                "timed out after receiving {} of {TOTAL} messages",
                received_server_ids.len()
            );
            match timeout(remaining.min(Duration::from_millis(100)), receiver.recv()).await {
                Ok(Ok(SdkEvent::Message(MessageEvent::Received { message }))) => {
                    assert!(
                        received_server_ids.insert(message.server_id.clone()),
                        "duplicate received message {}",
                        message.server_id
                    );
                }
                Ok(Ok(_)) => {}
                Ok(Err(error)) => panic!("event receiver closed: {error:?}"),
                Err(_) => {}
            }
        }

        assert_eq!(
            message_store.data.read().await.len(),
            TOTAL,
            "all dispatched messages should be persisted exactly once"
        );
    }

    #[tokio::test]
    async fn persist_worker_persists_asynchronously_when_started() {
        // A2 win#2：装载 worker 后，持久化由后台串行 worker 完成（最终一致），dispatch 不再内联 await 落盘。
        let bus = EventBus::new();
        let deduper = EventDeduper::new(Some(64));
        let message_deduper = MessageDeduper::new(Some(64));
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let message_store = Arc::new(MemoryMessageStore::new());
        let stores = StoreProvider {
            messages: message_store.clone(),
            conversations: Arc::new(NoopConversationStore),
            conversation_participants: None,
            cursors: Arc::new(NoopSyncCursorStore),
            pending_send_reader: None,
            pending_send_writer: None,
            upload_manifest_store: None,
            media_cache_store: None,
            media_cache_admin: None,
            user_file_download_store: None,
            user_profiles_reader: None,
            user_profiles_writer: None,
        };
        let notification_pipeline =
            test_notification_pipeline_with_deduper(bus.clone(), message_deduper.clone());
        let dispatcher = Arc::new(Dispatcher::new(
            bus.clone(),
            None,
            None,
            None,
            Some(stores),
            current_user_id,
            deduper,
            notification_pipeline,
            MetricsRecorder::disabled(),
        ));
        dispatcher.start_persist_worker();

        dispatcher
            .dispatch(DownlinkPayload::MessagePush(
                flare_proto::common::MessagePush {
                    messages: vec![flare_proto::common::Message {
                        server_id: "server-worker-1".to_string(),
                        client_msg_id: "client-worker-1".to_string(),
                        conversation_id: "conv-1".to_string(),
                        sender_id: "u2".to_string(),
                        conversation_seq: 5,
                        ..Default::default()
                    }],
                    notifications: Vec::new(),
                },
            ))
            .await
            .unwrap();

        // worker 异步落盘 → 最终一致地轮询存储
        let mut persisted = false;
        let start = tokio::time::Instant::now();
        while start.elapsed() < Duration::from_millis(500) {
            if message_store
                .data
                .read()
                .await
                .contains_key("server-worker-1")
            {
                persisted = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            persisted,
            "启动 worker 后，消息应由后台串行 worker 异步持久化"
        );
    }

    /// A2 win#2 压测:启动 worker 后并发投递(超过 channel 容量 → 触发背压),
    /// 断言**零丢失**(全部经后台串行 worker 落盘)、无死锁。
    #[tokio::test]
    async fn persist_worker_under_concurrent_dispatch_loses_nothing() {
        const TASKS: usize = 8;
        const PER_TASK: usize = 40; // 8×40=320 > 持久化 channel 容量 256 → 必经背压
        let total = TASKS * PER_TASK;

        let bus = EventBus::new();
        let deduper = EventDeduper::new(Some(2048));
        let message_deduper = MessageDeduper::new(Some(2048));
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let message_store = Arc::new(MemoryMessageStore::new());
        let stores = StoreProvider {
            messages: message_store.clone(),
            conversations: Arc::new(NoopConversationStore),
            conversation_participants: None,
            cursors: Arc::new(NoopSyncCursorStore),
            pending_send_reader: None,
            pending_send_writer: None,
            upload_manifest_store: None,
            media_cache_store: None,
            media_cache_admin: None,
            user_file_download_store: None,
            user_profiles_reader: None,
            user_profiles_writer: None,
        };
        let notification_pipeline =
            test_notification_pipeline_with_deduper(bus.clone(), message_deduper.clone());
        let dispatcher = Arc::new(Dispatcher::new(
            bus.clone(),
            None,
            None,
            None,
            Some(stores),
            current_user_id,
            deduper,
            notification_pipeline,
            MetricsRecorder::disabled(),
        ));
        dispatcher.start_persist_worker();

        let mut handles = Vec::with_capacity(TASKS);
        for t in 0..TASKS {
            let d = dispatcher.clone();
            handles.push(tokio::spawn(async move {
                for i in 0..PER_TASK {
                    d.dispatch(DownlinkPayload::MessagePush(
                        flare_proto::common::MessagePush {
                            messages: vec![flare_proto::common::Message {
                                server_id: format!("srv-{t}-{i}"),
                                client_msg_id: format!("cli-{t}-{i}"),
                                conversation_id: format!("conv-{t}"),
                                sender_id: "u2".to_string(),
                                conversation_seq: (i as u64) + 1,
                                ..Default::default()
                            }],
                            notifications: Vec::new(),
                        },
                    ))
                    .await
                    .unwrap();
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        // 持久化经后台 worker 异步完成 → 最终一致轮询:全部落盘、零丢失。
        let start = tokio::time::Instant::now();
        loop {
            let n = message_store.data.read().await.len();
            if n >= total {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "persist worker lost messages under concurrency: {n}/{total}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            message_store.data.read().await.len(),
            total,
            "all concurrently dispatched messages must be persisted exactly once via the worker"
        );
    }

    #[test]
    fn seq_helpers_ignore_duplicates_and_find_contiguous_tail() {
        assert_eq!(max_contiguous_seq(10, &[12, 11, 11, 13]), 13);
        assert_eq!(max_contiguous_seq(10, &[12, 13]), 10);
        assert_eq!(first_gap_after(10, &[11, 13, 14]), Some(11));
        assert_eq!(first_gap_after(10, &[11, 12, 13]), None);
    }

    // ---- TEST-1: 生成式属性测试(seq-gap 辅助函数 = 游标推进/补拉正确性的支点)----
    // 用确定性 LCG 造大量乱序+重复+空洞的输入,断言结构性不变量(独立于实现,非镜像)。

    struct Lcg(u64);
    impl Lcg {
        fn next_u64(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0
        }
        fn below(&mut self, n: u64) -> u64 {
            if n == 0 { 0 } else { self.next_u64() % n }
        }
    }

    /// 小值域 → 强制出现空洞/重复/0;含 0 以覆盖「0 非真实 seq」的过滤。
    fn gen_seqs(rng: &mut Lcg) -> Vec<u64> {
        let len = rng.below(12) as usize;
        let span = 1 + rng.below(10);
        (0..len).map(|_| rng.below(span + 1)).collect()
    }

    #[test]
    fn prop_max_contiguous_seq_is_the_maximal_present_run_from_known() {
        let mut rng = Lcg(0x1234_5678_9abc_def0);
        for _ in 0..4000 {
            let seqs = gen_seqs(&mut rng);
            let known = rng.below(8);
            let set: std::collections::BTreeSet<u64> = seqs.iter().copied().collect();

            let m = max_contiguous_seq(known, &seqs);
            assert!(
                m >= known,
                "result below known: m={m} known={known} seqs={seqs:?}"
            );
            // (known, m] 全部在集合内(运行确实连续且都存在)。
            for i in (known + 1)..=m {
                assert!(
                    set.contains(&i),
                    "hole inside run at {i}: m={m} known={known} seqs={seqs:?}"
                );
            }
            // 运行停在第一个缺口:m+1 不在集合。
            assert!(
                !set.contains(&(m + 1)),
                "run must stop at first hole but m+1 present: m={m} known={known} seqs={seqs:?}"
            );
        }
    }

    #[test]
    fn prop_first_gap_after_is_contiguous_tail_iff_data_beyond_hole() {
        let mut rng = Lcg(0xdead_beef_cafe_babe);
        for _ in 0..4000 {
            let seqs = gen_seqs(&mut rng);
            let known = rng.below(8);
            let set: std::collections::BTreeSet<u64> = seqs.iter().copied().collect();
            let m = max_contiguous_seq(known, &seqs);
            let has_later = set.iter().any(|s| *s > m + 1);

            match first_gap_after(known, &seqs) {
                Some(g) => {
                    assert_eq!(
                        g, m,
                        "gap-after must equal the contiguous tail; seqs={seqs:?}"
                    );
                    assert!(
                        has_later,
                        "Some(gap) requires data beyond the hole; seqs={seqs:?}"
                    );
                }
                None => assert!(
                    !has_later,
                    "None requires no data beyond the hole; seqs={seqs:?}"
                ),
            }
        }
    }

    #[test]
    fn prop_first_internal_gap_from_matches_independent_reference() {
        let mut rng = Lcg(0x0f0f_0f0f_1357_9bdf);
        for _ in 0..4000 {
            let seqs = gen_seqs(&mut rng);
            let base = rng.below(8);
            // 实现内部对 >= base 后还会再滤掉 0(0 非真实 seq),参考须对齐:>= base 且 > 0。
            let mut t: Vec<u64> = seqs
                .iter()
                .copied()
                .filter(|s| *s >= base && *s > 0)
                .collect();
            t.sort_unstable();
            t.dedup();
            let expected = t.windows(2).find(|w| w[1] > w[0] + 1).map(|w| w[0]);
            assert_eq!(
                first_internal_gap_after_from(base, &seqs),
                expected,
                "base={base} seqs={seqs:?} t={t:?}"
            );
        }
    }

    #[test]
    fn prop_seq_repair_backoff_is_monotonic_bounded_and_formula() {
        use super::{SEQ_REPAIR_BASE_BACKOFF_MS, SEQ_REPAIR_MAX_BACKOFF_MS};
        let mut prev = 0u64;
        for attempt in 0..40u32 {
            let b = seq_repair_backoff_ms(attempt);
            assert!(
                b >= SEQ_REPAIR_BASE_BACKOFF_MS,
                "below base at {attempt}: {b}"
            );
            assert!(
                b <= SEQ_REPAIR_MAX_BACKOFF_MS,
                "above cap at {attempt}: {b}"
            );
            assert!(
                b >= prev,
                "must be non-decreasing: at {attempt} {b} < {prev}"
            );
            prev = b;
        }
        assert_eq!(seq_repair_backoff_ms(0), SEQ_REPAIR_BASE_BACKOFF_MS);
        assert_eq!(seq_repair_backoff_ms(1), SEQ_REPAIR_BASE_BACKOFF_MS);
        assert_eq!(seq_repair_backoff_ms(2), SEQ_REPAIR_BASE_BACKOFF_MS * 2);
        assert_eq!(seq_repair_backoff_ms(100), SEQ_REPAIR_MAX_BACKOFF_MS);
    }

    #[test]
    fn waterline_ping_matches_control_type_or_kind_attribute() {
        assert!(is_waterline_ping("sync.waterline", &HashMap::new()));
        assert!(is_waterline_ping(
            "custom",
            &HashMap::from([(
                WATERLINE_ATTR_KIND.to_string(),
                "conversation.waterline".to_string()
            )])
        ));
        assert!(!is_waterline_ping("typing", &HashMap::new()));
    }

    #[test]
    fn waterline_attributes_parse_conversation_and_seq() {
        let attrs = HashMap::from([
            (
                WATERLINE_ATTR_CONVERSATION_ID_CAMEL.to_string(),
                " c1 ".to_string(),
            ),
            (WATERLINE_ATTR_MAX_SEQ_CAMEL.to_string(), "42".to_string()),
        ]);
        assert_eq!(
            attr_non_empty(&attrs, WATERLINE_ATTR_CONVERSATION_ID_CAMEL),
            Some("c1")
        );
        assert_eq!(attr_u64(&attrs, WATERLINE_ATTR_MAX_SEQ_CAMEL), Some(42));
    }

    #[tokio::test]
    async fn empty_event_envelope_waterline_triggers_message_sync() {
        let bus = EventBus::new();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let sync = Arc::new(RecordingSessionSyncRunner::new());
        let dispatcher = Dispatcher::new(
            bus.clone(),
            None,
            None,
            Some(sync.clone()),
            None,
            current_user_id,
            EventDeduper::new(Some(64)),
            test_notification_pipeline(bus),
            MetricsRecorder::disabled(),
        );

        dispatcher
            .dispatch(DownlinkPayload::EventEnvelope(EventEnvelope {
                conversation_id: "conversation-1".to_string(),
                max_conversation_seq: 2,
                ..Default::default()
            }))
            .await
            .unwrap();

        timeout(Duration::from_millis(200), sync.notify.notified())
            .await
            .expect("event envelope waterline should trigger message sync");
        assert_eq!(
            sync.message_sync_calls().await,
            vec!["conversation-1".to_string()]
        );
    }

    #[tokio::test]
    async fn standalone_event_waterline_triggers_message_sync() {
        let bus = EventBus::new();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let sync = Arc::new(RecordingSessionSyncRunner::new());
        let dispatcher = Dispatcher::new(
            bus.clone(),
            None,
            None,
            Some(sync.clone()),
            None,
            current_user_id,
            EventDeduper::new(Some(64)),
            test_notification_pipeline(bus),
            MetricsRecorder::disabled(),
        );

        dispatcher
            .dispatch(DownlinkPayload::Event(Event {
                conversation_id: "conversation-1".to_string(),
                conversation_seq: 101,
                ..Default::default()
            }))
            .await
            .unwrap();

        timeout(Duration::from_millis(200), sync.notify.notified())
            .await
            .expect("standalone event waterline should trigger message sync");
        assert_eq!(
            sync.message_sync_calls().await,
            vec!["conversation-1".to_string()]
        );
    }

    #[tokio::test]
    async fn in_flight_waterline_ping_runs_follow_up_pull() {
        let bus = EventBus::new();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let sync = Arc::new(BlockingSessionSyncRunner::new());
        let dispatcher = Dispatcher::new(
            bus.clone(),
            None,
            None,
            Some(sync.clone()),
            None,
            current_user_id,
            EventDeduper::new(Some(64)),
            test_notification_pipeline(bus),
            MetricsRecorder::disabled(),
        );

        dispatcher
            .maybe_trigger_waterline_pull(
                "test",
                "conversation.waterline",
                Some("conversation-1"),
                &HashMap::new(),
            )
            .await;
        timeout(Duration::from_millis(200), sync.notify.notified())
            .await
            .expect("first waterline pull should start");

        dispatcher
            .maybe_trigger_waterline_pull(
                "test",
                "conversation.waterline",
                Some("conversation-1"),
                &HashMap::new(),
            )
            .await;
        assert_eq!(
            sync.message_sync_calls().await,
            vec!["conversation-1".to_string()]
        );

        sync.release_first.notify_one();
        timeout(Duration::from_millis(200), sync.notify.notified())
            .await
            .expect("coalesced waterline ping should run a follow-up pull");
        assert_eq!(
            sync.message_sync_calls().await,
            vec!["conversation-1".to_string(), "conversation-1".to_string()]
        );
    }

    #[tokio::test]
    async fn unread_conversation_update_without_seq_triggers_message_sync() {
        let bus = EventBus::new();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let sync = Arc::new(RecordingSessionSyncRunner::new());
        let dispatcher = Dispatcher::new(
            bus.clone(),
            None,
            None,
            Some(sync.clone()),
            None,
            current_user_id,
            EventDeduper::new(Some(64)),
            test_notification_pipeline(bus),
            MetricsRecorder::disabled(),
        );

        dispatcher
            .dispatch(DownlinkPayload::Event(Event {
                conversation_id: "conversation-1".to_string(),
                conversation_seq: 0,
                payload: Some(ProtoEventPayload::Conversation(ConversationUpdateEvent {
                    conversation_id: "conversation-1".to_string(),
                    unread_count: 2,
                    ..Default::default()
                })),
                ..Default::default()
            }))
            .await
            .unwrap();

        timeout(Duration::from_millis(200), sync.notify.notified())
            .await
            .expect("unread conversation update should trigger message sync");
        assert_eq!(
            sync.message_sync_calls().await,
            vec!["conversation-1".to_string()]
        );
    }

    #[tokio::test]
    async fn unread_conversation_update_uses_typed_payload_conversation_id() {
        let bus = EventBus::new();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let sync = Arc::new(RecordingSessionSyncRunner::new());
        let dispatcher = Dispatcher::new(
            bus.clone(),
            None,
            None,
            Some(sync.clone()),
            None,
            current_user_id,
            EventDeduper::new(Some(64)),
            test_notification_pipeline(bus),
            MetricsRecorder::disabled(),
        );

        dispatcher
            .dispatch(DownlinkPayload::Event(Event {
                conversation_id: String::new(),
                conversation_seq: 0,
                payload: Some(ProtoEventPayload::Conversation(ConversationUpdateEvent {
                    conversation_id: "conversation-1".to_string(),
                    unread_count: 2,
                    ..Default::default()
                })),
                ..Default::default()
            }))
            .await
            .unwrap();

        timeout(Duration::from_millis(200), sync.notify.notified())
            .await
            .expect("typed conversation update should trigger message sync");
        assert_eq!(
            sync.message_sync_calls().await,
            vec!["conversation-1".to_string()]
        );
    }

    #[tokio::test]
    async fn recall_event_without_outer_conversation_id_publishes_local_message_conversation_id() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let message_store = Arc::new(MemoryMessageStore::new());
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-1".to_string();
        message.conversation_id = "conversation-1".to_string();
        message_store.save_one(&message).await.unwrap();
        let stores = StoreProvider {
            messages: message_store,
            conversations: Arc::new(NoopConversationStore),
            conversation_participants: None,
            cursors: Arc::new(NoopSyncCursorStore),
            pending_send_reader: None,
            pending_send_writer: None,
            upload_manifest_store: None,
            media_cache_store: None,
            media_cache_admin: None,
            user_file_download_store: None,
            user_profiles_reader: None,
            user_profiles_writer: None,
        };
        let dispatcher = Dispatcher::new(
            bus.clone(),
            None,
            None,
            None,
            Some(stores),
            current_user_id,
            EventDeduper::new(Some(64)),
            test_notification_pipeline(bus),
            MetricsRecorder::disabled(),
        );

        dispatcher
            .dispatch(DownlinkPayload::Event(Event {
                event_id: "recall-1".to_string(),
                conversation_id: String::new(),
                payload: Some(ProtoEventPayload::Recall(MessageRecallEvent {
                    server_msg_id: "server-1".to_string(),
                    ..Default::default()
                })),
                ..Default::default()
            }))
            .await
            .unwrap();

        let event = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected recall event")
            .expect("bus closed");
        match event {
            SdkEvent::Message(MessageEvent::Recalled {
                conversation_id, ..
            }) => {
                assert_eq!(conversation_id, "conversation-1");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn waterline_uses_materialized_message_seq_not_cursor_seq() {
        let bus = EventBus::new();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let sync = Arc::new(RecordingSessionSyncRunner::new());
        let stores = StoreProvider {
            messages: Arc::new(MemoryMessageStore::new()),
            conversations: Arc::new(NoopConversationStore),
            conversation_participants: None,
            cursors: Arc::new(CursorAtSeqStore { last_seq: 2 }),
            pending_send_reader: None,
            pending_send_writer: None,
            upload_manifest_store: None,
            media_cache_store: None,
            media_cache_admin: None,
            user_file_download_store: None,
            user_profiles_reader: None,
            user_profiles_writer: None,
        };
        let dispatcher = Dispatcher::new(
            bus.clone(),
            None,
            None,
            Some(sync.clone()),
            Some(stores),
            current_user_id,
            EventDeduper::new(Some(64)),
            test_notification_pipeline(bus),
            MetricsRecorder::disabled(),
        );

        dispatcher
            .dispatch(DownlinkPayload::EventEnvelope(EventEnvelope {
                conversation_id: "conversation-1".to_string(),
                max_conversation_seq: 2,
                ..Default::default()
            }))
            .await
            .unwrap();

        timeout(Duration::from_millis(200), sync.notify.notified())
            .await
            .expect("waterline must pull when cursor reached but message rows are missing");
        assert_eq!(
            sync.message_sync_calls().await,
            vec!["conversation-1".to_string()]
        );
    }

    #[test]
    fn internal_gap_detection_uses_local_window() {
        assert_eq!(first_internal_gap_after(&[590, 591, 596]), Some(591));
        assert_eq!(first_internal_gap_after(&[590, 591, 592]), None);
        assert_eq!(first_internal_gap_after(&[0, 590, 590, 591]), None);
    }

    #[test]
    fn internal_gap_detection_ignores_historical_gaps_before_base() {
        assert_eq!(
            first_internal_gap_after_from(752, &[1, 2, 752, 766]),
            Some(752)
        );
        assert_eq!(
            first_internal_gap_after_from(771, &[1, 2, 766, 771, 777]),
            Some(771)
        );
        assert_eq!(first_internal_gap_after_from(752, &[1, 2, 752, 753]), None);
    }

    #[test]
    fn seq_repair_backoff_is_exponential_and_capped() {
        assert_eq!(seq_repair_backoff_ms(1), 1_000);
        assert_eq!(seq_repair_backoff_ms(2), 2_000);
        assert_eq!(seq_repair_backoff_ms(3), 4_000);
        assert_eq!(seq_repair_backoff_ms(30), SEQ_REPAIR_MAX_BACKOFF_MS);
    }

    #[test]
    fn seq_repair_state_prunes_expired_idle_entries() {
        let now = 20 * 60 * 1_000;
        let mut states = HashMap::from([
            (
                "expired".to_string(),
                SeqRepairState {
                    updated_at_ms: now - SEQ_REPAIR_IDLE_TTL_MS - 1,
                    next_attempt_at_ms: now - 1,
                    ..Default::default()
                },
            ),
            (
                "backoff".to_string(),
                SeqRepairState {
                    updated_at_ms: now - SEQ_REPAIR_IDLE_TTL_MS - 1,
                    next_attempt_at_ms: now + 1_000,
                    ..Default::default()
                },
            ),
            (
                "in-flight".to_string(),
                SeqRepairState {
                    in_flight: true,
                    updated_at_ms: now - SEQ_REPAIR_IDLE_TTL_MS - 1,
                    ..Default::default()
                },
            ),
        ]);

        prune_seq_repair_state(&mut states, now);

        assert!(!states.contains_key("expired"));
        assert!(states.contains_key("backoff"));
        assert!(states.contains_key("in-flight"));
    }

    #[test]
    fn seq_repair_state_trims_oldest_idle_entries_when_over_capacity() {
        let now = 1_000;
        let mut states = HashMap::with_capacity(SEQ_REPAIR_MAX_TRACKED_CONVERSATIONS + 2);
        for index in 0..(SEQ_REPAIR_MAX_TRACKED_CONVERSATIONS + 2) {
            states.insert(
                format!("conv-{index}"),
                SeqRepairState {
                    updated_at_ms: now + index as u64,
                    ..Default::default()
                },
            );
        }

        prune_seq_repair_state(&mut states, now);

        assert_eq!(states.len(), SEQ_REPAIR_MAX_TRACKED_CONVERSATIONS);
        assert!(!states.contains_key("conv-0"));
        assert!(!states.contains_key("conv-1"));
        assert!(states.contains_key("conv-2"));
    }
}
