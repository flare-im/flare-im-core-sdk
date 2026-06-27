use std::sync::Arc;
use std::time::Duration;

use flare_proto::common::{
    ConversationParticipantsSync, ConversationUserSettingsSync, ConversationsSync,
    ConversationsSyncRes, GetSyncCursorSync, QueryEventsSync, SingleConversationSync,
    Sync as SyncWire, SyncRes, UpdateSyncCursorSync, sync::Payload as SyncPayload,
    sync_res::Payload as SyncResPayload,
};

use crate::domain::{
    CONVERSATIONS_SYNC_TIMEOUT_SECS, GET_SYNC_CURSOR_TIMEOUT_SECS, QUERY_EVENTS_TIMEOUT_SECS,
    UPDATE_CURSOR_TIMEOUT_SECS,
};
use crate::infrastructure::protocol::PacketSender;
use crate::shared::error::{FlareError, Result};

const DEFAULT_TIMEOUT_SECS: u64 = 30;

fn sync_wire(device_id: &str, payload: SyncPayload) -> SyncWire {
    SyncWire {
        device_id: device_id.to_string(),
        payload: Some(payload),
    }
}

pub struct SyncRequestUseCase {
    sender: Arc<PacketSender>,
    device_id: String,
}

impl SyncRequestUseCase {
    pub fn new(sender: Arc<PacketSender>, device_id: impl Into<String>) -> Self {
        Self {
            sender,
            device_id: device_id.into(),
        }
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
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
            &self.device_id,
            SyncPayload::SingleConversation(SingleConversationSync {
                conversation_id: conversation_id.to_string(),
                after_conversation_seq: last_seq,
                cursor,
                limit,
            }),
        );
        self.sender
            .send_sync_and_wait(&sync_event, Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .await
    }

    pub async fn request_query_events(&self, req: QueryEventsSync) -> Result<SyncRes> {
        let sync_event = sync_wire(&self.device_id, SyncPayload::QueryEvents(req));
        self.sender
            .send_sync_and_wait(&sync_event, Duration::from_secs(QUERY_EVENTS_TIMEOUT_SECS))
            .await
    }

    pub async fn request_conversation_participants(
        &self,
        req: ConversationParticipantsSync,
    ) -> Result<SyncRes> {
        let sync_event = sync_wire(&self.device_id, SyncPayload::ConversationParticipants(req));
        self.sender
            .send_sync_and_wait(&sync_event, Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .await
    }

    pub async fn request_get_cursor(&self, req: GetSyncCursorSync) -> Result<SyncRes> {
        let sync_event = sync_wire(&self.device_id, SyncPayload::GetSyncCursor(req));
        self.sender
            .send_sync_and_wait(
                &sync_event,
                Duration::from_secs(GET_SYNC_CURSOR_TIMEOUT_SECS),
            )
            .await
    }

    pub async fn request_update_cursor(&self, req: UpdateSyncCursorSync) -> Result<SyncRes> {
        let sync_event = sync_wire(&self.device_id, SyncPayload::UpdateSyncCursor(req));
        self.sender
            .send_sync_and_wait(&sync_event, Duration::from_secs(UPDATE_CURSOR_TIMEOUT_SECS))
            .await
    }

    pub async fn request_conversation_user_settings(
        &self,
        req: ConversationUserSettingsSync,
    ) -> Result<SyncRes> {
        let sync_event = sync_wire(&self.device_id, SyncPayload::ConversationUserSettings(req));
        self.sender
            .send_sync_and_wait(&sync_event, Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .await
    }

    pub async fn request_conversations(
        &self,
        cursor: String,
        limit: i32,
    ) -> Result<ConversationsSyncRes> {
        let sync_event = sync_wire(
            &self.device_id,
            SyncPayload::Conversations(ConversationsSync {
                cursor,
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
