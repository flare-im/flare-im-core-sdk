//! 同步协议处理：会话列表、单会话消息、已读上报及响应落库。
//! 与 flare-proto 对齐：上行 Ack(ReadAck)、Sync；下行 SyncRes。

use std::collections::HashSet;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use futures::stream::{self, StreamExt};

use flare_proto::common::{
    Ack, AckType, ConversationParticipantsSync, GetSyncCursorSync, MultiDeviceCursor,
    QueryEventsSync, ReadAck, SyncKind, SyncRes, UpdateConversationUserSettingsSync,
    UpdateSyncCursorSync, ack::Payload as AckPayload, sync_res::Payload as SyncResPayload,
};
use tokio::sync::Mutex as AsyncMutex;

use crate::application::notification::NotificationInboundPipeline;
use crate::application::services::EventDeduper;
use crate::application::usecases::sync_request::SyncRequestUseCase;
use crate::application::usecases::{SyncApplyUseCase, local_message_sync_start_seq};
use crate::core::ConversationSummarySync;
use crate::core::event::SyncNotify;
use crate::core::event::{EventBus, SdkEvent};
use crate::core::{SessionSyncRunner, SyncResponseHandler, SyncRunContext, SyncTrigger};
use crate::core::{SyncFsm, SyncState, SyncTransition};
use crate::domain::{
    CONVERSATION_CURSOR_KEY, CRITICAL_EVENT_CURSOR_KEY, DEFAULT_SYNC_LIMIT, ReadPosition,
    SyncPolicy,
};
use crate::infrastructure::persistence::StoreProvider;
use crate::infrastructure::protocol::PacketSender;
use crate::shared::error::{FlareError, Result};
use crate::shared::util::date::{ms_to_prost_timestamp, system_time_to_prost_timestamp};

#[derive(Debug, Clone, Default)]
struct QueryEventsReqV1 {
    conversation_id: String,
    after_seq: i64,
    before_seq: i64,
    limit: i32,
    event_types: Vec<i32>,
    include_deleted: bool,
}

fn build_read_ack(conversation_id: &str, read_seq: u64) -> Ack {
    let ack_id = format!("read:{}:{}", conversation_id, read_seq);
    Ack {
        r#type: AckType::Read as i32,
        ack_id: Some(ack_id.clone()),
        at: Some(system_time_to_prost_timestamp()),
        payload: Some(AckPayload::Read(ReadAck {
            conversation_id: conversation_id.to_string(),
            read_seq,
            device_id: None,
            ack_id: Some(ack_id),
        })),
    }
}

pub struct SyncProtocolAdapter {
    sender: Arc<PacketSender>,
    stores: StoreProvider,
    bus: EventBus,
    sync_state: Mutex<SyncState>,
    active_user_id: AsyncMutex<String>,
    sync_apply_use_case: SyncApplyUseCase,
    sync_request_use_case: SyncRequestUseCase,
    /// Init/重连：会话列表同步后按会话补拉消息的并发上限。
    init_message_sync_concurrency: usize,
}

impl SyncProtocolAdapter {
    async fn get_remote_cursor_seq(
        &self,
        _user_id: &str,
        conversation_id: &str,
    ) -> Result<Option<u64>> {
        let resp = self
            .sync_request_use_case
            .request_get_cursor(GetSyncCursorSync {
                device_id: String::new(),
                conversation_id: conversation_id.to_string(),
            })
            .await?;
        let Some(SyncResPayload::GetSyncCursor(res)) = resp.payload else {
            return Ok(None);
        };
        // 编排器 GetSyncCursor 在缓存未命中时返回 cursor=None。若当成 0 再与本地取 max，
        // 会误用本地 SQLite 里陈旧的毫秒游标做增量过滤，导致会话列表永远为空（conversation 侧 bootstrap 实际有数据）。
        let Some(parsed) = res.cursor else {
            return Ok(None);
        };
        Ok(Some(parsed.last_sync_seq))
    }

    async fn update_remote_cursor_seq(
        &self,
        user_id: &str,
        conversation_id: &str,
        last_seq: u64,
    ) -> Result<()> {
        let remote = self
            .get_remote_cursor_seq(user_id, conversation_id)
            .await
            .ok()
            .flatten()
            .unwrap_or(0);
        let effective = last_seq.max(remote);
        if effective <= remote {
            return Ok(());
        }
        let local_read_seq = self
            .stores
            .conversations
            .get(conversation_id)
            .await
            .ok()
            .flatten()
            .and_then(|conversation| ReadPosition::from_conversation(&conversation).ack_read_seq())
            .unwrap_or(0);
        let resp = self
            .sync_request_use_case
            .request_update_cursor(UpdateSyncCursorSync {
                cursor: Some(MultiDeviceCursor {
                    device_id: String::new(),
                    conversation_id: conversation_id.to_string(),
                    last_sync_seq: effective,
                    last_sync_at: Some(system_time_to_prost_timestamp()),
                    last_read_seq: local_read_seq,
                    last_critical_event_seq: 0,
                }),
            })
            .await?;
        let Some(SyncResPayload::UpdateSyncCursor(res)) = resp.payload else {
            return Ok(());
        };
        let _ = res.cursor;
        Ok(())
    }

    pub(crate) fn new(
        sender: Arc<PacketSender>,
        stores: StoreProvider,
        bus: EventBus,
        event_deduper: EventDeduper,
        notification_pipeline: NotificationInboundPipeline,
        init_message_sync_concurrency: usize,
    ) -> Self {
        let sync_apply_use_case = SyncApplyUseCase::new(
            stores.clone(),
            bus.clone(),
            event_deduper,
            notification_pipeline,
        );
        let sync_request_use_case = SyncRequestUseCase::new(sender.clone());
        Self {
            sender,
            stores,
            bus,
            sync_state: Mutex::new(SyncState::Idle),
            active_user_id: AsyncMutex::new(String::new()),
            sync_apply_use_case,
            sync_request_use_case,
            init_message_sync_concurrency: init_message_sync_concurrency.max(1),
        }
    }

    pub fn set_reliable_queue(&self, reliable_queue: Option<Arc<crate::core::ReliableSendQueue>>) {
        self.sync_apply_use_case.set_reliable_queue(reliable_queue);
    }

    async fn current_user_id(&self) -> String {
        self.active_user_id.lock().await.clone()
    }

    async fn save_cursor_with_remote(
        &self,
        user_id: &str,
        conversation_id: &str,
        last_seq: u64,
    ) -> Result<()> {
        self.sync_apply_use_case
            .save_cursor_with_remote(
                user_id,
                conversation_id,
                last_seq,
                |user_id, conversation_id, last_seq| async move {
                    self.update_remote_cursor_seq(&user_id, &conversation_id, last_seq)
                        .await
                },
            )
            .await
    }

    fn transition_sync(&self, run: &SyncRunContext, event: SyncTransition) {
        let mut guard = match self.sync_state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::warn!("sync state lock poisoned, recovering poisoned state");
                poisoned.into_inner()
            }
        };
        if let Ok(next) = SyncFsm::transition(*guard, &event) {
            *guard = next;
            drop(guard);
            self.bus.publish(SdkEvent::Sync(SyncNotify::StateChanged {
                run: run.clone(),
                state: next,
            }));
        } else {
            tracing::debug!("sync transition ignored: invalid state transition");
        }
    }

    /// 已读上报（ack.proto Ack.payload.read = ReadAck）。
    pub async fn send_read_ack(&self, conversation_id: &str, read_seq: u64) -> Result<()> {
        self.send_read_ack_impl(conversation_id, read_seq).await
    }

    /// 将本地已读位点批量推送给服务端（登录前、后台 read_states 任务共用）。
    pub async fn push_local_read_states(
        &self,
        conversations: &[crate::model::Conversation],
    ) -> Result<usize> {
        let mut ack_count = 0usize;
        for conversation in conversations {
            let Some(read_seq) = ReadPosition::from_conversation(conversation).ack_read_seq()
            else {
                continue;
            };
            self.send_read_ack_impl(&conversation.conversation_id, read_seq)
                .await?;
            ack_count = ack_count.saturating_add(1);
        }
        if ack_count > 0 {
            tracing::debug!(ack_count, "pushed local read states to server");
        }
        Ok(ack_count)
    }

    /// 冷启动/重连拉摘要前，先把本地真实已读位点推给服务端。
    /// 确保 participants 表读位与客户端一致，摘要 `last_read_seq` 不再依赖反推。
    async fn push_local_read_states_before_summary_sync(&self) -> Result<usize> {
        let list = self.stores.conversations.list().await?;
        self.push_local_read_states(&list).await
    }

    async fn send_read_ack_impl(&self, conversation_id: &str, read_seq: u64) -> Result<()> {
        let ack = build_read_ack(conversation_id, read_seq);
        self.sender.send_ack(&ack).await
    }

    /// 上行同步当前用户的会话偏好（置顶/免打扰/归档/草稿）
    pub async fn push_conversation_user_settings(
        &self,
        conversation_id: &str,
        base_settings_version: u64,
        patch: crate::application::sync_task::ConversationUserSettingsPatch,
    ) -> Result<()> {
        let req = UpdateConversationUserSettingsSync {
            conversation_id: conversation_id.to_string(),
            is_pinned: patch.is_pinned,
            is_muted: patch.is_muted,
            is_archived: patch.is_archived,
            draft: patch.draft,
            base_settings_version,
        };
        let resp = self
            .sync_request_use_case
            .request_update_conversation_user_settings(req)
            .await?;
        self.apply_user_settings_response(conversation_id, &resp)
            .await
    }

    pub async fn push_conversation_user_settings_from_local(
        &self,
        conversation_id: &str,
        base_settings_version: u64,
        conversation: &crate::model::Conversation,
    ) -> Result<()> {
        self.push_conversation_user_settings(
            conversation_id,
            base_settings_version,
            crate::application::sync_task::ConversationUserSettingsPatch {
                is_pinned: Some(conversation.is_pinned),
                is_muted: Some(conversation.is_muted),
                is_archived: Some(conversation.is_archived),
                draft: conversation.draft.clone(),
            },
        )
        .await
    }

    async fn apply_user_settings_response(
        &self,
        conversation_id: &str,
        resp: &SyncRes,
    ) -> Result<()> {
        let Some(SyncResPayload::UpdateConversationUserSettingsRes(body)) = &resp.payload else {
            return Ok(());
        };
        let Some(settings) = &body.settings else {
            return Ok(());
        };
        let Some(mut conversation) = self.stores.conversations.get(conversation_id).await? else {
            return Ok(());
        };
        conversation.is_pinned = settings.is_pinned;
        conversation.is_muted = settings.is_muted;
        conversation.is_archived = settings.is_archived;
        conversation.draft = if settings.draft.trim().is_empty() {
            None
        } else {
            Some(settings.draft.clone())
        };
        crate::model::apply_remote_settings_version(&mut conversation, settings.settings_version);
        self.stores
            .conversations
            .save_batch(&[conversation])
            .await?;
        self.bus.publish(crate::core::event::SdkEvent::Conversation(
            crate::core::event::ConversationEvent::Updated {
                conversation_id: conversation_id.to_string(),
            },
        ));
        Ok(())
    }

    /// 将 SyncRes.payload.single_conversation 转为事件列表并落库、发布、更新游标
    async fn apply_sync_res_single(
        &self,
        run: &SyncRunContext,
        conversation_id: &str,
        resp: &SyncRes,
        requested_after_seq: u64,
    ) -> Result<(u64, bool, String)> {
        let sc = match &resp.payload {
            Some(SyncResPayload::SingleConversation(s)) => s,
            _ => {
                tracing::warn!(conversation_id = %conversation_id, "同步响应payload为空或类型不匹配");
                self.transition_sync(run, SyncTransition::SyncDone);
                return Ok((0, false, String::new()));
            }
        };

        tracing::debug!(
            conversation_id = %conversation_id,
            items_count = sc.items.len(),
            max_seq = sc.max_seq,
            has_more = sc.has_more,
            "收到消息同步响应"
        );

        let user_id = self.current_user_id().await;
        let cursor_seq = if user_id.is_empty() {
            0
        } else {
            self.stores
                .cursors
                .get_conversation_cursor(&user_id, conversation_id)
                .await?
                .map(|c| c.last_seq)
                .unwrap_or(0)
        };
        let cleared_floor = self
            .stores
            .conversations
            .get(conversation_id)
            .await?
            .map(|c| crate::domain::local_cleared_through_seq(&c.ext).max(c.visible_after_seq))
            .unwrap_or(0);
        let decode_after_seq = cursor_seq.min(requested_after_seq).max(cleared_floor);

        tracing::debug!(
            conversation_id = %conversation_id,
            cursor_seq = cursor_seq,
            requested_after_seq = requested_after_seq,
            cleared_floor = cleared_floor,
            decode_after_seq = decode_after_seq,
            "本地已知消息seq"
        );

        let applied = self
            .sync_apply_use_case
            .apply_single_conversation_page(conversation_id, &user_id, decode_after_seq, sc)
            .await?;
        if applied.has_decoded_items {
            self.transition_sync(run, SyncTransition::DataReceived);
        }
        if applied.has_seq_gap {
            tracing::warn!(
                conversation_id = %conversation_id,
                cursor_seq,
                requested_after_seq,
                decode_after_seq,
                safe_max_seq = applied.max_seq,
                remote_max_seq = applied.remote_max_seq,
                "消息同步未到达远端水位，保留 cursor 在连续位点并等待后续补偿"
            );
        }
        if applied.max_seq > cursor_seq && !user_id.is_empty() {
            self.save_cursor_with_remote(&user_id, conversation_id, applied.max_seq)
                .await?;
        }
        if applied.has_more {
            self.transition_sync(run, SyncTransition::BatchDone);
        } else {
            self.transition_sync(run, SyncTransition::SyncDone);
        }
        Ok((applied.max_seq, applied.has_more, applied.next_cursor))
    }

    async fn request_single_page(
        &self,
        run: &SyncRunContext,
        conversation_id: &str,
        last_seq: u64,
        limit: i32,
        cursor: String,
    ) -> Result<(u64, bool, String)> {
        tracing::debug!(
            conversation_id = %conversation_id,
            last_seq = last_seq,
            limit = limit,
            cursor = %cursor,
            "请求消息同步页面"
        );
        let mut retries = 0u8;
        loop {
            match self
                .sync_request_use_case
                .request_single_page(conversation_id, last_seq, limit, cursor.clone())
                .await
            {
                Ok(resp) => {
                    return self
                        .apply_sync_res_single(run, conversation_id, &resp, last_seq)
                        .await;
                }
                Err(e) => {
                    retries += 1;
                    tracing::warn!(
                        conversation_id = %conversation_id,
                        retry = retries,
                        error = %e,
                        "消息同步请求失败，准备重试"
                    );
                    if retries >= 3 {
                        tracing::error!(
                            conversation_id = %conversation_id,
                            "消息同步请求重试次数超过上限(3次)"
                        );
                        self.transition_sync(run, SyncTransition::SyncFailed);
                        return Err(FlareError::general_error(format!(
                            "sdk.sync.single_conversation.timeout_or_failed conversation_id={conversation_id}"
                        )));
                    }
                }
            }
        }
    }

    async fn request_query_events(&self, req: QueryEventsSync) -> Result<SyncRes> {
        self.sync_request_use_case.request_query_events(req).await
    }

    pub async fn sync_critical_events(&self) -> Result<()> {
        let user_id = self.current_user_id().await;
        if user_id.is_empty() {
            return Ok(());
        }
        let conversations = self.stores.conversations.list().await?;
        if conversations.is_empty() {
            return Ok(());
        }

        for conversation in conversations {
            let conversation_id = conversation.conversation_id;
            if conversation_id.trim().is_empty() {
                continue;
            }
            let cursor_key = critical_event_cursor_key(&conversation_id);
            let mut after_seq = self
                .stores
                .cursors
                .get_conversation_cursor(&user_id, &cursor_key)
                .await?
                .map(|c| c.last_seq as i64)
                .unwrap_or(0);

            loop {
                let req = QueryEventsReqV1 {
                    conversation_id: conversation_id.clone(),
                    after_seq,
                    before_seq: 0,
                    limit: 200,
                    event_types: SyncPolicy::critical_event_query_plan().event_types,
                    include_deleted: true,
                };
                let resp = match self
                    .request_query_events(QueryEventsSync {
                        conversation_id: req.conversation_id,
                        after_seq: req.after_seq,
                        before_seq: req.before_seq,
                        limit: req.limit,
                        event_types: req.event_types,
                        include_deleted: req.include_deleted,
                        replay_preset: 0,
                        client_last_applied_event_seq: 0,
                    })
                    .await
                {
                    Ok(resp) => resp,
                    Err(e) => {
                        tracing::warn!(
                            conversation_id = %conversation_id,
                            error = %e,
                            after_seq,
                            "关键事件回放请求失败，跳过该会话本轮后台补偿"
                        );
                        break;
                    }
                };
                let Some(SyncResPayload::QueryEvents(query_res)) = resp.payload else {
                    break;
                };
                let envelope = query_res.envelope.unwrap_or_default();
                let applied_event_seqs = self
                    .sync_apply_use_case
                    .apply_critical_events(&user_id, &envelope.events)
                    .await;
                let safe_event_seq = max_applied_event_prefix_seq(
                    after_seq.max(0) as u64,
                    &envelope.events,
                    &applied_event_seqs,
                );
                if safe_event_seq > after_seq.max(0) as u64 {
                    after_seq = safe_event_seq as i64;
                    if let Err(e) = self
                        .sync_apply_use_case
                        .save_cursor_with_remote(
                            &user_id,
                            &cursor_key,
                            safe_event_seq,
                            |_, _, _| async { Ok(()) },
                        )
                        .await
                    {
                        tracing::warn!(
                            conversation_id = %conversation_id,
                            error = %e,
                            "关键事件回放：保存本地游标失败，继续本轮同步"
                        );
                    }
                }
                if envelope.max_seq > safe_event_seq {
                    tracing::warn!(
                        conversation_id = %conversation_id,
                        after_seq,
                        safe_event_seq,
                        remote_max_seq = envelope.max_seq,
                        "关键事件回放未连续完成，保留 cursor 等待后续重放"
                    );
                    break;
                }
                if !envelope.has_more || envelope.max_seq == 0 {
                    break;
                }
            }
        }
        Ok(())
    }
}

impl SessionSyncRunner for SyncProtocolAdapter {
    fn request_message_sync(
        &self,
        conversation_id: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = conversation_id.to_string();
        Box::pin(async move { self.sync_conversation(&id).await })
    }

    fn request_message_sync_from_seq(
        &self,
        conversation_id: &str,
        last_seq: u64,
        limit: i32,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = conversation_id.to_string();
        Box::pin(async move { self.sync_conversation_from_seq(&id, last_seq, limit).await })
    }

    fn send_read_ack(
        &self,
        conversation_id: &str,
        read_seq: u64,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = conversation_id.to_string();
        Box::pin(async move { self.send_read_ack_impl(&id, read_seq).await })
    }

    fn request_participants_sync(
        &self,
        conversation_id: &str,
        limit: i32,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = Result<Vec<crate::model::ConversationParticipant>>>
                + Send
                + '_,
        >,
    > {
        let id = conversation_id.to_string();
        Box::pin(async move { self.sync_conversation_participants(&id, limit).await })
    }
}

impl ConversationSummarySync for SyncProtocolAdapter {
    fn sync_conversation_summaries(
        &self,
        user_id: &str,
        run: SyncRunContext,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let user_id = user_id.to_string();
        Box::pin(async move { self.sync_conversations_with_context(&user_id, run).await })
    }
}

impl SyncResponseHandler for SyncProtocolAdapter {
    fn handle_sync_response(
        &self,
        resp: SyncRes,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            if self
                .sync_request_use_case
                .handle_response(resp.clone())
                .await
            {
                return;
            }
            if resp.payload.is_none() {
                tracing::warn!("sync response has no payload, ignored");
            }
        })
    }
}

fn critical_event_cursor_key(conversation_id: &str) -> String {
    format!("{CRITICAL_EVENT_CURSOR_KEY}:{conversation_id}")
}

fn max_applied_event_prefix_seq(
    known_seq: u64,
    events: &[flare_proto::common::Event],
    applied_seqs: &[u64],
) -> u64 {
    if events.is_empty() {
        return known_seq;
    }

    let applied = applied_seqs
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let mut cursor = known_seq;
    for event in events {
        let seq = event.seq;
        if seq <= cursor {
            continue;
        }
        if applied.contains(&seq) {
            cursor = seq;
            continue;
        }
        break;
    }
    cursor
}

impl SyncProtocolAdapter {
    /// 拉取会话列表（conversation.proto SyncConversationsRequest，经 DATA 发送）
    pub async fn sync_conversations_impl(&self, user_id: &str) -> Result<()> {
        self.sync_conversations_with_context(user_id, SyncRunContext::initial_login())
            .await
    }

    pub async fn sync_conversations_with_context(
        &self,
        user_id: &str,
        run: SyncRunContext,
    ) -> Result<()> {
        tracing::debug!(user_id = %user_id, "开始同步会话列表");

        {
            let mut user = self.active_user_id.lock().await;
            *user = user_id.to_string();
        }
        self.transition_sync(&run, SyncTransition::SyncRequested);
        if matches!(
            run.trigger,
            SyncTrigger::InitialLogin | SyncTrigger::Reconnect
        ) && let Err(error) = self.push_local_read_states_before_summary_sync().await
        {
            tracing::warn!(
                error = %error,
                "push local read states before summary sync failed"
            );
        }
        let prior_cursor = self
            .stores
            .cursors
            .get_conversation_cursor(user_id, CONVERSATION_CURSOR_KEY)
            .await?;
        let local_cursor_ms = prior_cursor.as_ref().and_then(|cursor| {
            if cursor.last_seq > 0 {
                Some(cursor.last_seq)
            } else if cursor.synced_at > 0 {
                Some(cursor.synced_at)
            } else {
                None
            }
        });
        let remote_cursor_ms = match self
            .get_remote_cursor_seq(user_id, CONVERSATION_CURSOR_KEY)
            .await
        {
            Ok(cursor) => cursor,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "远端会话游标拉取失败，跳过远端游标继续会话列表同步"
                );
                None
            }
        };

        let local_conv_count = self.stores.conversations.list().await?.len();
        let cursor_selection = SyncPolicy::select_conversation_cursor_ms(
            local_cursor_ms,
            remote_cursor_ms,
            local_conv_count,
        );
        if cursor_selection.drop_local_incremental_cursor {
            tracing::debug!(
                "服务端未返回 __conversations__ 游标（常见于同步编排实例冷启动/缓存未命中）；放弃本地时间游标，全量拉会话列表"
            );
        }
        let mut cursor_ts = cursor_selection
            .selected_cursor_ms
            .and_then(ms_to_prost_timestamp);

        tracing::debug!(
            local_cursor = ?local_cursor_ms,
            remote_cursor = ?remote_cursor_ms,
            using_cursor = ?cursor_ts,
            local_conv_count,
            "会话同步游标信息"
        );

        let mut total_synced = 0usize;
        let mut server_conversation_ids = HashSet::new();
        let should_prune_orphans = cursor_selection.drop_local_incremental_cursor
            || matches!(
                run.trigger,
                SyncTrigger::InitialLogin | SyncTrigger::Reconnect
            );
        loop {
            tracing::debug!(
                cursor = ?cursor_ts,
                sync_kind = SyncKind::Conversations as i32,
                "发送会话同步请求"
            );
            let resp = match self
                .sync_request_use_case
                .request_conversations(cursor_ts, 100)
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    tracing::error!(error = %error, "会话同步响应接收失败");
                    self.transition_sync(&run, SyncTransition::SyncFailed);
                    return Err(error);
                }
            };

            tracing::debug!(
                conversations_count = resp.conversations.len(),
                has_more = resp.has_more,
                "收到会话同步响应"
            );

            let applied = self
                .sync_apply_use_case
                .apply_conversations(user_id, &resp)
                .await?;
            server_conversation_ids.extend(applied.synced_conversation_ids.iter().cloned());
            total_synced += resp.conversations.len();
            let server_cursor_ms = applied.server_cursor_ms;
            self.save_cursor_with_remote(user_id, CONVERSATION_CURSOR_KEY, server_cursor_ms)
                .await?;
            if matches!(
                run.trigger,
                SyncTrigger::InitialLogin | SyncTrigger::Reconnect
            ) {
                self.sync_conversation_messages_bounded(
                    &applied.message_sync_conversation_ids,
                    run.clone(),
                    self.init_message_sync_concurrency,
                )
                .await?;
            } else {
                for conversation_id in &applied.message_sync_conversation_ids {
                    if let Err(error) = self
                        .sync_conversation_with_context(conversation_id, run.clone())
                        .await
                    {
                        tracing::warn!(
                            conversation_id = %conversation_id,
                            error = %error,
                            "会话摘要已更新，但补拉会话消息失败"
                        );
                        if run.visibility.is_user_visible() {
                            return Err(error);
                        }
                    }
                }
            }
            if !applied.has_more {
                if should_prune_orphans {
                    match self
                        .sync_apply_use_case
                        .prune_local_conversations_not_on_server(&server_conversation_ids)
                        .await
                    {
                        Ok(pruned) if !pruned.is_empty() => {
                            tracing::info!(
                                count = pruned.len(),
                                "pruned local conversations missing from server snapshot"
                            );
                        }
                        Ok(_) => {}
                        Err(error) => {
                            tracing::warn!(
                                error = %error,
                                "prune local orphan conversations failed"
                            );
                        }
                    }
                }
                self.transition_sync(&run, SyncTransition::SyncDone);
                tracing::debug!(total_synced, "会话列表同步完成");
                break;
            }
            cursor_ts = resp.server_conversation_cursor;
        }
        Ok(())
    }

    async fn sync_conversation_messages_bounded(
        &self,
        conversation_ids: &[String],
        run: SyncRunContext,
        concurrency: usize,
    ) -> Result<()> {
        if conversation_ids.is_empty() {
            return Ok(());
        }
        let limit = concurrency.max(1);
        let results: Vec<(String, Result<()>)> = stream::iter(conversation_ids.iter().cloned())
            .map(|conversation_id| {
                let run = run.clone();
                async move {
                    let result = self
                        .sync_conversation_with_context(&conversation_id, run)
                        .await;
                    (conversation_id, result)
                }
            })
            .buffer_unordered(limit)
            .collect()
            .await;
        for (conversation_id, result) in results {
            if let Err(error) = result {
                tracing::warn!(
                    conversation_id = %conversation_id,
                    error = %error,
                    "会话摘要已更新，但补拉会话消息失败"
                );
                if run.visibility.is_user_visible() {
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    /// 单会话消息同步（sync.proto Sync.single_conversation，经 DATA 发送）
    pub async fn sync_conversation(&self, conversation_id: &str) -> Result<()> {
        self.sync_conversation_with_context(
            conversation_id,
            SyncRunContext::manual_single_conversation(),
        )
        .await
    }

    pub async fn sync_conversation_with_context(
        &self,
        conversation_id: &str,
        run: SyncRunContext,
    ) -> Result<()> {
        tracing::debug!(conversation_id = %conversation_id, "开始同步会话消息");

        self.transition_sync(&run, SyncTransition::SyncRequested);
        let user_id = self.current_user_id().await;
        let cursor_last_seq = self
            .stores
            .cursors
            .get_conversation_cursor(&user_id, conversation_id)
            .await?
            .map(|c| c.last_seq)
            .unwrap_or(0);
        let conversation = self.stores.conversations.get(conversation_id).await?;
        let cleared_floor = conversation
            .as_ref()
            .map(|c| crate::domain::local_cleared_through_seq(&c.ext).max(c.visible_after_seq))
            .unwrap_or(0);
        let local_max_seq = self
            .stores
            .conversations
            .get_local_max_seq(conversation_id)
            .await?;
        let remote_cursor_seq = if user_id.is_empty() {
            0
        } else {
            self.get_remote_cursor_seq(&user_id, conversation_id)
                .await
                .ok()
                .flatten()
                .unwrap_or(0)
        };
        let last_seq = local_message_sync_start_seq(cursor_last_seq, local_max_seq, cleared_floor);

        tracing::debug!(
            conversation_id = %conversation_id,
            user_id = %user_id,
            last_seq = last_seq,
            cursor_last_seq = cursor_last_seq,
            local_max_seq = local_max_seq,
            cleared_floor = cleared_floor,
            remote_cursor_seq = remote_cursor_seq,
            "会话消息同步起始位置"
        );

        let (page_count, total_messages) = self
            .sync_conversation_loop(&run, conversation_id, last_seq, DEFAULT_SYNC_LIMIT)
            .await?;
        tracing::debug!(
            conversation_id = %conversation_id,
            total_pages = page_count,
            total_messages = total_messages,
            "会话消息同步完成"
        );
        // 会话级关键事件兜底回放容易在会话数较多时放大 QueryEvents 请求量，
        // 且与全局 key_events 任务存在重复。这里不阻塞消息同步链路，优先保证消息实时性。
        Ok(())
    }

    /// 按需同步非单聊会话成员。会话列表只负责摘要；成员在打开会话、发起群通话或空闲任务中调用。
    pub async fn sync_conversation_participants(
        &self,
        conversation_id: &str,
        page_limit: i32,
    ) -> Result<Vec<crate::model::ConversationParticipant>> {
        let Some(store) = &self.stores.conversation_participants else {
            return Ok(Vec::new());
        };
        let known_version = store.version(conversation_id).await?;
        let mut cursor = String::new();
        let mut all = Vec::new();
        let mut first_page = true;
        loop {
            let resp = self
                .sync_request_use_case
                .request_conversation_participants(ConversationParticipantsSync {
                    conversation_id: conversation_id.to_string(),
                    known_participant_version: known_version,
                    cursor: cursor.clone(),
                    limit: page_limit.max(1),
                    include_removed: false,
                })
                .await?;
            let Some(SyncResPayload::ConversationParticipants(page)) = resp.payload else {
                return Err(FlareError::general_error(
                    "unexpected conversation participants response".to_string(),
                ));
            };
            let participants = page
                .participants
                .into_iter()
                .map(crate::model::ConversationParticipant::from)
                .collect::<Vec<_>>();
            if !participants.is_empty() || first_page {
                store
                    .save_page(
                        conversation_id,
                        &participants,
                        page.participant_version,
                        first_page && cursor.is_empty(),
                    )
                    .await?;
            }
            all.extend(participants);
            if !page.has_more {
                break;
            }
            cursor = page.next_cursor;
            first_page = false;
        }
        Ok(all)
    }

    /// 单会话消息同步（显式指定 last_seq 与 limit，供业务层对接 storage sync 契约）
    pub async fn sync_conversation_from_seq(
        &self,
        conversation_id: &str,
        last_seq: u64,
        limit: i32,
    ) -> Result<()> {
        self.sync_conversation_from_seq_with_context(
            conversation_id,
            last_seq,
            limit,
            SyncRunContext::silent_gap_repair(),
        )
        .await
    }

    pub async fn sync_conversation_from_seq_with_context(
        &self,
        conversation_id: &str,
        last_seq: u64,
        limit: i32,
        run: SyncRunContext,
    ) -> Result<()> {
        self.transition_sync(&run, SyncTransition::SyncRequested);
        let page_limit = if limit > 0 { limit } else { DEFAULT_SYNC_LIMIT };
        let _ = self
            .sync_conversation_loop(&run, conversation_id, last_seq, page_limit)
            .await?;
        Ok(())
    }

    async fn sync_conversation_loop(
        &self,
        run: &SyncRunContext,
        conversation_id: &str,
        start_seq: u64,
        page_limit: i32,
    ) -> Result<(usize, usize)> {
        let mut from_seq = start_seq;
        let mut cursor = String::new();
        let mut total_messages = 0usize;
        let mut page_count = 0usize;

        loop {
            page_count += 1;
            let (next_seq, has_more, next_cursor) = self
                .request_single_page(run, conversation_id, from_seq, page_limit, cursor)
                .await?;

            let messages_in_page = if next_seq > from_seq {
                (next_seq - from_seq) as usize
            } else {
                0
            };
            total_messages += messages_in_page;

            tracing::debug!(
                conversation_id = %conversation_id,
                page = page_count,
                from_seq = from_seq,
                next_seq = next_seq,
                has_more = has_more,
                messages_in_page = messages_in_page,
                total_messages = total_messages,
                "消息同步页面完成"
            );

            if next_seq <= from_seq {
                if has_more {
                    tracing::warn!(
                        conversation_id = %conversation_id,
                        from_seq,
                        next_seq,
                        "消息同步页面未推进但仍存在远端水位，停止本轮同步并等待后续补偿"
                    );
                    self.transition_sync(run, SyncTransition::SyncFailed);
                }
                break;
            }
            if !has_more {
                break;
            }
            from_seq = next_seq;
            cursor = next_cursor;
        }

        Ok((page_count, total_messages))
    }
}

#[cfg(test)]
mod tests {
    use super::{build_read_ack, max_applied_event_prefix_seq};
    use flare_proto::common::{AckType, ack::Payload as AckPayload};

    fn make_event(seq: u64) -> flare_proto::common::Event {
        flare_proto::common::Event {
            seq,
            ..Default::default()
        }
    }

    #[test]
    fn event_prefix_progresses_with_sparse_seq_when_prefix_applied() {
        let events = vec![make_event(1184), make_event(1191), make_event(1202)];
        let safe = max_applied_event_prefix_seq(0, &events, &[1184, 1191, 1202]);
        assert_eq!(safe, 1202);
    }

    #[test]
    fn event_prefix_stops_on_first_not_applied_event() {
        let events = vec![make_event(1184), make_event(1191), make_event(1202)];
        let safe = max_applied_event_prefix_seq(0, &events, &[1184, 1202]);
        assert_eq!(safe, 1184);
    }

    #[test]
    fn event_prefix_keeps_known_seq_when_no_events() {
        let safe = max_applied_event_prefix_seq(77, &[], &[]);
        assert_eq!(safe, 77);
    }

    #[test]
    fn build_read_ack_uses_typed_payload() {
        let ack = build_read_ack("conv-1", 42);

        assert_eq!(ack.r#type, AckType::Read as i32);
        match ack.payload {
            Some(AckPayload::Read(read)) => {
                assert_eq!(read.conversation_id, "conv-1");
                assert_eq!(read.read_seq, 42);
                assert_eq!(read.device_id, None);
                assert_eq!(read.ack_id, ack.ack_id);
            }
            other => panic!("expected AckPayload::Read, got {other:?}"),
        }
    }
}
