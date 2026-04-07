use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use flare_proto::common::{
    ConversationsIncrementalSync, ConversationsIncrementalSyncRes, GetSyncCursorSync,
    QueryEventsSync, SingleConversationSync, Sync as SyncWire, SyncKind, SyncRes,
    UpdateSyncCursorSync, sync::Payload as SyncPayload, sync_res::Payload as SyncResPayload,
};
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::oneshot;

use crate::domain::{QUERY_EVENTS_TIMEOUT_SECS, UPDATE_CURSOR_TIMEOUT_SECS};
use crate::error::{FlareError, Result};
use crate::protocol::PacketSender;

const DEFAULT_TIMEOUT_SECS: u64 = 30;

pub struct SyncRequestUseCase {
    sender: Arc<PacketSender>,
    pending_sync: AsyncMutex<HashMap<String, VecDeque<oneshot::Sender<SyncRes>>>>,
    pending_sync_conv: AsyncMutex<Option<oneshot::Sender<ConversationsIncrementalSyncRes>>>,
}

impl SyncRequestUseCase {
    pub fn new(sender: Arc<PacketSender>) -> Self {
        Self {
            sender,
            pending_sync: AsyncMutex::new(HashMap::new()),
            pending_sync_conv: AsyncMutex::new(None),
        }
    }

    pub async fn handle_response(&self, resp: SyncRes) -> bool {
        if let Some(SyncResPayload::ConversationsIncremental(response)) = &resp.payload {
            let mut guard = self.pending_sync_conv.lock().await;
            if let Some(tx) = guard.take() {
                let _ = tx.send(response.clone());
            }
            return true;
        }
        if let Some(key) = pending_key_from_resp(&resp) {
            if let Some(tx) = self.pop_pending_sync(&key).await {
                let _ = tx.send(resp);
            }
            return true;
        }
        false
    }

    pub async fn request_single_page(
        &self,
        conversation_id: &str,
        last_seq: u64,
        limit: i32,
        cursor: String,
    ) -> Result<SyncRes> {
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
        self.request_sync_response(
            format!("single:{conversation_id}"),
            sync_event,
            Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            "single conversation timeout or canceled",
        )
        .await
    }

    pub async fn request_query_events(&self, req: QueryEventsSync) -> Result<SyncRes> {
        let sync_event = SyncWire {
            kind: SyncKind::QueryEvents as i32,
            device_id: String::new(),
            payload: Some(SyncPayload::QueryEvents(req)),
        };
        self.request_sync_response(
            "query_events".to_string(),
            sync_event,
            Duration::from_secs(QUERY_EVENTS_TIMEOUT_SECS),
            "sdk.sync.query_events.timeout_or_canceled",
        )
        .await
    }

    pub async fn request_get_cursor(&self, req: GetSyncCursorSync) -> Result<SyncRes> {
        let sync_event = SyncWire {
            kind: SyncKind::GetSyncCursor as i32,
            device_id: String::new(),
            payload: Some(SyncPayload::GetSyncCursor(req)),
        };
        self.request_sync_response(
            "get_cursor".to_string(),
            sync_event,
            Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            "get cursor timeout or canceled",
        )
        .await
    }

    pub async fn request_update_cursor(&self, req: UpdateSyncCursorSync) -> Result<SyncRes> {
        let sync_event = SyncWire {
            kind: SyncKind::UpdateSyncCursor as i32,
            device_id: String::new(),
            payload: Some(SyncPayload::UpdateSyncCursor(req)),
        };
        self.request_sync_response(
            "update_cursor".to_string(),
            sync_event,
            Duration::from_secs(UPDATE_CURSOR_TIMEOUT_SECS),
            "update cursor timeout or canceled",
        )
        .await
    }

    pub async fn request_conversations_incremental(
        &self,
        cursor: Option<prost_types::Timestamp>,
        limit: i32,
    ) -> Result<ConversationsIncrementalSyncRes> {
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
                    client_conversation_cursor: cursor,
                    limit,
                },
            )),
        };
        self.sender.send_sync(&sync_event).await?;
        match tokio::time::timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS), rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(FlareError::general_error(
                "conversations incremental channel closed".to_string(),
            )),
            Err(_) => Err(FlareError::general_error(
                "conversations incremental timeout or canceled".to_string(),
            )),
        }
    }

    async fn request_sync_response(
        &self,
        pending_key: String,
        sync_event: SyncWire,
        timeout_duration: Duration,
        timeout_message: &str,
    ) -> Result<SyncRes> {
        let (tx, rx) = oneshot::channel();
        self.push_pending_sync(&pending_key, tx).await;
        self.sender.send_sync(&sync_event).await?;
        match tokio::time::timeout(timeout_duration, rx).await {
            Ok(Ok(resp)) => Ok(resp),
            _ => Err(FlareError::general_error(timeout_message.to_string())),
        }
    }

    async fn push_pending_sync(&self, key: &str, tx: oneshot::Sender<SyncRes>) {
        let mut guard = self.pending_sync.lock().await;
        let queue = guard.entry(key.to_string()).or_insert_with(VecDeque::new);
        queue.retain(|sender| !sender.is_closed());
        queue.push_back(tx);
    }

    async fn pop_pending_sync(&self, key: &str) -> Option<oneshot::Sender<SyncRes>> {
        let mut guard = self.pending_sync.lock().await;
        let queue = guard.get_mut(key)?;
        queue.retain(|sender| !sender.is_closed());
        let tx = queue.pop_front();
        if queue.is_empty() {
            guard.remove(key);
        }
        tx
    }
}

fn pending_key_from_resp(resp: &SyncRes) -> Option<String> {
    match &resp.payload {
        Some(SyncResPayload::SingleConversation(response)) => {
            Some(format!("single:{}", response.conversation_id))
        }
        Some(SyncResPayload::QueryEvents(_)) => Some("query_events".to_string()),
        Some(SyncResPayload::GetSyncCursor(_)) => Some("get_cursor".to_string()),
        Some(SyncResPayload::UpdateSyncCursor(_)) => Some("update_cursor".to_string()),
        _ => None,
    }
}
