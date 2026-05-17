use std::sync::Arc;
use std::time::Duration;

use flare_proto::common::{
    ConversationParticipantsSync, ConversationsSync, ConversationsSyncRes, GetSyncCursorSync,
    QueryEventsSync, SingleConversationSync, Sync as SyncWire, SyncKind, SyncRes,
    UpdateSyncCursorSync, sync::Payload as SyncPayload, sync_res::Payload as SyncResPayload,
};

use crate::domain::{
    CONVERSATIONS_SYNC_TIMEOUT_SECS, QUERY_EVENTS_TIMEOUT_SECS, UPDATE_CURSOR_TIMEOUT_SECS,
};
use crate::error::{FlareError, Result};
use crate::protocol::PacketSender;

const DEFAULT_TIMEOUT_SECS: u64 = 30;

fn sync_wire(kind: SyncKind, payload: SyncPayload) -> SyncWire {
    SyncWire {
        kind: kind as i32,
        device_id: String::new(),
        payload: Some(payload),
        ..Default::default()
    }
}

pub struct SyncRequestUseCase {
    sender: Arc<PacketSender>,
}

impl SyncRequestUseCase {
    pub fn new(sender: Arc<PacketSender>) -> Self {
        Self { sender }
    }

    /// 推送路径兜底：服务端主动下发 SyncRes 时仍可被消费（主路径为 `send_sync_and_wait`）。
    pub async fn handle_response(&self, resp: SyncRes) -> bool {
        let _ = resp;
        false
    }

    pub async fn request_single_page(
        &self,
        conversation_id: &str,
        last_seq: u64,
        limit: i32,
        cursor: String,
    ) -> Result<SyncRes> {
        let sync_event = sync_wire(
            SyncKind::SingleConversation,
            SyncPayload::SingleConversation(SingleConversationSync {
                conversation_id: conversation_id.to_string(),
                max_seq: last_seq,
                cursor,
                limit,
                ..Default::default()
            }),
        );
        self.sender
            .send_sync_and_wait(&sync_event, Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .await
    }

    pub async fn request_query_events(&self, req: QueryEventsSync) -> Result<SyncRes> {
        let sync_event = sync_wire(SyncKind::QueryEvents, SyncPayload::QueryEvents(req));
        self.sender
            .send_sync_and_wait(&sync_event, Duration::from_secs(QUERY_EVENTS_TIMEOUT_SECS))
            .await
    }

    pub async fn request_conversation_participants(
        &self,
        req: ConversationParticipantsSync,
    ) -> Result<SyncRes> {
        let sync_event = sync_wire(
            SyncKind::ConversationParticipants,
            SyncPayload::ConversationParticipants(req),
        );
        self.sender
            .send_sync_and_wait(&sync_event, Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .await
    }

    pub async fn request_get_cursor(&self, req: GetSyncCursorSync) -> Result<SyncRes> {
        let sync_event = sync_wire(SyncKind::GetSyncCursor, SyncPayload::GetSyncCursor(req));
        self.sender
            .send_sync_and_wait(&sync_event, Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .await
    }

    pub async fn request_update_cursor(&self, req: UpdateSyncCursorSync) -> Result<SyncRes> {
        let sync_event = sync_wire(
            SyncKind::UpdateSyncCursor,
            SyncPayload::UpdateSyncCursor(req),
        );
        self.sender
            .send_sync_and_wait(&sync_event, Duration::from_secs(UPDATE_CURSOR_TIMEOUT_SECS))
            .await
    }

    pub async fn request_conversations(
        &self,
        cursor: Option<prost_types::Timestamp>,
        limit: i32,
    ) -> Result<ConversationsSyncRes> {
        let sync_event = sync_wire(
            SyncKind::Conversations,
            SyncPayload::Conversations(ConversationsSync {
                client_conversation_cursor: cursor,
                limit,
                ..Default::default()
            }),
        );
        let resp = self
            .sender
            .send_sync_and_wait(
                &sync_event,
                Duration::from_secs(CONVERSATIONS_SYNC_TIMEOUT_SECS),
            )
            .await?;
        match resp.payload {
            Some(SyncResPayload::Conversations(response)) => Ok(response),
            _ => Err(FlareError::general_error(
                "unexpected conversations sync response".to_string(),
            )),
        }
    }
}
