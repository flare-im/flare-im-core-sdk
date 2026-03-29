//! 可靠队列 Actor — 单任务循环：enqueue / ack / timeout → 转移

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use flare_proto::common::SendAck;
use tokio::sync::mpsc;
use tokio::time::{Instant, interval_at};
use tracing::warn;

use crate::domain::{PendingSendReader, PendingSendVo, PendingSendWriter};
use crate::error::{ErrorCode, FlareError, Result};
use crate::event::{EventBus, MessageEvent, SdkEvent};
use crate::fsm::{MessageStateEvent, MessageStateFsm};
use crate::model::IMMessage;
use crate::model::message::MessageLocalState;
use crate::protocol::PacketSender;
use crate::store::MessageStore;
use crate::util::id;
use crate::util::{RELIABLE_QUEUE_MAX_RETRIES, RELIABLE_QUEUE_TIMEOUT_SECS};

/// 队列命令（仅通过此与队列通信）
#[derive(Debug)]
pub enum QueueCommand {
    /// 入队发送（SDK 内部统一 IMMessage）
    Enqueue(IMMessage),
    /// 收到 SendAck（由外部收到下行后注入）
    AckReceived(SendAck),
}

/// 可靠发送队列句柄（发命令 + 持有 actor 所需依赖）
pub struct ReliableSendQueue {
    tx: mpsc::Sender<QueueCommand>,
    _worker: tokio::task::JoinHandle<()>,
}

struct QueueState {
    pending_reader: Arc<dyn PendingSendReader>,
    pending_writer: Arc<dyn PendingSendWriter>,
    sender: Arc<PacketSender>,
    message_store: Arc<dyn MessageStore>,
    bus: EventBus,
    timeout_duration: Duration,
    max_retries: u32,
    /// 当前在途消息（仅一条，严格顺序）
    in_flight: Option<(PendingSendVo, Instant)>,
    /// client_msg_id -> 已重试次数
    retry_count: HashMap<String, u32>,
}

impl ReliableSendQueue {
    /// 构建队列并启动后台任务；收到 ack 需由调用方往 `on_ack` 注入。
    /// reader 与 writer 通常为同一实现体（如 SqlitePendingSendRepo）的两个 Arc。
    pub fn new(
        pending_reader: Arc<dyn PendingSendReader>,
        pending_writer: Arc<dyn PendingSendWriter>,
        sender: Arc<PacketSender>,
        message_store: Arc<dyn MessageStore>,
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
            bus,
            timeout_duration,
            max_retries,
            in_flight: None,
            retry_count: HashMap::new(),
        }));

        let state_clone = state.clone();
        let _worker = tokio::spawn(async move {
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

        Self { tx, _worker }
    }

    /// 入队发送（持久化后由 worker 按序发送，SDK 内部仅 IMMessage）
    pub async fn enqueue(&self, message: IMMessage) -> Result<()> {
        self.tx
            .send(QueueCommand::Enqueue(message))
            .await
            .map_err(|_| FlareError::localized(ErrorCode::InternalError, "reliable queue closed"))
    }

    /// 将收到的 SendAck 注入队列（由 Dispatcher 或 Engine 在收到下行 ack 时调用）
    pub async fn on_ack(&self, ack: SendAck) -> Result<()> {
        self.tx
            .send(QueueCommand::AckReceived(ack))
            .await
            .map_err(|_| FlareError::localized(ErrorCode::InternalError, "reliable queue closed"))
    }
}

async fn handle_command(
    state: Arc<tokio::sync::Mutex<QueueState>>,
    cmd: QueueCommand,
) -> Result<()> {
    let mut st = state.lock().await;
    match cmd {
        QueueCommand::Enqueue(msg) => {
            let enqueued_at_ms = id::now_millis();
            let mut optimistic = msg.clone();
            optimistic.server_id = msg.client_msg_id.clone();
            optimistic.local_state = MessageLocalState {
                sending: true,
                failed: false,
                is_local: true,
                sort_ts: enqueued_at_ms,
            };
            if let Err(e) = st.message_store.save_batch(&[optimistic]).await {
                warn!(%e, "save optimistic message on enqueue failed");
            }
            let entry = PendingSendVo {
                client_msg_id: msg.client_msg_id.clone(),
                conversation_id: msg.conversation_id.clone(),
                message: msg,
                enqueued_at_ms,
            };
            st.pending_writer.push(entry).await?;
            try_send_next(&mut st).await?;
        }
        QueueCommand::AckReceived(ack) => {
            if let Some((entry, _)) = st.in_flight.take() {
                if entry.client_msg_id == ack.client_msg_id {
                    let _ = st.pending_writer.pop(&ack.client_msg_id).await;
                    let mut msg = entry.message;
                    if !ack.server_msg_id.is_empty() {
                        msg.server_id = ack.server_msg_id.clone();
                    }
                    msg.seq = ack.seq;
                    let current = MessageStateFsm::from_local_state(
                        msg.local_state.sending,
                        msg.local_state.failed,
                        msg.local_state.is_local,
                    );
                    if let Ok(sent) =
                        MessageStateFsm::transition(current, &MessageStateEvent::SendAckReceived)
                    {
                        let (sending, failed, is_local) =
                            MessageStateFsm::to_local_state_flags(sent);
                        msg.local_state = MessageLocalState {
                            sending,
                            failed,
                            is_local,
                            sort_ts: msg.local_state.sort_ts,
                        };
                    }
                    let cid = ack.client_msg_id.clone();
                    if let Err(e) = st.message_store.update_after_ack(&cid, &msg).await {
                        warn!(%e, "update_after_ack failed");
                    }
                    st.bus
                        .publish(SdkEvent::Message(MessageEvent::SendAck { ack }));
                } else {
                    st.in_flight = Some((entry, Instant::now()));
                }
            }
            try_send_next(&mut st).await?;
        }
    }
    Ok(())
}

async fn check_timeout(state: Arc<tokio::sync::Mutex<QueueState>>) -> Result<()> {
    let mut st = state.lock().await;
    let now = Instant::now();
    if let Some((entry, deadline)) = st.in_flight.take() {
        if now.duration_since(deadline) >= st.timeout_duration {
            let retries = st
                .retry_count
                .get(&entry.client_msg_id)
                .copied()
                .unwrap_or(0);
            if retries >= st.max_retries {
                let _ = st.pending_writer.pop(&entry.client_msg_id).await;
                st.retry_count.remove(&entry.client_msg_id);
                let mut failed_msg = entry.message.clone();
                failed_msg.server_id = entry.client_msg_id.clone();
                failed_msg.local_state = MessageLocalState {
                    sending: false,
                    failed: true,
                    is_local: true,
                    sort_ts: failed_msg.local_state.sort_ts,
                };
                if let Err(e) = st.message_store.save_batch(&[failed_msg]).await {
                    warn!(%e, "persist failed message state failed");
                }
                st.bus.publish(SdkEvent::Message(MessageEvent::SendFailed {
                    client_msg_id: entry.client_msg_id,
                    reason: "timeout after retries".to_string(),
                }));
            } else {
                st.retry_count
                    .insert(entry.client_msg_id.clone(), retries + 1);
                if let Err(e) = do_send_one(&st.sender, &entry).await {
                    warn!(%e, "retry send failed");
                }
                st.in_flight = Some((entry, now + st.timeout_duration));
                return Ok(());
            }
        } else {
            st.in_flight = Some((entry, deadline));
        }
    }
    try_send_next(&mut st).await?;
    Ok(())
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
        let retries = st
            .retry_count
            .get(&entry.client_msg_id)
            .copied()
            .unwrap_or(0);
        if retries >= st.max_retries {
            let _ = st.pending_writer.pop(&entry.client_msg_id).await;
            st.retry_count.remove(&entry.client_msg_id);
            let mut failed_msg = entry.message.clone();
            failed_msg.server_id = entry.client_msg_id.clone();
            failed_msg.local_state = MessageLocalState {
                sending: false,
                failed: true,
                is_local: true,
                sort_ts: failed_msg.local_state.sort_ts,
            };
            if let Err(e) = st.message_store.save_batch(&[failed_msg]).await {
                warn!(%e, "persist failed message state failed");
            }
            st.bus.publish(SdkEvent::Message(MessageEvent::SendFailed {
                client_msg_id: entry.client_msg_id.clone(),
                reason: "max retries exceeded".to_string(),
            }));
            continue;
        }
        let _ = st.pending_writer.pop(&entry.client_msg_id).await;
        do_send_one(&st.sender, &entry).await?;
        st.in_flight = Some((entry, Instant::now() + st.timeout_duration));
        return Ok(());
    }
}

async fn do_send_one(sender: &PacketSender, entry: &PendingSendVo) -> Result<()> {
    let proto = entry.message.to_proto();
    sender.send_message(&proto, Duration::from_secs(15)).await
}
