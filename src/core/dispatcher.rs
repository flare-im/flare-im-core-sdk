//! Router：下行载荷 → EventBus。与 flare-proto 对齐：消息=MessagePush，事件=Event/EventEnvelope，回执=SendAck；同步=`DataPacket.sync_response`→`SyncRes`，扩展=`DataPacket.user_custom`/`CustomData`。

use std::collections::HashMap;
use std::sync::Arc;

use flare_proto::common::MessageDeleteEvent;
use flare_proto::common::MessageStatus;
use flare_proto::common::event::Payload as EventPayload;
use prost::Message;
use tokio::sync::{Mutex as AsyncMutex, RwLock};
use tracing::warn;

use crate::application::notification::{
    NotificationInboundPipeline, partition_notification_durability,
};
use crate::application::projections::ConversationProjectionApplier;
use crate::application::services::EventDeduper;
use crate::application::services::IncomingMessageConverger;
use crate::application::services::MessageDeduper;
use crate::core::ReliableSendQueue;
use crate::core::event::{ConversationEvent, EventBus, ExtensionEvent, MessageEvent, SdkEvent};
use crate::core::{SessionSyncRunner, SyncResponseHandler};
use crate::domain::{DEFAULT_SYNC_LIMIT, SyncCursorVo};
use crate::infrastructure::persistence::StoreProvider;
use crate::infrastructure::protocol::DownlinkPayload;
use crate::model::IMMessage;
use crate::shared::util::spawn_background;

const SEQ_REPAIR_BASE_BACKOFF_MS: u64 = 1_000;
const SEQ_REPAIR_MAX_BACKOFF_MS: u64 = 60_000;

#[derive(Debug, Clone, Default)]
struct SeqRepairState {
    in_flight: bool,
    last_gap_after: u64,
    last_progress_seq: u64,
    attempts: u32,
    next_attempt_at_ms: u64,
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
    message_deduper: MessageDeduper,
    notification_pipeline: NotificationInboundPipeline,
    incoming_message_converger: Option<IncomingMessageConverger>,
    conversation_projection_applier: Option<ConversationProjectionApplier>,
    seq_repair_state: Arc<AsyncMutex<HashMap<String, SeqRepairState>>>,
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
        message_deduper: MessageDeduper,
        notification_pipeline: NotificationInboundPipeline,
    ) -> Self {
        let incoming_message_converger = stores.as_ref().map(|provider| {
            IncomingMessageConverger::new(
                provider.messages.clone(),
                bus.clone(),
                reliable_queue.clone(),
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
            message_deduper,
            notification_pipeline,
            incoming_message_converger,
            conversation_projection_applier,
            seq_repair_state: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }

    pub fn bus(&self) -> &EventBus {
        &self.bus
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
                    .map(|message| message.seq)
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
            if message.conversation_id.trim().is_empty() || message.seq == 0 {
                continue;
            }
            by_conversation
                .entry(message.conversation_id.clone())
                .or_default()
                .push(message.seq);
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
                    let state = guard.entry(conversation_id.clone()).or_default();
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
                        if let Err(error) = sync
                            .request_message_sync_from_seq(
                                &conversation_id,
                                first_gap_after,
                                DEFAULT_SYNC_LIMIT,
                            )
                            .await
                        {
                            warn!(
                                conversation_id = %conversation_id,
                                first_gap_after,
                                error = %error,
                                "实时消息缺口补拉失败"
                            );
                        }
                        if let Some(state) = repair_state.lock().await.get_mut(&conversation_id) {
                            state.in_flight = false;
                        }
                    });
                } else {
                    warn!(
                        conversation_id = %conversation_id,
                        first_gap_after,
                        "实时消息出现 seq 缺口，但 SDK 未配置单会话同步器，无法自动补拉"
                    );
                }
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

    pub async fn dispatch(
        &self,
        payload: DownlinkPayload,
    ) -> flare_core::common::error::Result<()> {
        match payload {
            DownlinkPayload::MessagePush(push) => {
                let mut all = Vec::new();
                all.extend(push.messages.clone());
                all.extend(push.notifications.clone());
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
                    let mut durable = true;
                    if !durable_messages.is_empty()
                        && let Some(ref stores) = self.stores
                    {
                        if let Err(e) = stores.messages.save_batch(&durable_messages).await {
                            warn!(error = %e, "MessagePush save_batch failed");
                            durable = false;
                        } else if let Some(applier) = &self.conversation_projection_applier
                            && let Err(e) = applier
                                .apply_messages(&durable_messages, &current_user_id)
                                .await
                        {
                            warn!(error = %e, "MessagePush conversation projection failed");
                            durable = false;
                        }
                        if durable {
                            self.repair_message_seq_after_persist(&durable_messages)
                                .await;
                        }
                    }
                    let mut inbound = if durable {
                        durable_messages
                    } else {
                        Vec::new()
                    };
                    inbound.extend(ephemeral_messages);
                    if !inbound.is_empty() {
                        self.notification_pipeline.finish_batch(inbound).await;
                    }
                }
            }
            DownlinkPayload::Event(ev) => {
                self.dispatch_single_event(&ev).await;
            }
            DownlinkPayload::EventEnvelope(env) => {
                tracing::debug!(
                    event_count = env.events.len(),
                    "dispatch EventEnvelope (push/sync)"
                );
                for ev in &env.events {
                    self.dispatch_single_event(ev).await;
                }
            }
            DownlinkPayload::SendAck(ack) => {
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
                self.bus.publish(SdkEvent::Extension(ExtensionEvent {
                    source: "custom".to_string(),
                    event_type: data.r#type.clone(),
                    payload: data.payload.clone(),
                }));
            }
            DownlinkPayload::SyncResp(resp) => {
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
        let mut messages: Vec<IMMessage> = Vec::new();
        if let Some(EventPayload::Message(m)) = &ev.payload {
            messages.push(IMMessage::new(m.clone()));
        }
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
        if !messages.is_empty() {
            if let Some(ref stores) = self.stores {
                if let Err(e) = stores.messages.save_batch(&messages).await {
                    warn!(error = %e, "single event message save_batch failed");
                    self.event_deduper.forget(ev).await;
                    return;
                } else if let Some(applier) = &self.conversation_projection_applier {
                    let current_user_id = self.current_user_id.read().await.clone();
                    if let Err(e) = applier.apply_messages(&messages, &current_user_id).await {
                        warn!(error = %e, "single event conversation projection failed");
                        self.event_deduper.forget(ev).await;
                        return;
                    }
                }
                self.repair_message_seq_after_persist(&messages).await;
            }
            for imm in messages {
                if self.message_deduper.record_if_new(&imm).await {
                    self.bus.publish(SdkEvent::Message(MessageEvent::Received {
                        message: Box::new(imm),
                    }));
                }
            }
        }
        let conv_id = ev.conversation_id.clone();
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
                    self.bus.publish(SdkEvent::Message(MessageEvent::Recalled {
                        conversation_id: conv_id,
                        event: recall.clone(),
                    }));
                }
                EventPayload::Edit(edit) => {
                    let mut should_publish = true;
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_edit_event(
                                &edit.server_msg_id,
                                edit.new_content.clone(),
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
                            conversation_id: conv_id.clone(),
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
                                &conv_id,
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
                                conversation_id: conv_id,
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
                                conversation_id: conv_id.clone(),
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
                                    &conv_id,
                                    &current_user_id,
                                    read.read_seq,
                                )
                                .await;
                        }
                    }
                    self.bus
                        .publish(SdkEvent::Message(MessageEvent::ReadReceipt {
                            conversation_id: conv_id,
                            event: read.clone(),
                        }));
                }
                EventPayload::BurnScheduled(burn_scheduled) => {
                    let mut should_publish = true;
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_burn_scheduled_event(
                                &burn_scheduled.message_id,
                                burn_scheduled.burn_at,
                                burn_scheduled.event_time,
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
                                    message_id = %burn_scheduled.message_id,
                                    "Event BurnScheduled: no local row matched; event will be retried after message sync"
                                );
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                            Err(error) => {
                                warn!(error = %error, message_id = %burn_scheduled.message_id, "Event BurnScheduled apply failed");
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                        }
                    }
                    if should_publish {
                        self.bus
                            .publish(SdkEvent::Message(MessageEvent::BurnScheduled {
                                conversation_id: conv_id,
                                event: burn_scheduled.clone(),
                            }));
                    }
                }
                EventPayload::Burned(burned) => {
                    let mut should_publish = true;
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_burned_event(
                                &burned.message_id,
                                burned.burn_at,
                                burned.burned_at,
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
                                    message_id = %burned.message_id,
                                    "Event Burned: no local row matched; event will be retried after message sync"
                                );
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                            Err(error) => {
                                warn!(error = %error, message_id = %burned.message_id, "Event Burned apply failed");
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                        }
                    }
                    if should_publish {
                        self.bus.publish(SdkEvent::Message(MessageEvent::Burned {
                            conversation_id: conv_id,
                            event: burned.clone(),
                        }));
                    }
                }
                EventPayload::HardDeleted(hard_deleted) => {
                    let mut should_publish = true;
                    if let Some(ref stores) = self.stores {
                        match stores
                            .messages
                            .apply_hard_deleted_event(
                                &hard_deleted.message_id,
                                hard_deleted.burn_at,
                                hard_deleted.burned_at,
                                hard_deleted.event_time,
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
                                    message_id = %hard_deleted.message_id,
                                    "Event HardDeleted: no local row matched; event will be retried after message sync"
                                );
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                            Err(error) => {
                                warn!(error = %error, message_id = %hard_deleted.message_id, "Event HardDeleted apply failed");
                                self.event_deduper.forget(ev).await;
                                return;
                            }
                        }
                    }
                    if should_publish {
                        self.bus
                            .publish(SdkEvent::Message(MessageEvent::HardDeleted {
                                conversation_id: conv_id,
                                event: hard_deleted.clone(),
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
                            conversation_id: conv_id,
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
                            conversation_id: conv_id,
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
                            conversation_id: conv_id,
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
                            conversation_id: conv_id,
                            event: unmark.clone(),
                        }));
                    }
                }
                EventPayload::Presence(presence) => {
                    self.bus
                        .publish(SdkEvent::Message(MessageEvent::PresenceChanged {
                            conversation_id: conv_id,
                            event: presence.clone(),
                        }));
                }
                EventPayload::CallSignal(call) => {
                    self.bus
                        .publish(SdkEvent::Message(MessageEvent::CallSignal {
                            conversation_id: conv_id,
                            event: Box::new(call.clone()),
                        }));
                }
                EventPayload::Custom(custom) => {
                    self.bus.publish(SdkEvent::Message(MessageEvent::Custom {
                        conversation_id: conv_id,
                        event: custom.clone(),
                    }));
                }
                EventPayload::Typing(typing) => {
                    self.bus.publish(SdkEvent::Message(MessageEvent::Typing {
                        conversation_id: conv_id,
                        event: typing.clone(),
                    }));
                }
                EventPayload::Conversation(_) => {
                    self.bus
                        .publish(SdkEvent::Conversation(ConversationEvent::Updated {
                            conversation_id: conv_id,
                        }));
                }
                EventPayload::ConversationDelete(_) => {
                    if let Some(stores) = self.stores.as_ref()
                        && let Err(error) = stores.conversations.delete(&conv_id).await
                    {
                        warn!(
                            conversation_id = %conv_id,
                            error = %error,
                            "ConversationDelete push: local conversation purge failed"
                        );
                    }
                    self.bus
                        .publish(SdkEvent::Conversation(ConversationEvent::Deleted {
                            conversation_id: conv_id,
                        }));
                }
                _ => {}
            }
        }
    }
}

fn operation_seq(event: &flare_proto::common::Event) -> Option<u64> {
    event
        .event_seq
        .or(if event.seq > 0 { Some(event.seq) } else { None })
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

fn now_ms() -> u64 {
    crate::shared::util::now_millis()
}

#[cfg(test)]
mod tests {
    use super::{
        Dispatcher, first_gap_after, first_internal_gap_after, first_internal_gap_after_from,
        max_contiguous_seq,
    };
    use super::{SEQ_REPAIR_MAX_BACKOFF_MS, seq_repair_backoff_ms};
    use crate::application::notification::{
        NotificationHandlerRegistry, NotificationInboundPipeline,
    };
    use crate::application::services::EventDeduper;
    use crate::application::services::MessageDeduper;
    use crate::application::usecases::SyncApplyUseCase;
    use crate::core::CurrentUserIdStore;
    use crate::core::event::{EventBus, MessageEvent, SdkEvent};
    use crate::core::{ReliableSendQueue, ReliableSendQueueConfig};
    use crate::domain::{
        ConversationReader, ConversationWriter, MessageReader, MessageStore, MessageWriter,
        PendingSendReader, PendingSendVo, PendingSendWriter, SyncCursorReader, SyncCursorVo,
        SyncCursorWriter,
    };
    use crate::infrastructure::persistence::StoreProvider;
    use crate::infrastructure::protocol::DownlinkPayload;
    use crate::infrastructure::protocol::{Codec, PacketSender, ProtobufCodec};
    use crate::model::IMMessage;
    use crate::shared::error::Result;
    use async_trait::async_trait;
    use flare_proto::common::{MessageDeleteEvent, SendAck};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::{Mutex, RwLock};
    use tokio::time::{Duration, timeout};

    fn test_notification_pipeline(bus: EventBus) -> NotificationInboundPipeline {
        NotificationInboundPipeline::new(
            Arc::new(NotificationHandlerRegistry::new()),
            MessageDeduper::new(Some(64)),
            bus,
        )
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
    }

    impl MemoryMessageStore {
        fn new() -> Self {
            Self {
                data: RwLock::new(HashMap::new()),
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
        }));

        let dispatcher = Dispatcher::new(
            bus.clone(),
            Some(reliable_queue.clone()),
            None,
            None,
            None,
            current_user_id,
            EventDeduper::new(Some(64)),
            MessageDeduper::new(Some(64)),
            test_notification_pipeline(bus.clone()),
        );

        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.client_msg_id = "client-1".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();
        reliable_queue.enqueue(message).await.unwrap();

        tokio::time::sleep(Duration::from_millis(20)).await;

        dispatcher
            .dispatch(DownlinkPayload::SendAck(SendAck {
                client_msg_id: "client-1".to_string(),
                server_msg_id: "server-1".to_string(),
                seq: 1,
                conversation_id: "conv-1".to_string(),
                success: true,
                ..Default::default()
            }))
            .await
            .unwrap();

        let first = timeout(Duration::from_millis(200), receiver.recv())
            .await
            .expect("expected one send ack event")
            .expect("bus closed");

        match first {
            SdkEvent::Message(MessageEvent::SendAck { ack }) => {
                assert_eq!(ack.client_msg_id, "client-1");
                assert_eq!(ack.server_msg_id, "server-1");
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
        }));

        let dispatcher = Dispatcher::new(
            bus.clone(),
            Some(reliable_queue.clone()),
            None,
            None,
            Some(stores),
            current_user_id,
            EventDeduper::new(Some(64)),
            MessageDeduper::new(Some(64)),
            test_notification_pipeline(bus.clone()),
        );

        let mut optimistic = IMMessage::new(flare_proto::common::Message::default());
        optimistic.client_msg_id = "client-self-1".to_string();
        optimistic.conversation_id = "conv-1".to_string();
        optimistic.sender_id = "u1".to_string();
        reliable_queue.enqueue(optimistic).await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;

        let proto_message = flare_proto::common::Message {
            server_id: "server-self-1".to_string(),
            client_msg_id: "client-self-1".to_string(),
            conversation_id: "conv-1".to_string(),
            sender_id: "u1".to_string(),
            seq: 11,
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
                assert_eq!(ack.server_msg_id, "server-self-1");
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
        }));
        let dispatcher = Dispatcher::new(
            bus.clone(),
            Some(reliable_queue.clone()),
            None,
            None,
            None,
            current_user_id,
            EventDeduper::new(Some(64)),
            MessageDeduper::new(Some(64)),
            test_notification_pipeline(bus.clone()),
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

        tokio::time::sleep(Duration::from_millis(20)).await;

        dispatcher
            .dispatch(DownlinkPayload::SendAck(SendAck {
                client_msg_id: "client-2".to_string(),
                server_msg_id: "server-2".to_string(),
                seq: 2,
                conversation_id: "conv-1".to_string(),
                success: true,
                ..Default::default()
            }))
            .await
            .unwrap();

        dispatcher
            .dispatch(DownlinkPayload::SendAck(SendAck {
                client_msg_id: "client-1".to_string(),
                server_msg_id: "server-1".to_string(),
                seq: 1,
                conversation_id: "conv-1".to_string(),
                success: true,
                ..Default::default()
            }))
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
            MessageDeduper::new(Some(64)),
            test_notification_pipeline(bus.clone()),
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
        let dispatcher = Dispatcher::new(
            bus.clone(),
            None,
            None,
            None,
            Some(stores.clone()),
            current_user_id,
            deduper.clone(),
            message_deduper.clone(),
            test_notification_pipeline(bus.clone()),
        );
        let sync_apply = SyncApplyUseCase::new(
            stores,
            bus.clone(),
            deduper,
            test_notification_pipeline(bus.clone()),
        );

        let proto_message = flare_proto::common::Message {
            server_id: "server-msg-1".to_string(),
            client_msg_id: "client-msg-1".to_string(),
            conversation_id: "conv-1".to_string(),
            sender_id: "u2".to_string(),
            seq: 10,
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
                        seq: 10,
                        created_at: None,
                        payload: prost::Message::encode_to_vec(&proto_message),
                        kind: flare_proto::common::SyncSliceItemKind::Message as i32,
                        skip_reason: String::new(),
                    }],
                    max_seq: 10,
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

    #[test]
    fn seq_helpers_ignore_duplicates_and_find_contiguous_tail() {
        assert_eq!(max_contiguous_seq(10, &[12, 11, 11, 13]), 13);
        assert_eq!(max_contiguous_seq(10, &[12, 13]), 10);
        assert_eq!(first_gap_after(10, &[11, 13, 14]), Some(11));
        assert_eq!(first_gap_after(10, &[11, 12, 13]), None);
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
}
