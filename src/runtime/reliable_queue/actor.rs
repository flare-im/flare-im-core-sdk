//! 可靠队列 Actor — 单任务循环：enqueue / ack / timeout → 转移

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use flare_proto::common::SendAck;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

use crate::domain::{
    ConversationStore, DeliveryLocalSnapshot, InFlightReconcileDecision, MessageDeliveryService,
    MessageStore, PendingDispatchDecision, PendingSendReader, PendingSendVo, PendingSendWriter,
    REASON_ORPHAN_RECOVERED, RetryDecision,
};
use crate::infrastructure::protocol::PacketSender;
use crate::kernel::event::{EventBus, MessageEvent, SdkEvent};
use crate::kernel::{CurrentUserIdStore, ReliableSendQueuePort};
use crate::model::IMMessage;
use crate::model::message::{MessageLocalState, MessageStatus};
use crate::shared::error::{ErrorCode, FlareError, Result};
use crate::shared::util::id;
use crate::shared::util::spawn_background_task;
use crate::shared::util::time::{deadline_after, delay, is_deadline_elapsed};
use crate::shared::util::{
    RELIABLE_QUEUE_MAX_IN_FLIGHT, RELIABLE_QUEUE_MAX_RETRIES, RELIABLE_QUEUE_TIMEOUT_SECS,
};
use crate::spi::metrics::{MetricLabel, MetricsRecorder};

const ADAPTIVE_INITIAL_MAX_IN_FLIGHT: usize = 4;
const ADAPTIVE_MIN_IN_FLIGHT: usize = 1;

/// 队列命令（仅通过此与队列通信）
#[derive(Debug)]
enum QueueCommand {
    /// 入队发送（SDK 内部统一 IMMessage）
    Enqueue {
        message: Box<IMMessage>,
        resp: oneshot::Sender<Result<()>>,
    },
    /// 收到 SendAck（由外部收到下行后注入）
    AckReceived(Box<SendAck>),
    /// 异步发送尝试完成；结果回流到 actor,由 actor 统一处理重试/失败状态。
    SendAttemptFinished {
        entry: Box<PendingSendVo>,
        retries: u32,
        result: Result<()>,
    },
    /// 登录时清空队列（包含 in_flight + pending），并将本地消息收敛为 failed
    ResetPendingOnLogin {
        resp: oneshot::Sender<Result<Vec<String>>>,
    },
    /// 连接/冷启动后：恢复当前账号下的 pending 队列，并做自愈扫描
    RecoverPendingForCurrentUser {
        resp: oneshot::Sender<Result<Vec<String>>>,
    },
}

/// 可靠发送队列句柄（发命令 + 持有 actor 所需依赖）
pub struct ReliableSendQueue {
    tx: mpsc::Sender<QueueCommand>,
    worker: crate::shared::util::BackgroundTask,
}

pub struct ReliableSendQueueConfig {
    pub pending_reader: Arc<dyn PendingSendReader>,
    pub pending_writer: Arc<dyn PendingSendWriter>,
    pub sender: Arc<PacketSender>,
    pub message_store: Arc<dyn MessageStore>,
    pub conversation_store: Arc<dyn ConversationStore>,
    pub current_user_id: CurrentUserIdStore,
    pub bus: EventBus,
    pub timeout_secs: Option<u64>,
    pub max_retries: Option<u32>,
    pub max_in_flight: Option<usize>,
    pub metrics: MetricsRecorder,
}

struct QueueState {
    pending_reader: Arc<dyn PendingSendReader>,
    pending_writer: Arc<dyn PendingSendWriter>,
    command_tx: mpsc::Sender<QueueCommand>,
    sender: Arc<PacketSender>,
    message_store: Arc<dyn MessageStore>,
    conversation_store: Arc<dyn ConversationStore>,
    current_user_id: CurrentUserIdStore,
    bus: EventBus,
    timeout_duration: Duration,
    max_retries: u32,
    max_in_flight: usize,
    adaptive_window: AdaptiveInFlightWindow,
    /// 当前在途消息；ACK 按 client_msg_id 精确收敛。
    in_flight: HashMap<String, InFlightSend>,
    /// 已被服务端接收但尚未持久化确认的消息；释放发送窗口，但保留 pending 以便恢复。
    awaiting_durable: HashMap<String, AwaitingDurableAck>,
    /// client_msg_id -> 已重试次数
    retry_count: HashMap<String, u32>,
    /// 提前/乱序到达的 ACK，等待对应 pending 成为当前处理项后再收敛
    pending_acks: HashMap<String, SendAck>,
    metrics: MetricsRecorder,
}

struct InFlightSend {
    entry: PendingSendVo,
    deadline_ms: u64,
}

struct AwaitingDurableAck {
    entry: PendingSendVo,
    deadline_ms: u64,
}

#[derive(Debug, Clone)]
struct AdaptiveInFlightWindow {
    current: usize,
    max: usize,
    consecutive_successes: usize,
}

impl AdaptiveInFlightWindow {
    fn new(max: usize) -> Self {
        let max = max.max(ADAPTIVE_MIN_IN_FLIGHT);
        Self {
            current: max.clamp(ADAPTIVE_MIN_IN_FLIGHT, ADAPTIVE_INITIAL_MAX_IN_FLIGHT),
            max,
            consecutive_successes: 0,
        }
    }

    fn current(&self) -> usize {
        self.current
    }

    fn note_success(&mut self) -> bool {
        if self.current >= self.max {
            self.consecutive_successes = 0;
            return false;
        }
        self.consecutive_successes = self.consecutive_successes.saturating_add(1);
        if self.consecutive_successes < self.current {
            return false;
        }
        self.current = (self.current + 1).min(self.max);
        self.consecutive_successes = 0;
        true
    }

    fn note_congestion(&mut self) -> bool {
        self.consecutive_successes = 0;
        let next = (self.current / 2).max(ADAPTIVE_MIN_IN_FLIGHT);
        if next == self.current {
            return false;
        }
        self.current = next;
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AckApplyResult {
    Terminal,
    KeepInFlight,
    AwaitDurable,
}

#[cfg(test)]
mod adaptive_window_tests {
    use super::AdaptiveInFlightWindow;

    #[test]
    fn adaptive_window_grows_after_window_sized_success_batch() {
        let mut window = AdaptiveInFlightWindow::new(8);
        assert_eq!(window.current(), 4);

        for _ in 0..3 {
            assert!(!window.note_success());
        }
        assert!(window.note_success());
        assert_eq!(window.current(), 5);
    }

    #[test]
    fn adaptive_window_halves_on_congestion() {
        let mut window = AdaptiveInFlightWindow::new(16);
        for _ in 0..4 {
            window.note_success();
        }
        assert_eq!(window.current(), 5);

        assert!(window.note_congestion());
        assert_eq!(window.current(), 2);
        assert!(window.note_congestion());
        assert_eq!(window.current(), 1);
        assert!(!window.note_congestion());
    }
}

impl ReliableSendQueue {
    /// 构建队列并启动后台任务；收到 ack 需由调用方往 `on_ack` 注入。
    /// reader 与 writer 通常为同一实现体（如 SqlitePendingSendRepo）的两个 Arc。
    pub fn new(config: ReliableSendQueueConfig) -> Self {
        let ReliableSendQueueConfig {
            pending_reader,
            pending_writer,
            sender,
            message_store,
            conversation_store,
            current_user_id,
            bus,
            timeout_secs,
            max_retries,
            max_in_flight,
            metrics,
        } = config;
        let (tx, mut rx) = mpsc::channel::<QueueCommand>(256);
        let timeout_duration =
            Duration::from_secs(timeout_secs.unwrap_or(RELIABLE_QUEUE_TIMEOUT_SECS));
        let max_retries = max_retries.unwrap_or(RELIABLE_QUEUE_MAX_RETRIES);
        let max_in_flight = max_in_flight
            .unwrap_or(RELIABLE_QUEUE_MAX_IN_FLIGHT)
            .clamp(1, 1024);
        let adaptive_window = AdaptiveInFlightWindow::new(max_in_flight);

        let mut state = QueueState {
            pending_reader,
            pending_writer,
            command_tx: tx.clone(),
            sender,
            message_store,
            conversation_store,
            current_user_id,
            bus,
            timeout_duration,
            max_retries,
            max_in_flight,
            adaptive_window,
            in_flight: HashMap::new(),
            awaiting_durable: HashMap::new(),
            retry_count: HashMap::new(),
            pending_acks: HashMap::new(),
            metrics,
        };
        state
            .metrics
            .gauge("reliable_queue.max_in_flight", max_in_flight as i64);
        state.metrics.gauge(
            "reliable_queue.adaptive_in_flight",
            state.adaptive_window.current() as i64,
        );

        let worker = spawn_background_task(async move {
            loop {
                tokio::select! {
                    Some(cmd) = rx.recv() => {
                        if let Err(e) = handle_command(&mut state, cmd).await {
                            warn!(%e, "reliable queue command error");
                        }
                    }
                    _ = delay(Duration::from_secs(1)) => {
                        if let Err(e) = check_timeout(&mut state).await {
                            warn!(%e, "reliable queue timeout check error");
                        }
                    }
                    else => break,
                }
            }
        });

        Self { tx, worker }
    }

    /// 入队发送（持久化后由 worker 按序发送，SDK 内部仅 IMMessage）
    pub async fn enqueue(&self, message: IMMessage) -> Result<()> {
        let (resp, rx) = oneshot::channel();
        self.tx
            .send(QueueCommand::Enqueue {
                message: Box::new(message),
                resp,
            })
            .await
            .map_err(|_| {
                FlareError::localized(ErrorCode::InternalError, "reliable queue closed")
            })?;
        rx.await
            .map_err(|_| FlareError::localized(ErrorCode::InternalError, "reliable queue closed"))?
    }

    /// 将收到的 SendAck 注入队列（由 Dispatcher 或 Engine 在收到下行 ack 时调用）
    pub async fn on_ack(&self, ack: SendAck) -> Result<()> {
        self.tx
            .send(QueueCommand::AckReceived(Box::new(ack)))
            .await
            .map_err(|_| FlareError::localized(ErrorCode::InternalError, "reliable queue closed"))
    }

    /// 登录专用：原子清空 in_flight + pending，避免会话切换时脏队列阻塞新消息。
    pub async fn reset_pending_on_login(&self) -> Result<Vec<String>> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(QueueCommand::ResetPendingOnLogin { resp: tx })
            .await
            .map_err(|_| {
                FlareError::localized(ErrorCode::InternalError, "reliable queue closed")
            })?;
        rx.await.map_err(|_| {
            FlareError::localized(
                ErrorCode::InternalError,
                "reliable queue reset response dropped",
            )
        })?
    }

    /// 冷启动/显式 connect 后：恢复当前账号下的 pending 队列，并自愈孤儿 sending / 跨账号脏 pending。
    pub async fn recover_pending_for_current_user(&self) -> Result<Vec<String>> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(QueueCommand::RecoverPendingForCurrentUser { resp: tx })
            .await
            .map_err(|_| {
                FlareError::localized(ErrorCode::InternalError, "reliable queue closed")
            })?;
        rx.await.map_err(|_| {
            FlareError::localized(
                ErrorCode::InternalError,
                "reliable queue recover response dropped",
            )
        })?
    }

    pub(crate) fn shutdown(&self) {
        self.worker.abort();
    }
}

#[async_trait::async_trait]
impl ReliableSendQueuePort for ReliableSendQueue {
    async fn enqueue(&self, message: IMMessage) -> Result<()> {
        ReliableSendQueue::enqueue(self, message).await
    }

    async fn on_ack(&self, ack: SendAck) -> Result<()> {
        ReliableSendQueue::on_ack(self, ack).await
    }

    async fn reset_pending_on_login(&self) -> Result<Vec<String>> {
        ReliableSendQueue::reset_pending_on_login(self).await
    }

    async fn recover_pending_for_current_user(&self) -> Result<Vec<String>> {
        ReliableSendQueue::recover_pending_for_current_user(self).await
    }
}

impl Drop for ReliableSendQueue {
    fn drop(&mut self) {
        // Drop JoinHandle 会 detach 任务；显式 abort 才能在会话结束时停止旧队列重试。
        self.shutdown();
    }
}

async fn handle_command(st: &mut QueueState, cmd: QueueCommand) -> Result<()> {
    match cmd {
        QueueCommand::Enqueue { message: msg, resp } => {
            st.metrics.counter("reliable_queue.enqueue_total", 1);
            let msg = *msg;
            let enqueued_at_ms = id::now_millis();
            let mut optimistic = msg.clone();
            optimistic.server_id = msg.client_msg_id.clone();
            optimistic.local_state = MessageLocalState {
                sending: true,
                failed: false,
                is_local: true,
                uploading: false,
                upload_progress: 0,
                sort_ts: enqueued_at_ms,
            };
            let entry = PendingSendVo {
                client_msg_id: msg.client_msg_id.clone(),
                conversation_id: msg.conversation_id.clone(),
                message: msg,
                enqueued_at_ms,
            };
            let (message_store, pending_writer) =
                (st.message_store.clone(), st.pending_writer.clone());
            if let Err(e) = message_store
                .save_batch(std::slice::from_ref(&optimistic))
                .await
            {
                let _ = resp.send(Err(e));
                return Ok(());
            }
            if let Err(e) = pending_writer.push(entry).await {
                let _ = resp.send(Err(e));
                return Ok(());
            }
            if let Err(e) = update_conversation_last_message(st, &optimistic).await {
                warn!(%e, conversation_id = %optimistic.conversation_id, "update optimistic conversation projection failed");
            }
            st.bus
                .publish(SdkEvent::Message(MessageEvent::ReceivedBatch {
                    messages: vec![optimistic],
                }));
            let _ = resp.send(Ok(()));
            try_send_next(st).await?;
        }
        QueueCommand::AckReceived(ack) => {
            st.metrics.counter("reliable_queue.ack_received_total", 1);
            let ack = *ack;
            if let Some(in_flight) = st.in_flight.remove(&ack.client_msg_id) {
                match apply_ack_and_publish(st, &in_flight.entry, ack).await {
                    AckApplyResult::Terminal => {}
                    AckApplyResult::KeepInFlight => {
                        st.in_flight
                            .insert(in_flight.entry.client_msg_id.clone(), in_flight);
                    }
                    AckApplyResult::AwaitDurable => {
                        st.awaiting_durable.insert(
                            in_flight.entry.client_msg_id.clone(),
                            AwaitingDurableAck {
                                entry: in_flight.entry,
                                deadline_ms: deadline_after(st.timeout_duration),
                            },
                        );
                    }
                }
            } else if let Some(awaiting) = st.awaiting_durable.remove(&ack.client_msg_id) {
                match apply_ack_and_publish(st, &awaiting.entry, ack).await {
                    AckApplyResult::Terminal => {}
                    AckApplyResult::KeepInFlight | AckApplyResult::AwaitDurable => {
                        st.awaiting_durable.insert(
                            awaiting.entry.client_msg_id.clone(),
                            AwaitingDurableAck {
                                entry: awaiting.entry,
                                deadline_ms: awaiting.deadline_ms,
                            },
                        );
                    }
                }
            } else if st.pending_reader.get(&ack.client_msg_id).await?.is_some() {
                st.pending_acks.insert(ack.client_msg_id.clone(), ack);
            } else if MessageDeliveryService::accepted_from_ack(&ack).is_some()
                || MessageDeliveryService::error_message_from_ack(&ack).is_some()
            {
                if let Err(error) = apply_orphan_durable_ack(st, &ack).await {
                    warn!(
                        %error,
                        client_msg_id = %ack.client_msg_id,
                        "apply orphan durable ack failed"
                    );
                }
                st.bus.publish(SdkEvent::Message(MessageEvent::SendAck {
                    ack: Box::new(ack),
                }));
            }
            try_send_next(st).await?;
        }
        QueueCommand::SendAttemptFinished {
            entry,
            retries,
            result,
        } => {
            handle_send_attempt_finished(st, *entry, retries, result).await?;
        }
        QueueCommand::ResetPendingOnLogin { resp } => {
            let result = reset_pending_on_login(st).await;
            let _ = resp.send(result);
        }
        QueueCommand::RecoverPendingForCurrentUser { resp } => {
            let result = recover_pending_for_current_user(st).await;
            let _ = resp.send(result);
        }
    }
    record_queue_health(st);
    Ok(())
}

fn record_queue_health(st: &QueueState) {
    st.metrics
        .gauge("reliable_queue.in_flight", st.in_flight.len() as i64);
    st.metrics.gauge(
        "reliable_queue.awaiting_durable",
        st.awaiting_durable.len() as i64,
    );
    st.metrics
        .gauge("reliable_queue.pending_acks", st.pending_acks.len() as i64);
    st.metrics.gauge(
        "reliable_queue.adaptive_in_flight",
        st.adaptive_window.current() as i64,
    );
}

fn note_send_success(st: &mut QueueState) {
    if st.adaptive_window.note_success() {
        let current = st.adaptive_window.current();
        debug!(
            current,
            max = st.max_in_flight,
            "reliable queue adaptive in-flight window increased"
        );
        st.metrics
            .gauge("reliable_queue.adaptive_in_flight", current as i64);
    }
}

fn note_send_congestion(st: &mut QueueState, reason: &'static str) {
    if st.adaptive_window.note_congestion() {
        let current = st.adaptive_window.current();
        debug!(
            current,
            max = st.max_in_flight,
            reason,
            "reliable queue adaptive in-flight window reduced"
        );
        st.metrics
            .gauge("reliable_queue.adaptive_in_flight", current as i64);
    }
}

async fn mark_send_failed_and_publish(
    st: &mut QueueState,
    entry: &PendingSendVo,
    reason: &str,
) -> Result<()> {
    let _ = st.pending_writer.pop(&entry.client_msg_id).await?;
    st.retry_count.remove(&entry.client_msg_id);

    let failed_msg = MessageDeliveryService::mark_failed(&entry.message);
    st.message_store.save_batch(&[failed_msg]).await?;
    st.bus.publish(SdkEvent::Message(MessageEvent::SendFailed {
        client_msg_id: entry.client_msg_id.clone(),
        reason: reason.to_string(),
    }));
    Ok(())
}

fn dispatch_send_attempt(st: &mut QueueState, entry: PendingSendVo, retries: u32) {
    st.metrics.counter_with_labels(
        "reliable_queue.send_attempt_total",
        &[MetricLabel::new("retry", retries.to_string())],
        1,
    );
    let client_msg_id = entry.client_msg_id.clone();
    st.in_flight.insert(
        client_msg_id,
        InFlightSend {
            entry: entry.clone(),
            deadline_ms: deadline_after(st.timeout_duration),
        },
    );

    let sender = st.sender.clone();
    let command_tx = st.command_tx.clone();
    let _ = spawn_background_task(async move {
        let result = do_send_one(&sender, &entry).await;
        let _ = command_tx
            .send(QueueCommand::SendAttemptFinished {
                entry: Box::new(entry),
                retries,
                result,
            })
            .await;
    });
}

async fn handle_send_attempt_finished(
    st: &mut QueueState,
    entry: PendingSendVo,
    retries: u32,
    result: Result<()>,
) -> Result<()> {
    if result.is_ok() {
        return Ok(());
    }
    let current_retries = st
        .retry_count
        .get(&entry.client_msg_id)
        .copied()
        .unwrap_or(0);
    if current_retries != retries {
        return Ok(());
    }
    let Some(in_flight) = st.in_flight.remove(&entry.client_msg_id) else {
        return Ok(());
    };
    let entry = in_flight.entry;
    let Err(error) = result else {
        return Ok(());
    };
    note_send_congestion(st, "send_error");
    match MessageDeliveryService::decide_send_attempt_failure(retries, st.max_retries) {
        RetryDecision::Retry { next_retry_count } => {
            st.metrics.counter("reliable_queue.send_retry_total", 1);
            st.retry_count
                .insert(entry.client_msg_id.clone(), next_retry_count);
            warn!(
                %error,
                client_msg_id = %entry.client_msg_id,
                retries = next_retry_count,
                "send attempt failed, keep entry in pending queue for retry"
            );
            st.in_flight.insert(
                entry.client_msg_id.clone(),
                InFlightSend {
                    entry,
                    deadline_ms: deadline_after(st.timeout_duration),
                },
            );
        }
        RetryDecision::Fail { reason } => {
            st.metrics.counter("reliable_queue.send_failed_total", 1);
            let _ = st.pending_writer.pop(&entry.client_msg_id).await;
            st.retry_count.remove(&entry.client_msg_id);
            let failed_msg = MessageDeliveryService::mark_failed(&entry.message);
            if let Err(save_err) = st.message_store.save_batch(&[failed_msg]).await {
                warn!(%save_err, "persist failed message state failed");
            }
            st.bus.publish(SdkEvent::Message(MessageEvent::SendFailed {
                client_msg_id: entry.client_msg_id,
                reason: reason.to_string(),
            }));
            try_send_next(st).await?;
        }
    }
    Ok(())
}

async fn reset_pending_on_login(st: &mut QueueState) -> Result<Vec<String>> {
    let mut dropped = Vec::new();
    st.pending_acks.clear();

    let in_flight_entries = st
        .in_flight
        .drain()
        .map(|(_, in_flight)| in_flight.entry)
        .collect::<Vec<_>>();
    for in_flight in in_flight_entries {
        let id = in_flight.client_msg_id.clone();
        mark_send_failed_and_publish(
            st,
            &in_flight,
            "pending queue dropped during login session reset",
        )
        .await?;
        dropped.push(id);
    }
    let awaiting_entries = st
        .awaiting_durable
        .drain()
        .map(|(_, awaiting)| awaiting.entry)
        .collect::<Vec<_>>();
    for awaiting in awaiting_entries {
        let id = awaiting.client_msg_id.clone();
        mark_send_failed_and_publish(
            st,
            &awaiting,
            "server accepted queue dropped during login session reset",
        )
        .await?;
        dropped.push(id);
    }

    let pending_entries = st.pending_reader.list().await?;
    for entry in pending_entries {
        mark_send_failed_and_publish(
            st,
            &entry,
            "pending queue dropped during login session reset",
        )
        .await?;
        dropped.push(entry.client_msg_id);
    }

    dropped.sort();
    dropped.dedup();
    Ok(dropped)
}

async fn recover_pending_for_current_user(st: &mut QueueState) -> Result<Vec<String>> {
    let current_user_id = st.current_user_id.read().await.clone();
    if current_user_id.trim().is_empty() {
        return Ok(Vec::new());
    }

    let pending_entries = st.pending_reader.list().await?;
    let pending_ids: Vec<String> = pending_entries
        .iter()
        .map(|entry| entry.client_msg_id.clone())
        .collect();

    let mut affected = Vec::new();

    let cross_account_ids = st
        .message_store
        .heal_cross_account_pending_messages(&current_user_id, &pending_ids)
        .await?;
    for client_msg_id in &cross_account_ids {
        let _ = st.pending_writer.pop(client_msg_id).await?;
        st.retry_count.remove(client_msg_id);
        st.pending_acks.remove(client_msg_id);
        st.bus.publish(SdkEvent::Message(MessageEvent::SendFailed {
            client_msg_id: client_msg_id.clone(),
            reason: crate::domain::REASON_PENDING_ANOTHER_ACCOUNT.to_string(),
        }));
    }
    affected.extend(cross_account_ids);

    let remaining_entries = st.pending_reader.list().await?;
    let remaining_ids: Vec<String> = remaining_entries
        .iter()
        .map(|entry| entry.client_msg_id.clone())
        .collect();
    let orphan_ids = st
        .message_store
        .heal_orphan_sending_messages(&current_user_id, &remaining_ids)
        .await?;
    for client_msg_id in &orphan_ids {
        st.bus.publish(SdkEvent::Message(MessageEvent::SendFailed {
            client_msg_id: client_msg_id.clone(),
            reason: REASON_ORPHAN_RECOVERED.to_string(),
        }));
    }
    affected.extend(orphan_ids);

    affected.sort();
    affected.dedup();
    try_send_next(st).await?;
    Ok(affected)
}

async fn check_timeout(st: &mut QueueState) -> Result<()> {
    if reconcile_in_flight_terminal_states(st).await? {
        try_send_next(st).await?;
        return Ok(());
    }
    if reconcile_awaiting_durable_terminal_states(st).await? {
        try_send_next(st).await?;
        return Ok(());
    }
    check_awaiting_durable_timeout(st).await?;
    let ids = st.in_flight.keys().cloned().collect::<Vec<_>>();
    for client_msg_id in ids {
        let Some(in_flight) = st.in_flight.remove(&client_msg_id) else {
            continue;
        };
        let entry = in_flight.entry;
        if is_deadline_elapsed(in_flight.deadline_ms) {
            note_send_congestion(st, "ack_timeout");
            let retries = st
                .retry_count
                .get(&entry.client_msg_id)
                .copied()
                .unwrap_or(0);
            match MessageDeliveryService::decide_timeout_expiry(retries, st.max_retries) {
                RetryDecision::Fail { reason } => {
                    st.metrics.counter_with_labels(
                        "reliable_queue.send_failed_total",
                        &[MetricLabel::new("reason", "timeout")],
                        1,
                    );
                    let _ = st.pending_writer.pop(&entry.client_msg_id).await;
                    st.retry_count.remove(&entry.client_msg_id);
                    let failed_msg = MessageDeliveryService::mark_failed(&entry.message);
                    if let Err(e) = st.message_store.save_batch(&[failed_msg]).await {
                        warn!(%e, "persist failed message state failed");
                    }
                    st.bus.publish(SdkEvent::Message(MessageEvent::SendFailed {
                        client_msg_id: entry.client_msg_id,
                        reason: reason.to_string(),
                    }));
                }
                RetryDecision::Retry { next_retry_count } => {
                    st.metrics.counter_with_labels(
                        "reliable_queue.send_retry_total",
                        &[MetricLabel::new("reason", "timeout")],
                        1,
                    );
                    st.retry_count
                        .insert(entry.client_msg_id.clone(), next_retry_count);
                    dispatch_send_attempt(st, entry, next_retry_count);
                }
            }
        } else {
            st.in_flight.insert(
                entry.client_msg_id.clone(),
                InFlightSend {
                    entry,
                    deadline_ms: in_flight.deadline_ms,
                },
            );
        }
    }
    try_send_next(st).await?;
    Ok(())
}

async fn check_awaiting_durable_timeout(st: &mut QueueState) -> Result<()> {
    let ids = st.awaiting_durable.keys().cloned().collect::<Vec<_>>();
    for client_msg_id in ids {
        let Some(awaiting) = st.awaiting_durable.remove(&client_msg_id) else {
            continue;
        };
        let entry = awaiting.entry;
        if !is_deadline_elapsed(awaiting.deadline_ms) {
            st.awaiting_durable.insert(
                entry.client_msg_id.clone(),
                AwaitingDurableAck {
                    entry,
                    deadline_ms: awaiting.deadline_ms,
                },
            );
            continue;
        }

        note_send_congestion(st, "durable_ack_timeout");
        let retries = st
            .retry_count
            .get(&entry.client_msg_id)
            .copied()
            .unwrap_or(0);
        match MessageDeliveryService::decide_timeout_expiry(retries, st.max_retries) {
            RetryDecision::Retry { next_retry_count } => {
                st.metrics.counter_with_labels(
                    "reliable_queue.send_retry_total",
                    &[MetricLabel::new("reason", "durable_ack_timeout")],
                    1,
                );
                st.retry_count
                    .insert(entry.client_msg_id.clone(), next_retry_count);
                dispatch_send_attempt(st, entry, next_retry_count);
            }
            RetryDecision::Fail { reason } => {
                st.metrics.counter_with_labels(
                    "reliable_queue.send_failed_total",
                    &[MetricLabel::new("reason", "durable_ack_timeout")],
                    1,
                );
                let _ = st.pending_writer.pop(&entry.client_msg_id).await;
                st.retry_count.remove(&entry.client_msg_id);
                let failed_msg = MessageDeliveryService::mark_failed(&entry.message);
                if let Err(e) = st.message_store.save_batch(&[failed_msg]).await {
                    warn!(%e, "persist durable-timeout failed message state failed");
                }
                st.bus.publish(SdkEvent::Message(MessageEvent::SendFailed {
                    client_msg_id: entry.client_msg_id,
                    reason: reason.to_string(),
                }));
            }
        }
    }
    Ok(())
}

/// SDK 内补偿：对账 in-flight 与本地消息终态。
///
/// 典型场景：
/// - SendAck 丢失，但下行消息已落库（status>=Sent 且 server_id/seq 已回填）；
/// - 本地消息已被其他路径收敛为 Failed。
///
/// 命中终态时在 SDK 内原子收敛队列，避免前端长期“发送中”。
async fn reconcile_in_flight_terminal_states(st: &mut QueueState) -> Result<bool> {
    let ids = st.in_flight.keys().cloned().collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(false);
    }
    // 批量读取所有 in-flight 对应本地消息，避免每条一次 DB 往返（高在途时抢占 sqlx 连接池）。
    let snapshot_by_id: HashMap<String, DeliveryLocalSnapshot> = st
        .message_store
        .get_by_client_msg_ids(&ids)
        .await?
        .iter()
        .map(|message| {
            (
                message.client_msg_id.clone(),
                DeliveryLocalSnapshot::from(message),
            )
        })
        .collect();
    let mut progressed = false;

    for client_msg_id in ids {
        let Some(in_flight) = st.in_flight.remove(&client_msg_id) else {
            continue;
        };
        let entry = in_flight.entry;
        let local_snapshot = snapshot_by_id.get(&entry.client_msg_id);

        match MessageDeliveryService::reconcile_in_flight(local_snapshot, &entry.client_msg_id) {
            InFlightReconcileDecision::KeepWaiting => {
                st.in_flight.insert(
                    entry.client_msg_id.clone(),
                    InFlightSend {
                        entry,
                        deadline_ms: in_flight.deadline_ms,
                    },
                );
            }
            InFlightReconcileDecision::MarkFailed { reason } => {
                let _ = st.pending_writer.pop(&entry.client_msg_id).await?;
                st.retry_count.remove(&entry.client_msg_id);
                st.bus.publish(SdkEvent::Message(MessageEvent::SendFailed {
                    client_msg_id: entry.client_msg_id.clone(),
                    reason: reason.to_string(),
                }));
                progressed = true;
            }
            InFlightReconcileDecision::SynthesizeAck { snapshot } => {
                let _ = st.pending_writer.pop(&entry.client_msg_id).await?;
                st.retry_count.remove(&entry.client_msg_id);
                st.bus.publish(SdkEvent::Message(MessageEvent::SendAck {
                    ack: Box::new(MessageDeliveryService::synthetic_ack(
                        &entry.client_msg_id,
                        &snapshot,
                    )),
                }));
                progressed = true;
            }
        }
    }

    Ok(progressed)
}

async fn reconcile_awaiting_durable_terminal_states(st: &mut QueueState) -> Result<bool> {
    let ids = st.awaiting_durable.keys().cloned().collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(false);
    }
    let snapshot_by_id: HashMap<String, DeliveryLocalSnapshot> = st
        .message_store
        .get_by_client_msg_ids(&ids)
        .await?
        .iter()
        .map(|message| {
            (
                message.client_msg_id.clone(),
                DeliveryLocalSnapshot::from(message),
            )
        })
        .collect();
    let mut progressed = false;

    for client_msg_id in ids {
        let Some(awaiting) = st.awaiting_durable.remove(&client_msg_id) else {
            continue;
        };
        let entry = awaiting.entry;
        let local_snapshot = snapshot_by_id.get(&entry.client_msg_id);

        match MessageDeliveryService::reconcile_in_flight(local_snapshot, &entry.client_msg_id) {
            InFlightReconcileDecision::KeepWaiting => {
                st.awaiting_durable.insert(
                    entry.client_msg_id.clone(),
                    AwaitingDurableAck {
                        entry,
                        deadline_ms: awaiting.deadline_ms,
                    },
                );
            }
            InFlightReconcileDecision::MarkFailed { reason } => {
                let _ = st.pending_writer.pop(&entry.client_msg_id).await?;
                st.retry_count.remove(&entry.client_msg_id);
                st.bus.publish(SdkEvent::Message(MessageEvent::SendFailed {
                    client_msg_id: entry.client_msg_id.clone(),
                    reason: reason.to_string(),
                }));
                progressed = true;
            }
            InFlightReconcileDecision::SynthesizeAck { snapshot } => {
                let _ = st.pending_writer.pop(&entry.client_msg_id).await?;
                st.retry_count.remove(&entry.client_msg_id);
                st.bus.publish(SdkEvent::Message(MessageEvent::SendAck {
                    ack: Box::new(MessageDeliveryService::synthetic_ack(
                        &entry.client_msg_id,
                        &snapshot,
                    )),
                }));
                progressed = true;
            }
        }
    }

    Ok(progressed)
}

async fn try_send_next(st: &mut QueueState) -> Result<()> {
    loop {
        let current_limit = st.adaptive_window.current();
        let available = current_limit.saturating_sub(st.in_flight.len());
        if available == 0 {
            return Ok(());
        }
        let mut excluded = st.in_flight.keys().cloned().collect::<Vec<_>>();
        excluded.extend(st.awaiting_durable.keys().cloned());
        let entries = st
            .pending_reader
            .list_oldest_excluding(&excluded, available)
            .await?;
        if entries.is_empty() {
            return Ok(());
        }

        let connected_user_id = st.current_user_id.read().await.clone();
        let entry_ids = entries
            .iter()
            .map(|entry| entry.client_msg_id.clone())
            .collect::<Vec<_>>();
        let local_snapshots = match st.message_store.get_by_client_msg_ids(&entry_ids).await {
            Ok(messages) => messages
                .iter()
                .map(|message| {
                    (
                        message.client_msg_id.clone(),
                        DeliveryLocalSnapshot::from(message),
                    )
                })
                .collect::<HashMap<_, _>>(),
            Err(error) => {
                warn!(%error, "batch load local messages for pending dispatch failed");
                HashMap::new()
            }
        };

        let mut made_progress = false;
        for entry in entries {
            if st.in_flight.len() >= st.adaptive_window.current() {
                return Ok(());
            }
            let retries = st
                .retry_count
                .get(&entry.client_msg_id)
                .copied()
                .unwrap_or(0);
            let local_snapshot = local_snapshots.get(&entry.client_msg_id);
            match MessageDeliveryService::decide_pending_dispatch(
                &connected_user_id,
                &entry.client_msg_id,
                &entry.message.sender_id,
                local_snapshot,
                retries,
                st.max_retries,
            ) {
                PendingDispatchDecision::DropAsCrossAccount { reason } => {
                    debug!(
                        client_msg_id = %entry.client_msg_id,
                        sender_id = %entry.message.sender_id,
                        connected_user_id = %connected_user_id,
                        "reliable queue: drop cross-account pending entry"
                    );
                    let _ = st.pending_writer.pop(&entry.client_msg_id).await;
                    st.retry_count.remove(&entry.client_msg_id);
                    let failed_msg = MessageDeliveryService::mark_failed(&entry.message);
                    if let Err(e) = st.message_store.save_batch(&[failed_msg]).await {
                        warn!(%e, "persist cross-account failed message state failed");
                    }
                    st.bus.publish(SdkEvent::Message(MessageEvent::SendFailed {
                        client_msg_id: entry.client_msg_id.clone(),
                        reason: reason.to_string(),
                    }));
                    made_progress = true;
                    continue;
                }
                PendingDispatchDecision::DropAsTerminal => {
                    debug!(
                        client_msg_id = %entry.client_msg_id,
                        "reliable queue: drop stale pending entry"
                    );
                    let _ = st.pending_writer.pop(&entry.client_msg_id).await;
                    st.retry_count.remove(&entry.client_msg_id);
                    made_progress = true;
                    continue;
                }
                PendingDispatchDecision::FailMaxRetries { reason } => {
                    let _ = st.pending_writer.pop(&entry.client_msg_id).await;
                    st.retry_count.remove(&entry.client_msg_id);
                    let failed_msg = MessageDeliveryService::mark_failed(&entry.message);
                    if let Err(e) = st.message_store.save_batch(&[failed_msg]).await {
                        warn!(%e, "persist failed message state failed");
                    }
                    st.bus.publish(SdkEvent::Message(MessageEvent::SendFailed {
                        client_msg_id: entry.client_msg_id.clone(),
                        reason: reason.to_string(),
                    }));
                    made_progress = true;
                    continue;
                }
                PendingDispatchDecision::SendNow => {}
            }
            if let Some(ack) = st.pending_acks.remove(&entry.client_msg_id) {
                match apply_ack_and_publish(st, &entry, ack).await {
                    AckApplyResult::Terminal => {}
                    AckApplyResult::KeepInFlight => {
                        st.in_flight.insert(
                            entry.client_msg_id.clone(),
                            InFlightSend {
                                entry,
                                deadline_ms: deadline_after(st.timeout_duration),
                            },
                        );
                    }
                    AckApplyResult::AwaitDurable => {
                        st.awaiting_durable.insert(
                            entry.client_msg_id.clone(),
                            AwaitingDurableAck {
                                entry,
                                deadline_ms: deadline_after(st.timeout_duration),
                            },
                        );
                    }
                }
                made_progress = true;
                continue;
            }
            dispatch_send_attempt(st, entry, retries);
            made_progress = true;
        }
        if !made_progress {
            return Ok(());
        }
    }
}

async fn do_send_one(sender: &PacketSender, entry: &PendingSendVo) -> Result<()> {
    let proto = entry.message.to_proto();
    sender.send_message(&proto, Duration::from_secs(15)).await
}

fn conversation_projection_ts(message: &IMMessage) -> u64 {
    if message.conversation_seq > 0 {
        if message.created_at > 0 {
            return message.created_at;
        }
        if message.client_created_at > 0 {
            return message.client_created_at;
        }
    }
    message.timeline_sort_ts()
}

async fn update_conversation_last_message(st: &QueueState, message: &IMMessage) -> Result<()> {
    let conversation_id = message.conversation_id.trim();
    let last_message_id = if message.server_id.trim().is_empty() {
        message.client_msg_id.trim()
    } else {
        message.server_id.trim()
    };
    if conversation_id.is_empty() || last_message_id.is_empty() {
        return Ok(());
    }
    let preview = message.text_for_storage();
    st.conversation_store
        .update_last_message(
            conversation_id,
            last_message_id,
            message.sender_id(),
            conversation_projection_ts(message),
            preview.as_deref(),
            message.conversation_seq,
        )
        .await
}

/// Send-to-ack latency above this is logged at warn so a slow send stands out in client logs
/// without a metrics backend; normal sends stay at debug.
const SLOW_SEND_LOG_THRESHOLD_MS: u64 = 1000;

fn log_send_latency(client_msg_id: &str, latency_ms: u64, stage: &str) {
    if latency_ms > SLOW_SEND_LOG_THRESHOLD_MS {
        warn!(%client_msg_id, latency_ms, stage, "slow send: ack latency exceeded threshold");
    } else {
        debug!(%client_msg_id, latency_ms, stage, "send ack latency");
    }
}

async fn apply_ack_and_publish(
    st: &mut QueueState,
    entry: &PendingSendVo,
    ack: SendAck,
) -> AckApplyResult {
    if MessageDeliveryService::durable_accepted_from_ack(&ack).is_some() {
        let _ = st.pending_writer.pop(&ack.client_msg_id).await;
        st.retry_count.remove(&ack.client_msg_id);
        let latency_ms = id::now_millis().saturating_sub(entry.enqueued_at_ms);
        st.metrics
            .histogram("reliable_queue.send_ack_latency_ms", latency_ms as f64);
        log_send_latency(&ack.client_msg_id, latency_ms, "durable");
        note_send_success(st);
        let msg = MessageDeliveryService::mark_sent_from_ack(&entry.message, &ack);
        let cid = ack.client_msg_id.clone();
        if let Err(e) = st.message_store.update_after_ack(&cid, &msg).await {
            warn!(%e, "update_after_ack failed");
        }
        if let Err(e) = update_conversation_last_message(st, &msg).await {
            warn!(%e, conversation_id = %msg.conversation_id, "update conversation projection after ack failed");
        }
        st.bus.publish(SdkEvent::Message(MessageEvent::SendAck {
            ack: Box::new(ack),
        }));
        return AckApplyResult::Terminal;
    }
    if MessageDeliveryService::accepted_from_ack(&ack).is_some() {
        let latency_ms = id::now_millis().saturating_sub(entry.enqueued_at_ms);
        st.metrics.histogram(
            "reliable_queue.server_accepted_latency_ms",
            latency_ms as f64,
        );
        log_send_latency(&ack.client_msg_id, latency_ms, "accepted");
        note_send_success(st);
        st.bus.publish(SdkEvent::Message(MessageEvent::SendAck {
            ack: Box::new(ack),
        }));
        return AckApplyResult::AwaitDurable;
    }
    let _ = st.pending_writer.pop(&ack.client_msg_id).await;
    st.retry_count.remove(&ack.client_msg_id);
    let failed_msg = MessageDeliveryService::mark_failed(&entry.message);
    if let Err(e) = st.message_store.save_batch(&[failed_msg]).await {
        warn!(%e, "persist failed message state from ack failed");
    }
    let reason = MessageDeliveryService::error_message_from_ack(&ack)
        .unwrap_or_else(|| "send ack missing accepted result".to_string());
    let client_msg_id = ack.client_msg_id.clone();
    st.bus.publish(SdkEvent::Message(MessageEvent::SendAck {
        ack: Box::new(ack),
    }));
    st.bus.publish(SdkEvent::Message(MessageEvent::SendFailed {
        client_msg_id,
        reason,
    }));
    AckApplyResult::Terminal
}

async fn apply_orphan_durable_ack(st: &mut QueueState, ack: &SendAck) -> Result<()> {
    let Some(accepted) = MessageDeliveryService::durable_accepted_from_ack(ack) else {
        return Ok(());
    };
    let client_msg_id = ack.client_msg_id.trim();
    if client_msg_id.is_empty() {
        return Ok(());
    }
    let Some(local) = st.message_store.get_by_client_msg_id(client_msg_id).await? else {
        return Ok(());
    };
    if !orphan_durable_ack_should_update(&local, client_msg_id, accepted.conversation_seq) {
        return Ok(());
    }
    let msg = MessageDeliveryService::mark_sent_from_ack(&local, ack);
    st.message_store
        .update_after_ack(client_msg_id, &msg)
        .await?;
    update_conversation_last_message(st, &msg).await?;
    Ok(())
}

fn orphan_durable_ack_should_update(local: &IMMessage, client_msg_id: &str, ack_seq: u64) -> bool {
    let pending_like = local.server_id == client_msg_id
        || local.local_state.is_local
        || local.local_state.sending
        || local.status < MessageStatus::Sent as i32;
    pending_like || local.conversation_seq == 0 || ack_seq > local.conversation_seq
}

#[cfg(all(test, feature = "storage-sqlite"))]
mod tests {
    use super::{ReliableSendQueue, ReliableSendQueueConfig};
    use crate::domain::{
        ConversationReader, ConversationWriter, MessageReader, MessageWriter, PendingSendReader,
        PendingSendVo, PendingSendWriter,
    };
    use crate::infrastructure::persistence::sqlite::{
        SqliteConversationRepo, SqliteMessageRepo, SqlitePendingSendRepo,
        init_schema as sqlite_init_schema,
    };
    use crate::infrastructure::protocol::{Codec, PacketSender, ProtobufCodec};
    use crate::kernel::CurrentUserIdStore;
    use crate::kernel::event::{EventBus, MessageEvent, SdkEvent};
    use crate::model::message::MessageStatus;
    use crate::model::{Conversation, IMMessage};
    use crate::spi::metrics::MetricsRecorder;
    use flare_proto::common::{SendAccepted, SendAck, SendAckDurability, send_ack};
    use sqlx::SqlitePool;
    use std::sync::Arc;
    use tokio::sync::{Mutex, RwLock};
    use tokio::time::{Duration, timeout};

    fn dummy_sender() -> Arc<PacketSender> {
        Arc::new(PacketSender::new(
            Arc::new(Mutex::new(None)),
            Arc::new(ProtobufCodec) as Arc<dyn Codec>,
        ))
    }

    #[cfg(feature = "storage-sqlite")]
    #[tokio::test]
    async fn enqueue_returns_after_optimistic_message_is_persisted() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite_init_schema(&pool).await.unwrap();
        let pending_store = Arc::new(SqlitePendingSendRepo::new(pool.clone()));
        let message_store = Arc::new(SqliteMessageRepo::new(pool.clone()));
        let conversation_store = Arc::new(SqliteConversationRepo::new(pool));
        let queue = ReliableSendQueue::new(ReliableSendQueueConfig {
            pending_reader: pending_store.clone(),
            pending_writer: pending_store,
            sender: dummy_sender(),
            message_store: message_store.clone(),
            conversation_store,
            current_user_id,
            bus: bus.clone(),
            timeout_secs: Some(60),
            max_retries: Some(3),
            max_in_flight: Some(32),
            metrics: MetricsRecorder::disabled(),
        });
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.client_msg_id = "client-1".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();

        queue.enqueue(message).await.unwrap();

        let stored = message_store
            .get_by_client_msg_id("client-1")
            .await
            .unwrap()
            .expect("optimistic message should be visible after enqueue returns");
        assert_eq!(stored.server_id, "client-1");
        assert!(stored.local_state.sending);
        assert!(stored.local_state.is_local);

        let event = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected optimistic message event")
            .expect("bus closed");
        match event {
            SdkEvent::Message(MessageEvent::ReceivedBatch { messages }) => {
                let optimistic = messages
                    .first()
                    .expect("optimistic message batch should not be empty");
                assert_eq!(optimistic.client_msg_id, "client-1");
                assert!(optimistic.local_state.sending);
                assert!(optimistic.local_state.is_local);
            }
            other => panic!("expected optimistic ReceivedBatch event, got {other:?}"),
        }
    }

    #[cfg(feature = "storage-sqlite")]
    #[tokio::test]
    async fn ack_updates_conversation_last_message_projection_before_publish() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite_init_schema(&pool).await.unwrap();
        let pending_store = Arc::new(SqlitePendingSendRepo::new(pool.clone()));
        let message_store = Arc::new(SqliteMessageRepo::new(pool.clone()));
        let conversation_store = Arc::new(SqliteConversationRepo::new(pool));
        conversation_store
            .save_one(&Conversation {
                conversation_id: "conv-ack".to_string(),
                last_message_id: Some("server-old".to_string()),
                last_sender_id: Some("u2".to_string()),
                last_message_at: Some(1_000),
                last_message_preview: Some("old".to_string()),
                max_seq: 4,
                ..Default::default()
            })
            .await
            .unwrap();
        let queue = ReliableSendQueue::new(ReliableSendQueueConfig {
            pending_reader: pending_store.clone(),
            pending_writer: pending_store,
            sender: dummy_sender(),
            message_store,
            conversation_store: conversation_store.clone(),
            current_user_id,
            bus: bus.clone(),
            timeout_secs: Some(60),
            max_retries: Some(3),
            max_in_flight: Some(32),
            metrics: MetricsRecorder::disabled(),
        });
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.client_msg_id = "client-ack".to_string();
        message.server_id = "client-ack".to_string();
        message.conversation_id = "conv-ack".to_string();
        message.sender_id = "u1".to_string();

        queue.enqueue(message).await.unwrap();
        let optimistic_event = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected optimistic message event")
            .expect("bus closed");
        assert!(matches!(
            optimistic_event,
            SdkEvent::Message(MessageEvent::ReceivedBatch { .. })
        ));
        tokio::time::sleep(Duration::from_millis(20)).await;
        queue
            .on_ack(SendAck {
                client_msg_id: "client-ack".to_string(),
                conversation_id: "conv-ack".to_string(),
                result: Some(send_ack::Result::Accepted(SendAccepted {
                    server_msg_id: "server-new".to_string(),
                    conversation_seq: 5,
                    server_time: 0,
                    durability: SendAckDurability::Persisted as i32,
                })),
                ..Default::default()
            })
            .await
            .unwrap();

        let event = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected send ack")
            .expect("bus closed");
        assert!(matches!(
            event,
            SdkEvent::Message(MessageEvent::SendAck { .. })
        ));
        let updated = conversation_store
            .get("conv-ack")
            .await
            .unwrap()
            .expect("conversation should exist");
        assert_eq!(updated.last_message_id.as_deref(), Some("server-new"));
        assert_eq!(updated.last_sender_id.as_deref(), Some("u1"));
        assert_eq!(updated.max_seq, 5);
        assert_ne!(updated.last_message_preview.as_deref(), Some("old"));
        assert_eq!(updated.unread_count, 0);
    }

    #[cfg(feature = "storage-sqlite")]
    #[tokio::test]
    async fn broker_ack_keeps_pending_message_recoverable() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite_init_schema(&pool).await.unwrap();
        let pending_store = Arc::new(SqlitePendingSendRepo::new(pool.clone()));
        let message_store = Arc::new(SqliteMessageRepo::new(pool.clone()));
        let conversation_store = Arc::new(SqliteConversationRepo::new(pool));
        let queue = ReliableSendQueue::new(ReliableSendQueueConfig {
            pending_reader: pending_store.clone(),
            pending_writer: pending_store.clone(),
            sender: dummy_sender(),
            message_store: message_store.clone(),
            conversation_store,
            current_user_id,
            bus,
            timeout_secs: Some(60),
            max_retries: Some(3),
            max_in_flight: Some(32),
            metrics: MetricsRecorder::disabled(),
        });
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.client_msg_id = "client-broker".to_string();
        message.server_id = "client-broker".to_string();
        message.conversation_id = "conv-broker".to_string();
        message.sender_id = "u1".to_string();

        queue.enqueue(message).await.unwrap();
        let optimistic_event = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected optimistic message event")
            .expect("bus closed");
        assert!(matches!(
            optimistic_event,
            SdkEvent::Message(MessageEvent::ReceivedBatch { .. })
        ));
        tokio::time::sleep(Duration::from_millis(20)).await;
        queue
            .on_ack(SendAck {
                client_msg_id: "client-broker".to_string(),
                conversation_id: "conv-broker".to_string(),
                result: Some(send_ack::Result::Accepted(SendAccepted {
                    server_msg_id: "server-broker".to_string(),
                    conversation_seq: 9,
                    server_time: 0,
                    durability: SendAckDurability::BrokerAccepted as i32,
                })),
                ..Default::default()
            })
            .await
            .unwrap();

        let event = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected broker send ack")
            .expect("bus closed");
        assert!(matches!(
            event,
            SdkEvent::Message(MessageEvent::SendAck { .. })
        ));
        assert!(
            pending_store.get("client-broker").await.unwrap().is_some(),
            "broker ack must not clear recoverable pending state"
        );
        let local = message_store
            .get_by_client_msg_id("client-broker")
            .await
            .unwrap()
            .expect("local optimistic message should remain");
        assert!(local.local_state.sending);
        assert!(local.local_state.is_local);
        assert_eq!(local.server_id, "client-broker");
        assert_eq!(local.conversation_seq, 0);
    }

    #[cfg(feature = "storage-sqlite")]
    #[tokio::test]
    async fn broker_ack_releases_window_without_clearing_pending() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite_init_schema(&pool).await.unwrap();
        let pending_store = Arc::new(SqlitePendingSendRepo::new(pool.clone()));
        let message_store = Arc::new(SqliteMessageRepo::new(pool.clone()));
        let conversation_store = Arc::new(SqliteConversationRepo::new(pool));
        let queue = ReliableSendQueue::new(ReliableSendQueueConfig {
            pending_reader: pending_store.clone(),
            pending_writer: pending_store.clone(),
            sender: dummy_sender(),
            message_store,
            conversation_store,
            current_user_id,
            bus,
            timeout_secs: Some(60),
            max_retries: Some(3),
            max_in_flight: Some(1),
            metrics: MetricsRecorder::disabled(),
        });

        for client_msg_id in ["client-broker-window-1", "client-broker-window-2"] {
            let mut message = IMMessage::new(flare_proto::common::Message::default());
            message.client_msg_id = client_msg_id.to_string();
            message.server_id = client_msg_id.to_string();
            message.conversation_id = "conv-broker-window".to_string();
            message.sender_id = "u1".to_string();
            queue.enqueue(message).await.unwrap();
        }
        for _ in 0..2 {
            let optimistic_event = timeout(Duration::from_millis(200), receiver.recv())
                .await
                .expect("expected optimistic message event")
                .expect("bus closed");
            assert!(matches!(
                optimistic_event,
                SdkEvent::Message(MessageEvent::ReceivedBatch { .. })
            ));
        }

        queue
            .on_ack(SendAck {
                client_msg_id: "client-broker-window-1".to_string(),
                conversation_id: "conv-broker-window".to_string(),
                result: Some(send_ack::Result::Accepted(SendAccepted {
                    server_msg_id: "server-broker-window-1".to_string(),
                    conversation_seq: 1,
                    server_time: 0,
                    durability: SendAckDurability::BrokerAccepted as i32,
                })),
                ..Default::default()
            })
            .await
            .unwrap();

        let broker_event = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected broker send ack")
            .expect("bus closed");
        assert!(matches!(
            broker_event,
            SdkEvent::Message(MessageEvent::SendAck { .. })
        ));
        assert!(
            pending_store
                .get("client-broker-window-1")
                .await
                .unwrap()
                .is_some(),
            "broker ack must keep first message recoverable"
        );

        queue
            .on_ack(SendAck {
                client_msg_id: "client-broker-window-2".to_string(),
                conversation_id: "conv-broker-window".to_string(),
                result: Some(send_ack::Result::Accepted(SendAccepted {
                    server_msg_id: "server-broker-window-2".to_string(),
                    conversation_seq: 2,
                    server_time: 0,
                    durability: SendAckDurability::Persisted as i32,
                })),
                ..Default::default()
            })
            .await
            .unwrap();

        let persisted_event = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected second persisted send ack after broker window release")
            .expect("bus closed");
        match persisted_event {
            SdkEvent::Message(MessageEvent::SendAck { ack }) => {
                assert_eq!(ack.client_msg_id, "client-broker-window-2");
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(
            pending_store
                .get("client-broker-window-2")
                .await
                .unwrap()
                .is_none(),
            "persisted ack should clear the second message"
        );
    }

    #[cfg(feature = "storage-sqlite")]
    #[tokio::test]
    async fn orphan_durable_ack_is_published() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite_init_schema(&pool).await.unwrap();
        let pending_store = Arc::new(SqlitePendingSendRepo::new(pool.clone()));
        let message_store = Arc::new(SqliteMessageRepo::new(pool.clone()));
        let conversation_store = Arc::new(SqliteConversationRepo::new(pool));
        let queue = ReliableSendQueue::new(ReliableSendQueueConfig {
            pending_reader: pending_store.clone(),
            pending_writer: pending_store,
            sender: dummy_sender(),
            message_store,
            conversation_store,
            current_user_id,
            bus,
            timeout_secs: Some(60),
            max_retries: Some(3),
            max_in_flight: Some(32),
            metrics: MetricsRecorder::disabled(),
        });

        queue
            .on_ack(SendAck {
                client_msg_id: "client-orphan".to_string(),
                conversation_id: "conv-orphan".to_string(),
                result: Some(send_ack::Result::Accepted(SendAccepted {
                    server_msg_id: "server-orphan".to_string(),
                    conversation_seq: 10,
                    server_time: 0,
                    durability: SendAckDurability::Persisted as i32,
                })),
                ..Default::default()
            })
            .await
            .unwrap();

        let event = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected orphan durable send ack")
            .expect("bus closed");
        match event {
            SdkEvent::Message(MessageEvent::SendAck { ack }) => {
                assert_eq!(ack.client_msg_id, "client-orphan");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[cfg(feature = "storage-sqlite")]
    #[tokio::test]
    async fn orphan_later_persisted_ack_advances_local_canonical_seq() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite_init_schema(&pool).await.unwrap();
        let pending_store = Arc::new(SqlitePendingSendRepo::new(pool.clone()));
        let message_store = Arc::new(SqliteMessageRepo::new(pool.clone()));
        let conversation_store = Arc::new(SqliteConversationRepo::new(pool));
        let mut local = IMMessage::new(flare_proto::common::Message::default());
        local.server_id = "server-old".to_string();
        local.client_msg_id = "client-late-persisted".to_string();
        local.conversation_id = "conv-late-persisted".to_string();
        local.sender_id = "u1".to_string();
        local.conversation_seq = 10;
        local.status = MessageStatus::Sent as i32;
        message_store.save_batch(&[local]).await.unwrap();

        let queue = ReliableSendQueue::new(ReliableSendQueueConfig {
            pending_reader: pending_store.clone(),
            pending_writer: pending_store,
            sender: dummy_sender(),
            message_store: message_store.clone(),
            conversation_store,
            current_user_id,
            bus,
            timeout_secs: Some(60),
            max_retries: Some(3),
            max_in_flight: Some(32),
            metrics: MetricsRecorder::disabled(),
        });

        queue
            .on_ack(SendAck {
                client_msg_id: "client-late-persisted".to_string(),
                conversation_id: "conv-late-persisted".to_string(),
                result: Some(send_ack::Result::Accepted(SendAccepted {
                    server_msg_id: "server-new".to_string(),
                    conversation_seq: 42,
                    server_time: 1234,
                    durability: SendAckDurability::Persisted as i32,
                })),
                ..Default::default()
            })
            .await
            .unwrap();

        let event = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected orphan persisted send ack")
            .expect("bus closed");
        assert!(matches!(
            event,
            SdkEvent::Message(MessageEvent::SendAck { .. })
        ));
        assert!(message_store.get("server-old").await.unwrap().is_none());
        let stored = message_store
            .get_by_client_msg_id("client-late-persisted")
            .await
            .unwrap()
            .expect("canonical row should remain addressable by client id");
        assert_eq!(stored.server_id, "server-new");
        assert_eq!(stored.conversation_seq, 42);
    }

    #[cfg(feature = "storage-sqlite")]
    #[tokio::test]
    async fn pipelined_queue_accepts_out_of_order_durable_acks() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite_init_schema(&pool).await.unwrap();
        let pending_store = Arc::new(SqlitePendingSendRepo::new(pool.clone()));
        let message_store = Arc::new(SqliteMessageRepo::new(pool.clone()));
        let conversation_store = Arc::new(SqliteConversationRepo::new(pool));
        let queue = ReliableSendQueue::new(ReliableSendQueueConfig {
            pending_reader: pending_store.clone(),
            pending_writer: pending_store.clone(),
            sender: dummy_sender(),
            message_store,
            conversation_store,
            current_user_id,
            bus,
            timeout_secs: Some(60),
            max_retries: Some(3),
            max_in_flight: Some(2),
            metrics: MetricsRecorder::disabled(),
        });
        for client_msg_id in ["client-pipe-1", "client-pipe-2"] {
            let mut message = IMMessage::new(flare_proto::common::Message::default());
            message.client_msg_id = client_msg_id.to_string();
            message.server_id = client_msg_id.to_string();
            message.conversation_id = "conv-pipe".to_string();
            message.sender_id = "u1".to_string();
            queue.enqueue(message).await.unwrap();
        }
        for _ in 0..2 {
            let optimistic_event = timeout(Duration::from_millis(200), receiver.recv())
                .await
                .expect("expected optimistic message event")
                .expect("bus closed");
            assert!(matches!(
                optimistic_event,
                SdkEvent::Message(MessageEvent::ReceivedBatch { .. })
            ));
        }

        tokio::time::sleep(Duration::from_millis(20)).await;
        queue
            .on_ack(SendAck {
                client_msg_id: "client-pipe-2".to_string(),
                conversation_id: "conv-pipe".to_string(),
                result: Some(send_ack::Result::Accepted(SendAccepted {
                    server_msg_id: "server-pipe-2".to_string(),
                    conversation_seq: 2,
                    server_time: 0,
                    durability: SendAckDurability::Persisted as i32,
                })),
                ..Default::default()
            })
            .await
            .unwrap();

        let event = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected second send ack")
            .expect("bus closed");
        match event {
            SdkEvent::Message(MessageEvent::SendAck { ack }) => {
                assert_eq!(ack.client_msg_id, "client-pipe-2");
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(
            pending_store.get("client-pipe-1").await.unwrap().is_some(),
            "first in-flight message should remain pending"
        );
        assert!(
            pending_store.get("client-pipe-2").await.unwrap().is_none(),
            "second in-flight message should be independently cleared"
        );
    }

    #[cfg(feature = "storage-sqlite")]
    #[tokio::test]
    async fn recover_pending_for_current_user_drops_cross_account_entries() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite_init_schema(&pool).await.unwrap();
        let pending_store = Arc::new(SqlitePendingSendRepo::new(pool.clone()));
        let message_store = Arc::new(SqliteMessageRepo::new(pool.clone()));
        let conversation_store = Arc::new(SqliteConversationRepo::new(pool));
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "c1".to_string();
        message.client_msg_id = "c1".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u2".to_string();
        message.local_state.sending = true;
        message.local_state.is_local = true;
        message_store.save_batch(&[message.clone()]).await.unwrap();
        pending_store
            .push(PendingSendVo {
                client_msg_id: "c1".to_string(),
                conversation_id: "conv-1".to_string(),
                message,
                enqueued_at_ms: 1,
            })
            .await
            .unwrap();
        let queue = ReliableSendQueue::new(ReliableSendQueueConfig {
            pending_reader: pending_store.clone(),
            pending_writer: pending_store.clone(),
            sender: dummy_sender(),
            message_store,
            conversation_store,
            current_user_id,
            bus: bus.clone(),
            timeout_secs: Some(60),
            max_retries: Some(3),
            max_in_flight: Some(32),
            metrics: MetricsRecorder::disabled(),
        });

        let affected = queue.recover_pending_for_current_user().await.unwrap();
        assert_eq!(affected, vec!["c1".to_string()]);
        assert!(pending_store.list().await.unwrap().is_empty());

        let event = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected send failed")
            .expect("bus closed");
        match event {
            SdkEvent::Message(MessageEvent::SendFailed { client_msg_id, .. }) => {
                assert_eq!(client_msg_id, "c1");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[cfg(feature = "storage-sqlite")]
    #[tokio::test]
    async fn recover_pending_for_current_user_publishes_orphan_failures() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_raw();
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite_init_schema(&pool).await.unwrap();
        let pending_store = Arc::new(SqlitePendingSendRepo::new(pool.clone()));
        let message_store = Arc::new(SqliteMessageRepo::new(pool.clone()));
        let conversation_store = Arc::new(SqliteConversationRepo::new(pool));
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "orphan-1".to_string();
        message.client_msg_id = "orphan-1".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();
        message.local_state.sending = true;
        message.local_state.is_local = true;
        message_store.save_batch(&[message]).await.unwrap();
        let queue = ReliableSendQueue::new(ReliableSendQueueConfig {
            pending_reader: pending_store.clone(),
            pending_writer: pending_store,
            sender: dummy_sender(),
            message_store,
            conversation_store,
            current_user_id,
            bus: bus.clone(),
            timeout_secs: Some(60),
            max_retries: Some(3),
            max_in_flight: Some(32),
            metrics: MetricsRecorder::disabled(),
        });

        let affected = queue.recover_pending_for_current_user().await.unwrap();
        assert_eq!(affected, vec!["orphan-1".to_string()]);

        let event = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected orphan send failed")
            .expect("bus closed");
        match event {
            SdkEvent::Message(MessageEvent::SendFailed {
                client_msg_id,
                reason,
            }) => {
                assert_eq!(client_msg_id, "orphan-1");
                assert!(reason.contains("orphan"));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
