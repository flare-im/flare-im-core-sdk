//! 同步协议处理：会话列表、单会话消息、已读上报及响应落库。
//! 与 flare-proto 对齐：上行 Ack(ConversationAck)、Sync；下行 SyncRes。

use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use flare_proto::common::{
    Ack, AckType, ConversationAck, ConversationsIncrementalSync, ErrorCode, GetSyncCursorSync,
    MessageStatus, MultiDeviceCursor, QueryEventsSync, SingleConversationSync, Sync as SyncWire,
    SyncKind, SyncRes, UpdateSyncCursorSync, ack::Payload as AckPayload,
    event::Payload as DomainEventPayload, sync::Payload as SyncPayload,
    sync_res::Payload as SyncResPayload,
};
use prost::Message;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::oneshot;

use crate::core::{SessionSyncRunner, SyncResponseHandler};
use crate::domain::SyncCursorVo;
use crate::error::{FlareError, Result};
use crate::event::SyncNotify;
use crate::event::{EventBus, ExtensionEvent, MessageEvent, SdkEvent};
use crate::fsm::{SyncFsm, SyncState, SyncTransition};
use crate::model::IMMessage;
use crate::protocol::PacketSender;
use crate::store::StoreProvider;
use crate::util::date::{
    ms_to_prost_timestamp, prost_timestamp_to_ms, system_time_to_prost_timestamp,
};

const DEFAULT_SYNC_LIMIT: i32 = 100;
const CONVERSATION_CURSOR_KEY: &str = "__conversations__";
const CRITICAL_EVENT_CURSOR_KEY: &str = "__critical_events__";

#[derive(Debug, Clone, Default)]
struct QueryEventsReqV1 {
    conversation_id: String,
    after_seq: i64,
    before_seq: i64,
    limit: i32,
    event_types: Vec<i32>,
    include_deleted: bool,
}

pub struct SyncHandler {
    sender: Arc<PacketSender>,
    stores: StoreProvider,
    bus: EventBus,
    pending_sync: AsyncMutex<HashMap<String, VecDeque<oneshot::Sender<SyncRes>>>>,
    pending_sync_conv:
        AsyncMutex<Option<oneshot::Sender<flare_proto::common::ConversationsIncrementalSyncRes>>>,
    sync_state: Mutex<SyncState>,
    active_user_id: AsyncMutex<String>,
}

impl SyncHandler {
    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn pending_key_from_resp(resp: &SyncRes) -> Option<String> {
        match &resp.payload {
            Some(SyncResPayload::SingleConversation(s)) => {
                Some(format!("single:{}", s.conversation_id))
            }
            Some(SyncResPayload::QueryEvents(_)) => Some("query_events".to_string()),
            Some(SyncResPayload::GetSyncCursor(_)) => Some("get_cursor".to_string()),
            Some(SyncResPayload::UpdateSyncCursor(_)) => Some("update_cursor".to_string()),
            _ => None,
        }
    }

    async fn push_pending_sync(&self, key: &str, tx: oneshot::Sender<SyncRes>) {
        let mut guard = self.pending_sync.lock().await;
        guard
            .entry(key.to_string())
            .or_insert_with(VecDeque::new)
            .push_back(tx);
    }

    async fn pop_pending_sync(&self, key: &str) -> Option<oneshot::Sender<SyncRes>> {
        let mut guard = self.pending_sync.lock().await;
        let queue = guard.get_mut(key)?;
        let tx = queue.pop_front();
        if queue.is_empty() {
            guard.remove(key);
        }
        tx
    }

    async fn pop_any_pending_sync(&self) -> Option<oneshot::Sender<SyncRes>> {
        let mut guard = self.pending_sync.lock().await;
        const PREFERRED_KEYS: [&str; 3] = ["get_cursor", "query_events", "update_cursor"];
        for key in PREFERRED_KEYS {
            if let Some(queue) = guard.get_mut(key) {
                let tx = queue.pop_front();
                if queue.is_empty() {
                    guard.remove(key);
                }
                if tx.is_some() {
                    return tx;
                }
            }
        }
        None
    }

    async fn get_remote_cursor_seq(
        &self,
        _user_id: &str,
        conversation_id: &str,
    ) -> Result<Option<u64>> {
        let resp = self
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
        Ok(Some(parsed.last_sync_seq.max(0) as u64))
    }

    async fn update_remote_cursor_seq(
        &self,
        _user_id: &str,
        conversation_id: &str,
        last_seq: u64,
    ) -> Result<()> {
        let resp = self
            .request_update_cursor(UpdateSyncCursorSync {
                cursor: Some(MultiDeviceCursor {
                    device_id: String::new(),
                    conversation_id: conversation_id.to_string(),
                    last_sync_seq: last_seq,
                    last_sync_at: Some(system_time_to_prost_timestamp()),
                    last_read_seq: 0,
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

    async fn save_cursor_with_remote(
        &self,
        user_id: &str,
        conversation_id: &str,
        last_seq: u64,
    ) -> Result<()> {
        self.stores
            .cursors
            .save_conversation_cursor(&SyncCursorVo {
                user_id: user_id.to_string(),
                conversation_id: conversation_id.to_string(),
                last_seq,
                synced_at: Self::now_ms(),
            })
            .await?;
        if let Err(e) = self
            .update_remote_cursor_seq(user_id, conversation_id, last_seq)
            .await
        {
            tracing::warn!(
                user_id = %user_id,
                conversation_id = %conversation_id,
                error = %e,
                "update remote cursor failed"
            );
        }
        Ok(())
    }

    pub fn new(sender: Arc<PacketSender>, stores: StoreProvider, bus: EventBus) -> Self {
        Self {
            sender,
            stores,
            bus,
            pending_sync: AsyncMutex::new(HashMap::new()),
            pending_sync_conv: AsyncMutex::new(None),
            sync_state: Mutex::new(SyncState::Idle),
            active_user_id: AsyncMutex::new(String::new()),
        }
    }

    async fn current_user_id(&self) -> String {
        self.active_user_id.lock().await.clone()
    }

    fn transition_sync(&self, event: SyncTransition) {
        let mut guard = self.sync_state.lock().unwrap();
        if let Ok(next) = SyncFsm::transition(*guard, &event) {
            *guard = next;
            drop(guard);
            self.bus
                .publish(SdkEvent::Sync(SyncNotify::StateChanged { state: next }));
        }
    }

    /// 已读上报（ack.proto Ack.payload.conversation = ConversationAck）
    pub async fn send_read_ack(&self, conversation_id: &str, read_seq: u64) -> Result<()> {
        self.send_read_ack_impl(conversation_id, read_seq).await
    }

    async fn send_read_ack_impl(&self, conversation_id: &str, read_seq: u64) -> Result<()> {
        let ack = Ack {
            r#type: AckType::Converstion as i32,
            ack_id: None,
            at: Some(system_time_to_prost_timestamp()),
            payload: Some(AckPayload::Conversation(ConversationAck {
                conversation_id: conversation_id.to_string(),
                server_msg_ids: vec![],
                last_delivered_seq: read_seq,
                metadata: std::collections::HashMap::new(),
            })),
        };
        self.sender.send_ack(&ack).await
    }

    /// 将 SyncRes.payload.single_conversation 转为事件列表并落库、发布、更新游标
    async fn apply_sync_res_single(
        &self,
        conversation_id: &str,
        resp: &SyncRes,
    ) -> Result<(u64, bool, String)> {
        if let Some(status) = &resp.status {
            if status.code != ErrorCode::Ok as i32 {
                tracing::error!(
                    conversation_id = %conversation_id,
                    code = status.code,
                    message = %status.message,
                    "同步响应错误状态"
                );
                self.transition_sync(SyncTransition::SyncFailed);
                return Ok((0, false, String::new()));
            }
        }
        let sc = match &resp.payload {
            Some(SyncResPayload::SingleConversation(s)) => s,
            _ => {
                tracing::warn!(conversation_id = %conversation_id, "同步响应payload为空或类型不匹配");
                self.transition_sync(SyncTransition::SyncDone);
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
        let known_seq = if user_id.is_empty() {
            0
        } else {
            self.stores
                .cursors
                .get_conversation_cursor(&user_id, conversation_id)
                .await?
                .map(|c| c.last_seq)
                .unwrap_or(0)
        };

        tracing::debug!(
            conversation_id = %conversation_id,
            known_seq = known_seq,
            "本地已知消息seq"
        );

        let mut events = Vec::new();
        let mut decoded_messages = Vec::new();
        for item in &sc.items {
            // single_conversation 的 payload 主体是 Message（非 Event）。
            if let Ok(msg) = flare_proto::common::Message::decode(item.payload.as_slice()) {
                decoded_messages.push(msg);
                continue;
            }
            if let Ok(ev) = flare_proto::common::Event::decode(item.payload.as_slice()) {
                events.push(ev);
            }
        }
        if !events.is_empty() || !decoded_messages.is_empty() {
            self.transition_sync(SyncTransition::DataReceived);
        }
        let mut messages: Vec<IMMessage> = Vec::new();
        for m in &decoded_messages {
            if m.seq > known_seq {
                messages.push(IMMessage::new(m.clone()));
            }
        }
        for ev in &events {
            if let Some(DomainEventPayload::Message(m)) = &ev.payload {
                if m.seq > known_seq {
                    messages.push(IMMessage::new(m.clone()));
                }
                continue;
            }
            if let Some(DomainEventPayload::Recall(recall)) = &ev.payload {
                let _ = self
                    .stores
                    .messages
                    .update_status(&recall.server_msg_id, MessageStatus::Recalled as i32)
                    .await;
                self.bus.publish(SdkEvent::Message(MessageEvent::Recalled {
                    conversation_id: ev.conversation_id.clone(),
                    event: recall.clone(),
                }));
                continue;
            }
            if let Some(DomainEventPayload::Edit(edit)) = &ev.payload {
                let _ = self
                    .stores
                    .messages
                    .update_content(&edit.server_msg_id, edit.new_content.clone())
                    .await;
                self.bus.publish(SdkEvent::Extension(ExtensionEvent {
                    source: "sync_replay".to_string(),
                    event_type: "message_edit".to_string(),
                    payload: edit.encode_to_vec(),
                }));
                continue;
            }
            if let Some(DomainEventPayload::Delete(delete)) = &ev.payload {
                let _ = self.stores.messages.delete(&delete.server_msg_id).await;
                self.bus.publish(SdkEvent::Extension(ExtensionEvent {
                    source: "sync_replay".to_string(),
                    event_type: "message_delete".to_string(),
                    payload: delete.encode_to_vec(),
                }));
                continue;
            }
            if let Some(DomainEventPayload::ConversationDelete(_)) = &ev.payload {
                self.bus.publish(SdkEvent::Conversation(
                    crate::event::ConversationEvent::Deleted {
                        conversation_id: ev.conversation_id.clone(),
                    },
                ));
                continue;
            }
            if let Some(DomainEventPayload::Conversation(_)) = &ev.payload {
                self.bus.publish(SdkEvent::Conversation(
                    crate::event::ConversationEvent::Updated {
                        conversation_id: ev.conversation_id.clone(),
                    },
                ));
                continue;
            }
            match ev.r#type {
                12 | 13 | 14 | 15 => {
                    self.bus.publish(SdkEvent::Extension(ExtensionEvent {
                        source: "sync_replay".to_string(),
                        event_type: format!("event_type_{}", ev.r#type),
                        payload: ev.encode_to_vec(),
                    }));
                }
                _ => {}
            }
        }
        if !messages.is_empty() {
            tracing::info!(
                conversation_id = %conversation_id,
                count = messages.len(),
                "保存消息到本地存储"
            );

            self.stores.messages.save_batch(&messages).await?;

            tracing::info!(
                conversation_id = %conversation_id,
                count = messages.len(),
                "消息保存成功"
            );

            if let Some(latest) = messages.iter().max_by_key(|m| m.seq) {
                let max_seq = sc.max_seq.max(latest.seq);
                let _ = self
                    .stores
                    .conversations
                    .update_last_message(
                        conversation_id,
                        latest.server_id(),
                        latest.sender_id(),
                        latest.timestamp,
                        latest.text_for_storage().as_deref(),
                        max_seq,
                    )
                    .await;
            }
            for m in &messages {
                self.bus.publish(SdkEvent::Message(MessageEvent::Received {
                    message: m.clone(),
                }));
            }
        } else {
            tracing::debug!(conversation_id = %conversation_id, "没有新消息需要保存");
        }
        if sc.max_seq > 0 {
            if !user_id.is_empty() {
                self.save_cursor_with_remote(&user_id, conversation_id, sc.max_seq)
                    .await?;
            }
        }
        if sc.has_more {
            self.transition_sync(SyncTransition::BatchDone);
        } else {
            self.transition_sync(SyncTransition::SyncDone);
        }
        Ok((sc.max_seq, sc.has_more, sc.next_cursor.clone()))
    }

    async fn request_single_page(
        &self,
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

        let sync_event = SyncWire {
            kind: SyncKind::SingleConversation as i32,
            device_id: String::new(),
            payload: Some(SyncPayload::SingleConversation(SingleConversationSync {
                conversation_id: conversation_id.to_string(),
                max_seq: last_seq,
                cursor,
                limit,
            })),
        };
        let mut retries = 0u8;
        loop {
            let (tx, rx) = oneshot::channel();
            let pending_key = format!("single:{conversation_id}");
            self.push_pending_sync(&pending_key, tx).await;
            self.sender.send_sync(&sync_event).await?;
            match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
                Ok(Ok(resp)) => {
                    return self.apply_sync_res_single(conversation_id, &resp).await;
                }
                _ => {
                    retries += 1;
                    tracing::warn!(
                        conversation_id = %conversation_id,
                        retry = retries,
                        "消息同步请求超时或失败，准备重试"
                    );
                    {
                        let _ = self.pop_pending_sync(&pending_key).await;
                    }
                    if retries >= 3 {
                        tracing::error!(
                            conversation_id = %conversation_id,
                            "消息同步请求重试次数超过上限(3次)"
                        );
                        self.transition_sync(SyncTransition::SyncFailed);
                        return Ok((last_seq, false, String::new()));
                    }
                }
            }
        }
    }

    async fn request_query_events(&self, req: QueryEventsSync) -> Result<SyncRes> {
        let sync_event = SyncWire {
            kind: SyncKind::QueryEvents as i32,
            device_id: String::new(),
            payload: Some(SyncPayload::QueryEvents(req)),
        };
        let pending_key = "query_events".to_string();
        let (tx, rx) = oneshot::channel();
        self.push_pending_sync(&pending_key, tx).await;
        self.sender.send_sync(&sync_event).await?;
        match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(Ok(resp)) => Ok(resp),
            _ => {
                let _ = self.pop_pending_sync(&pending_key).await;
                self.transition_sync(SyncTransition::SyncFailed);
                Err(FlareError::general_error(
                    "query events timeout or canceled".to_string(),
                ))
            }
        }
    }

    async fn request_get_cursor(&self, req: GetSyncCursorSync) -> Result<SyncRes> {
        let sync_event = SyncWire {
            kind: SyncKind::GetSyncCursor as i32,
            device_id: String::new(),
            payload: Some(SyncPayload::GetSyncCursor(req)),
        };
        let pending_key = "get_cursor".to_string();
        let (tx, rx) = oneshot::channel();
        self.push_pending_sync(&pending_key, tx).await;

        tracing::info!("发送GetSyncCursor请求");
        self.sender.send_sync(&sync_event).await?;

        match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(Ok(resp)) => {
                if let Some(status) = &resp.status {
                    if status.code != ErrorCode::Ok as i32 {
                        tracing::warn!(code = status.code, message = %status.message, "GetSyncCursor响应为错误状态");
                    }
                }
                tracing::info!("GetSyncCursor响应接收成功");
                Ok(resp)
            }
            Ok(Err(_)) => {
                tracing::error!("GetSyncCursor响应channel关闭");
                let _ = self.pop_pending_sync(&pending_key).await;
                self.transition_sync(SyncTransition::SyncFailed);
                Err(FlareError::general_error(
                    "get cursor channel closed".to_string(),
                ))
            }
            Err(_) => {
                tracing::error!("GetSyncCursor请求超时(30秒)");
                let _ = self.pop_pending_sync(&pending_key).await;
                self.transition_sync(SyncTransition::SyncFailed);
                Err(FlareError::general_error(
                    "get cursor timeout or canceled".to_string(),
                ))
            }
        }
    }

    async fn request_update_cursor(&self, req: UpdateSyncCursorSync) -> Result<SyncRes> {
        let sync_event = SyncWire {
            kind: SyncKind::UpdateSyncCursor as i32,
            device_id: String::new(),
            payload: Some(SyncPayload::UpdateSyncCursor(req)),
        };
        let pending_key = "update_cursor".to_string();
        let (tx, rx) = oneshot::channel();
        self.push_pending_sync(&pending_key, tx).await;
        self.sender.send_sync(&sync_event).await?;
        match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(Ok(resp)) => Ok(resp),
            _ => {
                let _ = self.pop_pending_sync(&pending_key).await;
                self.transition_sync(SyncTransition::SyncFailed);
                Err(FlareError::general_error(
                    "update cursor timeout or canceled".to_string(),
                ))
            }
        }
    }

    pub async fn sync_critical_events(&self) -> Result<()> {
        let user_id = self.current_user_id().await;
        if user_id.is_empty() {
            return Ok(());
        }
        let local_after_seq = self
            .stores
            .cursors
            .get_conversation_cursor(&user_id, CRITICAL_EVENT_CURSOR_KEY)
            .await?
            .map(|c| c.last_seq as i64)
            .unwrap_or(0);
        let remote_after_seq = self
            .get_remote_cursor_seq(&user_id, CRITICAL_EVENT_CURSOR_KEY)
            .await?
            .unwrap_or(0) as i64;
        let mut after_seq = local_after_seq.max(remote_after_seq);

        loop {
            let req = QueryEventsReqV1 {
                conversation_id: String::new(),
                after_seq,
                before_seq: 0,
                limit: 500,
                event_types: vec![],
                include_deleted: true,
            };
            let resp = self
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
                .await?;
            let Some(SyncResPayload::QueryEvents(query_res)) = resp.payload else {
                break;
            };
            let envelope = query_res.envelope.unwrap_or_default();
            for ev in &envelope.events {
                if let Some(DomainEventPayload::Recall(recall)) = &ev.payload {
                    let _ = self
                        .stores
                        .messages
                        .update_status(&recall.server_msg_id, MessageStatus::Recalled as i32)
                        .await;
                }
                if let Some(DomainEventPayload::Edit(edit)) = &ev.payload {
                    let _ = self
                        .stores
                        .messages
                        .update_content(&edit.server_msg_id, edit.new_content.clone())
                        .await;
                }
                if let Some(DomainEventPayload::Delete(delete)) = &ev.payload {
                    let _ = self.stores.messages.delete(&delete.server_msg_id).await;
                }
                self.bus.publish(SdkEvent::Extension(ExtensionEvent {
                    source: "sync_query_events".to_string(),
                    event_type: format!("event_type_{}", ev.r#type),
                    payload: ev.encode_to_vec(),
                }));
            }
            after_seq = envelope.max_seq as i64;
            self.save_cursor_with_remote(&user_id, CRITICAL_EVENT_CURSOR_KEY, envelope.max_seq)
                .await?;
            if !envelope.has_more || envelope.max_seq == 0 {
                break;
            }
        }
        Ok(())
    }
}

impl SessionSyncRunner for SyncHandler {
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
}

impl SyncResponseHandler for SyncHandler {
    fn handle_sync_response(
        &self,
        resp: SyncRes,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            if let Some(SyncResPayload::ConversationsIncremental(r)) = &resp.payload {
                let mut guard = self.pending_sync_conv.lock().await;
                if let Some(tx) = guard.take() {
                    let _ = tx.send(r.clone());
                }
                return;
            }
            if let Some(key) = Self::pending_key_from_resp(&resp) {
                if let Some(tx) = self.pop_pending_sync(&key).await {
                    let _ = tx.send(resp);
                }
                return;
            }

            let status_code = resp.status.as_ref().map(|s| s.code).unwrap_or_default();
            if status_code != ErrorCode::Ok as i32 {
                if let Some(tx) = self.pop_any_pending_sync().await {
                    tracing::warn!(
                        code = status_code,
                        "sync response has no payload, routed to pending request"
                    );
                    let _ = tx.send(resp);
                }
            }
        })
    }
}

impl SyncHandler {
    /// 拉取会话列表（conversation.proto SyncConversationsRequest，经 DATA 发送）
    pub async fn sync_conversations_impl(&self, user_id: &str) -> Result<()> {
        tracing::info!(user_id = %user_id, "开始同步会话列表");

        {
            let mut user = self.active_user_id.lock().await;
            *user = user_id.to_string();
        }
        self.transition_sync(SyncTransition::SyncRequested);
        let prior_cursor = self
            .stores
            .cursors
            .get_conversation_cursor(user_id, CONVERSATION_CURSOR_KEY)
            .await?;
        let local_cursor_ts = prior_cursor
            .as_ref()
            .and_then(|c| ms_to_prost_timestamp(c.last_seq))
            .or_else(|| {
                prior_cursor
                    .as_ref()
                    .and_then(|c| ms_to_prost_timestamp(c.synced_at))
            });
        let remote_cursor_ts = self
            .get_remote_cursor_seq(user_id, CONVERSATION_CURSOR_KEY)
            .await?
            .and_then(ms_to_prost_timestamp);
        let mut cursor_ts = match (local_cursor_ts, remote_cursor_ts) {
            (Some(a), Some(b)) => {
                if prost_timestamp_to_ms(Some(&a)) >= prost_timestamp_to_ms(Some(&b)) {
                    Some(a)
                } else {
                    Some(b)
                }
            }
            (Some(_a), None) => {
                tracing::info!(
                    "服务端未返回 __conversations__ 游标（常见于同步编排实例冷启动/缓存未命中）；放弃本地时间游标，全量拉会话列表"
                );
                None
            }
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        let local_conv_count = self.stores.conversations.list().await?.len();
        if local_conv_count == 0 {
            tracing::info!(
                user_id = %user_id,
                "本地会话数为 0，强制清空增量游标并全量同步"
            );
            cursor_ts = None;
        }

        tracing::info!(
            local_cursor = ?local_cursor_ts,
            remote_cursor = ?remote_cursor_ts,
            using_cursor = ?cursor_ts,
            local_conv_count,
            "会话同步游标信息"
        );

        let mut total_synced = 0usize;
        loop {
            let (tx, rx) = oneshot::channel();
            {
                let mut guard = self.pending_sync_conv.lock().await;
                *guard = Some(tx);
            }
            let sync_event = SyncWire {
                kind: SyncKind::ConversationsIncremental as i32,
                device_id: String::new(),
                payload: Some(SyncPayload::ConversationsIncremental(
                    ConversationsIncrementalSync {
                        client_conversation_cursor: cursor_ts.clone(),
                        limit: 100,
                    },
                )),
            };

            tracing::info!(
                cursor = ?cursor_ts,
                sync_kind = sync_event.kind,
                "发送会话同步请求"
            );
            self.sender.send_sync(&sync_event).await?;

            let resp = match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    tracing::error!(error = %e, "会话同步响应接收失败");
                    self.transition_sync(SyncTransition::SyncFailed);
                    break;
                }
                Err(_) => {
                    tracing::error!("会话同步请求超时(30秒)");
                    self.transition_sync(SyncTransition::SyncFailed);
                    break;
                }
            };

            let conversation_ids: Vec<String> = resp
                .patches
                .iter()
                .map(|p| p.conversation_id.clone())
                .collect();
            let summaries = patches_to_summaries(&resp.patches);

            tracing::info!(
                patches_count = resp.patches.len(),
                summaries_count = summaries.len(),
                has_more = resp.has_more,
                "收到会话同步响应"
            );

            if !summaries.is_empty() {
                let conversations: Vec<crate::model::Conversation> = summaries
                    .into_iter()
                    .map(crate::model::Conversation::from)
                    .collect();

                tracing::debug!(count = conversations.len(), "保存会话到本地存储");

                if let Err(e) = self.stores.conversations.save_batch(&conversations).await {
                    tracing::error!(%e, count = conversations.len(), "保存会话失败");
                } else {
                    total_synced += conversations.len();
                    tracing::info!(
                        count = conversations.len(),
                        total = total_synced,
                        "会话保存成功"
                    );
                }
            } else {
                tracing::warn!("响应中没有会话数据");
            }

            self.bus.publish(SdkEvent::Conversation(
                crate::event::ConversationEvent::Synced { conversation_ids },
            ));
            let server_cursor_ms = prost_timestamp_to_ms(resp.server_conversation_cursor.as_ref());
            self.save_cursor_with_remote(user_id, CONVERSATION_CURSOR_KEY, server_cursor_ms)
                .await?;
            if !resp.has_more {
                self.transition_sync(SyncTransition::SyncDone);
                tracing::info!(total_synced, "会话列表同步完成");
                break;
            }
            cursor_ts = resp.server_conversation_cursor.clone();
        }
        Ok(())
    }

    /// 单会话消息同步（sync.proto Sync.single_conversation，经 DATA 发送）
    pub async fn sync_conversation(&self, conversation_id: &str) -> Result<()> {
        tracing::info!(conversation_id = %conversation_id, "开始同步会话消息");

        self.transition_sync(SyncTransition::SyncRequested);
        let user_id = self.current_user_id().await;
        let last_seq = self
            .stores
            .cursors
            .get_conversation_cursor(&user_id, conversation_id)
            .await?
            .map(|c| c.last_seq)
            .unwrap_or(0);

        tracing::info!(
            conversation_id = %conversation_id,
            user_id = %user_id,
            last_seq = last_seq,
            "会话消息同步起始位置"
        );

        let mut from_seq = last_seq;
        let mut cursor = String::new();
        let mut total_messages = 0usize;
        let mut page_count = 0usize;

        loop {
            page_count += 1;
            let (next_seq, has_more, next_cursor) = self
                .request_single_page(conversation_id, from_seq, DEFAULT_SYNC_LIMIT, cursor)
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

            if !has_more {
                tracing::info!(
                    conversation_id = %conversation_id,
                    total_pages = page_count,
                    total_messages = total_messages,
                    "会话消息同步完成"
                );
                break;
            }
            if next_seq <= from_seq {
                tracing::info!(
                    conversation_id = %conversation_id,
                    total_pages = page_count,
                    total_messages = total_messages,
                    "会话消息同步完成(无新消息)"
                );
                break;
            }
            from_seq = next_seq;
            cursor = next_cursor;
        }
        Ok(())
    }

    /// 单会话消息同步（显式指定 last_seq 与 limit，供业务层对接 storage sync 契约）
    pub async fn sync_conversation_from_seq(
        &self,
        conversation_id: &str,
        last_seq: u64,
        limit: i32,
    ) -> Result<()> {
        self.transition_sync(SyncTransition::SyncRequested);

        let mut from_seq = last_seq;
        let mut cursor = String::new();
        let page_limit = if limit > 0 { limit } else { DEFAULT_SYNC_LIMIT };
        loop {
            let (next_seq, has_more, next_cursor) = self
                .request_single_page(conversation_id, from_seq, page_limit, cursor)
                .await?;
            if !has_more {
                break;
            }
            if next_seq <= from_seq {
                break;
            }
            from_seq = next_seq;
            cursor = next_cursor;
        }
        Ok(())
    }
}

fn patches_to_summaries(
    patches: &[flare_proto::common::ConversationPatch],
) -> Vec<flare_proto::common::ConversationSummary> {
    patches
        .iter()
        .filter_map(|p| {
            if let Some(s) = &p.summary {
                return Some(s.clone());
            }
            p.light
                .as_ref()
                .map(|l| light_to_summary(l, &p.conversation_id))
        })
        .collect()
}

fn light_to_summary(
    l: &flare_proto::common::ConversationLight,
    conversation_id: &str,
) -> flare_proto::common::ConversationSummary {
    let ext = l.ext.clone();
    flare_proto::common::ConversationSummary {
        conversation_id: conversation_id.to_string(),
        conversation_type: l.conversation_type.clone(),
        unread_count: l.unread_count,
        max_seq: l.max_seq,
        last_read_seq: l.last_read_seq,
        is_muted: l.is_muted,
        is_pinned: l.is_pinned,
        updated_at: l.updated_at.clone(),
        last_message: l.preview.clone(),
        channel_id: l.channel_id.clone(),
        ext,
        ..Default::default()
    }
}
