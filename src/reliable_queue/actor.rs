//! 可靠队列 Actor — 单任务循环：enqueue / ack / timeout → 转移

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use flare_proto::common::SendAck;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, interval_at};
use tracing::{debug, warn};

use crate::domain::{
    DeliveryLocalSnapshot, InFlightReconcileDecision, MessageDeliveryService, MessageStore,
    PendingDispatchDecision, PendingSendReader, PendingSendVo, PendingSendWriter,
    REASON_ORPHAN_RECOVERED, RetryDecision,
};
use crate::error::{ErrorCode, FlareError, Result};
use crate::event::{EventBus, MessageEvent, SdkEvent};
use crate::model::IMMessage;
use crate::model::message::MessageLocalState;
use crate::protocol::PacketSender;
use crate::util::id;
use crate::util::{RELIABLE_QUEUE_MAX_RETRIES, RELIABLE_QUEUE_TIMEOUT_SECS};

/// 队列命令（仅通过此与队列通信）
#[derive(Debug)]
pub enum QueueCommand {
    /// 入队发送（SDK 内部统一 IMMessage）
    Enqueue {
        message: IMMessage,
        resp: oneshot::Sender<Result<()>>,
    },
    /// 收到 SendAck（由外部收到下行后注入）
    AckReceived(SendAck),
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
    worker: tokio::task::JoinHandle<()>,
}

struct QueueState {
    pending_reader: Arc<dyn PendingSendReader>,
    pending_writer: Arc<dyn PendingSendWriter>,
    sender: Arc<PacketSender>,
    message_store: Arc<dyn MessageStore>,
    current_user_id: crate::core::CurrentUserIdStore,
    bus: EventBus,
    timeout_duration: Duration,
    max_retries: u32,
    /// 当前在途消息（仅一条，严格顺序）
    in_flight: Option<(PendingSendVo, Instant)>,
    /// client_msg_id -> 已重试次数
    retry_count: HashMap<String, u32>,
    /// 提前/乱序到达的 ACK，等待对应 pending 成为当前处理项后再收敛
    pending_acks: HashMap<String, SendAck>,
}

impl ReliableSendQueue {
    /// 构建队列并启动后台任务；收到 ack 需由调用方往 `on_ack` 注入。
    /// reader 与 writer 通常为同一实现体（如 SqlitePendingSendRepo）的两个 Arc。
    pub fn new(
        pending_reader: Arc<dyn PendingSendReader>,
        pending_writer: Arc<dyn PendingSendWriter>,
        sender: Arc<PacketSender>,
        message_store: Arc<dyn MessageStore>,
        current_user_id: crate::core::CurrentUserIdStore,
        bus: EventBus,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
    ) -> Self {
        let (tx, mut rx) = mpsc::channel::<QueueCommand>(256);
        let timeout_duration =
            Duration::from_secs(timeout_secs.unwrap_or(RELIABLE_QUEUE_TIMEOUT_SECS));
        let max_retries = max_retries.unwrap_or(RELIABLE_QUEUE_MAX_RETRIES);

        let state = Arc::new(tokio::sync::Mutex::new(QueueState {
            pending_reader,
            pending_writer,
            sender,
            message_store,
            current_user_id,
            bus,
            timeout_duration,
            max_retries,
            in_flight: None,
            retry_count: HashMap::new(),
            pending_acks: HashMap::new(),
        }));

        let state_clone = state.clone();
        let worker = tokio::spawn(async move {
            let mut ticker = interval_at(Instant::now(), Duration::from_secs(1));
            loop {
                tokio::select! {
                    Some(cmd) = rx.recv() => {
                        if let Err(e) = handle_command(state_clone.clone(), cmd).await {
                            warn!(%e, "reliable queue command error");
                        }
                    }
                    _ = ticker.tick() => {
                        if let Err(e) = check_timeout(state_clone.clone()).await {
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
            .send(QueueCommand::Enqueue { message, resp })
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
            .send(QueueCommand::AckReceived(ack))
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
}

impl Drop for ReliableSendQueue {
    fn drop(&mut self) {
        // Drop JoinHandle 会 detach 任务；显式 abort 才能在会话结束时停止旧队列重试。
        self.worker.abort();
    }
}

async fn handle_command(
    state: Arc<tokio::sync::Mutex<QueueState>>,
    cmd: QueueCommand,
) -> Result<()> {
    let mut st = state.lock().await;
    match cmd {
        QueueCommand::Enqueue { message: msg, resp } => {
            let enqueued_at_ms = id::now_millis();
            let mut optimistic = msg.clone();
            optimistic.server_id = msg.client_msg_id.clone();
            optimistic.local_state = MessageLocalState {
                sending: true,
                failed: false,
                is_local: true,
                sort_ts: enqueued_at_ms,
            };
            let entry = PendingSendVo {
                client_msg_id: msg.client_msg_id.clone(),
                conversation_id: msg.conversation_id.clone(),
                message: msg,
                enqueued_at_ms,
            };
            // 持锁期间不要做 SQLite：与 1s ticker / check_timeout 争用会把整池拖死并触发 sqlx slow acquire。
            let (message_store, pending_writer) =
                (st.message_store.clone(), st.pending_writer.clone());
            drop(st);
            if let Err(e) = message_store.save_batch(&[optimistic]).await {
                let _ = resp.send(Err(e));
                return Ok(());
            }
            if let Err(e) = pending_writer.push(entry).await {
                let _ = resp.send(Err(e));
                return Ok(());
            }
            let _ = resp.send(Ok(()));
            let mut st = state.lock().await;
            try_send_next(&mut st).await?;
        }
        QueueCommand::AckReceived(ack) => {
            if let Some((entry, deadline)) = st.in_flight.take() {
                if entry.client_msg_id == ack.client_msg_id {
                    apply_ack_and_publish(&mut st, &entry, ack).await;
                } else {
                    st.in_flight = Some((entry, deadline));
                    if st.pending_reader.get(&ack.client_msg_id).await?.is_some() {
                        st.pending_acks.insert(ack.client_msg_id.clone(), ack);
                    }
                }
            } else if st.pending_reader.get(&ack.client_msg_id).await?.is_some() {
                st.pending_acks.insert(ack.client_msg_id.clone(), ack);
            }
            try_send_next(&mut st).await?;
        }
        QueueCommand::ResetPendingOnLogin { resp } => {
            let result = reset_pending_on_login(&mut st).await;
            let _ = resp.send(result);
        }
        QueueCommand::RecoverPendingForCurrentUser { resp } => {
            let result = recover_pending_for_current_user(&mut st).await;
            let _ = resp.send(result);
        }
    }
    Ok(())
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

async fn reset_pending_on_login(st: &mut QueueState) -> Result<Vec<String>> {
    let mut dropped = Vec::new();
    st.pending_acks.clear();

    if let Some((in_flight, _)) = st.in_flight.take() {
        let id = in_flight.client_msg_id.clone();
        mark_send_failed_and_publish(
            st,
            &in_flight,
            "pending queue dropped during login session reset",
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

async fn check_timeout(state: Arc<tokio::sync::Mutex<QueueState>>) -> Result<()> {
    let mut st = state.lock().await;
    if reconcile_in_flight_terminal_state(&mut st).await? {
        try_send_next(&mut st).await?;
        return Ok(());
    }
    let now = Instant::now();
    if let Some((entry, deadline)) = st.in_flight.take() {
        if now >= deadline {
            let retries = st
                .retry_count
                .get(&entry.client_msg_id)
                .copied()
                .unwrap_or(0);
            match MessageDeliveryService::decide_timeout_expiry(retries, st.max_retries) {
                RetryDecision::Fail { reason } => {
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
                    st.retry_count
                        .insert(entry.client_msg_id.clone(), next_retry_count);
                    if let Err(e) = do_send_one(&st.sender, &entry).await {
                        warn!(%e, "retry send failed");
                    }
                    st.in_flight = Some((entry, now + st.timeout_duration));
                    return Ok(());
                }
            }
        } else {
            st.in_flight = Some((entry, deadline));
        }
    }
    try_send_next(&mut st).await?;
    Ok(())
}

/// SDK 内补偿：对账 in-flight 与本地消息终态。
///
/// 典型场景：
/// - SendAck 丢失，但下行消息已落库（status>=Sent 且 server_id/seq 已回填）；
/// - 本地消息已被其他路径收敛为 Failed。
///
/// 命中终态时在 SDK 内原子收敛队列，避免前端长期“发送中”。
async fn reconcile_in_flight_terminal_state(st: &mut QueueState) -> Result<bool> {
    let Some((entry, deadline)) = st.in_flight.take() else {
        return Ok(false);
    };

    let local = st
        .message_store
        .get_by_client_msg_id(&entry.client_msg_id)
        .await?;
    let local_snapshot = local.as_ref().map(DeliveryLocalSnapshot::from);

    match MessageDeliveryService::reconcile_in_flight(local_snapshot.as_ref(), &entry.client_msg_id)
    {
        InFlightReconcileDecision::KeepWaiting => {
            st.in_flight = Some((entry, deadline));
            Ok(false)
        }
        InFlightReconcileDecision::MarkFailed { reason } => {
            let _ = st.pending_writer.pop(&entry.client_msg_id).await?;
            st.retry_count.remove(&entry.client_msg_id);
            st.bus.publish(SdkEvent::Message(MessageEvent::SendFailed {
                client_msg_id: entry.client_msg_id.clone(),
                reason: reason.to_string(),
            }));
            Ok(true)
        }
        InFlightReconcileDecision::SynthesizeAck { snapshot } => {
            let _ = st.pending_writer.pop(&entry.client_msg_id).await?;
            st.retry_count.remove(&entry.client_msg_id);
            st.bus.publish(SdkEvent::Message(MessageEvent::SendAck {
                ack: MessageDeliveryService::synthetic_ack(&entry.client_msg_id, &snapshot),
            }));
            Ok(true)
        }
    }
}

async fn try_send_next(st: &mut QueueState) -> Result<()> {
    loop {
        if st.in_flight.is_some() {
            return Ok(());
        }
        let entry = match st.pending_reader.take_oldest().await? {
            Some(e) => e,
            None => return Ok(()),
        };
        let connected_user_id = st.current_user_id.read().await.clone();
        let retries = st
            .retry_count
            .get(&entry.client_msg_id)
            .copied()
            .unwrap_or(0);
        let local_snapshot = match st
            .message_store
            .get_by_client_msg_id(&entry.client_msg_id)
            .await
        {
            Ok(local) => local.as_ref().map(DeliveryLocalSnapshot::from),
            Err(error) => {
                warn!(%error, "load local message for pending dispatch failed");
                None
            }
        };
        match MessageDeliveryService::decide_pending_dispatch(
            &connected_user_id,
            &entry.client_msg_id,
            &entry.message.sender_id,
            local_snapshot.as_ref(),
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
                continue;
            }
            PendingDispatchDecision::DropAsTerminal => {
                debug!(
                    client_msg_id = %entry.client_msg_id,
                    "reliable queue: drop stale pending entry"
                );
                let _ = st.pending_writer.pop(&entry.client_msg_id).await;
                st.retry_count.remove(&entry.client_msg_id);
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
                continue;
            }
            PendingDispatchDecision::SendNow => {}
        }
        if let Some(ack) = st.pending_acks.remove(&entry.client_msg_id) {
            apply_ack_and_publish(st, &entry, ack).await;
            continue;
        }
        if let Err(e) = do_send_one(&st.sender, &entry).await {
            match MessageDeliveryService::decide_send_attempt_failure(retries, st.max_retries) {
                RetryDecision::Retry { next_retry_count } => {
                    st.retry_count
                        .insert(entry.client_msg_id.clone(), next_retry_count);
                    warn!(
                        %e,
                        client_msg_id = %entry.client_msg_id,
                        retries = next_retry_count,
                        "send attempt failed, keep entry in pending queue for retry"
                    );
                    st.in_flight = Some((entry, Instant::now() + st.timeout_duration));
                    return Ok(());
                }
                RetryDecision::Fail { reason } => {
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
                    continue;
                }
            }
        }
        st.in_flight = Some((entry, Instant::now() + st.timeout_duration));
        return Ok(());
    }
}

async fn do_send_one(sender: &PacketSender, entry: &PendingSendVo) -> Result<()> {
    let proto = entry.message.to_proto();
    sender.send_message(&proto, Duration::from_secs(15)).await
}

async fn apply_ack_and_publish(st: &mut QueueState, entry: &PendingSendVo, ack: SendAck) {
    let _ = st.pending_writer.pop(&ack.client_msg_id).await;
    st.retry_count.remove(&ack.client_msg_id);
    if ack.success {
        let msg = MessageDeliveryService::mark_sent_from_ack(&entry.message, &ack);
        let cid = ack.client_msg_id.clone();
        if let Err(e) = st.message_store.update_after_ack(&cid, &msg).await {
            warn!(%e, "update_after_ack failed");
        }
        st.bus
            .publish(SdkEvent::Message(MessageEvent::SendAck { ack }));
        return;
    }
    let failed_msg = MessageDeliveryService::mark_failed(&entry.message);
    if let Err(e) = st.message_store.save_batch(&[failed_msg]).await {
        warn!(%e, "persist failed message state from ack failed");
    }
    let reason = if ack.error_message.trim().is_empty() {
        "send ack reported failure".to_string()
    } else {
        ack.error_message.clone()
    };
    let client_msg_id = ack.client_msg_id.clone();
    st.bus
        .publish(SdkEvent::Message(MessageEvent::SendAck { ack }));
    st.bus.publish(SdkEvent::Message(MessageEvent::SendFailed {
        client_msg_id,
        reason,
    }));
}

#[cfg(all(test, feature = "storage-sqlite"))]
mod tests {
    use super::ReliableSendQueue;
    use crate::core::CurrentUserIdStore;
    use crate::domain::{
        MessageReader, MessageWriter, PendingSendReader, PendingSendVo, PendingSendWriter,
    };
    use crate::event::{EventBus, MessageEvent, SdkEvent};
    use crate::infrastructure::persistence::sqlite::{
        SqliteMessageRepo, SqlitePendingSendRepo, init_schema as sqlite_init_schema,
    };
    use crate::model::IMMessage;
    use crate::protocol::{Codec, PacketSender, ProtobufCodec};
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
        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new("u1".to_string()));
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite_init_schema(&pool).await.unwrap();
        let pending_store = Arc::new(SqlitePendingSendRepo::new(pool.clone()));
        let message_store = Arc::new(SqliteMessageRepo::new(pool));
        let queue = ReliableSendQueue::new(
            pending_store.clone(),
            pending_store,
            dummy_sender(),
            message_store.clone(),
            current_user_id,
            bus,
            Some(60),
            Some(3),
        );
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
        let message_store = Arc::new(SqliteMessageRepo::new(pool));
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
        let queue = ReliableSendQueue::new(
            pending_store.clone(),
            pending_store.clone(),
            dummy_sender(),
            message_store,
            current_user_id,
            bus.clone(),
            Some(60),
            Some(3),
        );

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
        let message_store = Arc::new(SqliteMessageRepo::new(pool));
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "orphan-1".to_string();
        message.client_msg_id = "orphan-1".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();
        message.local_state.sending = true;
        message.local_state.is_local = true;
        message_store.save_batch(&[message]).await.unwrap();
        let queue = ReliableSendQueue::new(
            pending_store.clone(),
            pending_store,
            dummy_sender(),
            message_store,
            current_user_id,
            bus.clone(),
            Some(60),
            Some(3),
        );

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
