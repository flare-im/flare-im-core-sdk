use std::sync::Arc;
use std::time::Duration;

use flare_proto::common::{
    ConversationParticipantsSync, ConversationUserSettingsSync, ConversationsSync,
    ConversationsSyncRes, EnsureConversationSync, GetSyncCursorSync, MultiConversationSync,
    QueryEventsSync, SingleConversationSync, Sync as SyncWire, SyncRes, SyncSnapshotSync,
    SyncSnapshotSyncRes, UpdateSyncCursorSync, sync::Payload as SyncPayload,
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
                before_conversation_seq: 0,
            }),
        );
        self.sender
            .send_sync_and_wait(&sync_event, Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .await
    }

    /// 历史回溯页：返回严格小于 `before_seq` 的最近 `limit` 条（方向经 proto 显式字段表达）。
    pub async fn request_backfill_page(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: i32,
    ) -> Result<SyncRes> {
        let sync_event = sync_wire(
            &self.device_id,
            SyncPayload::SingleConversation(SingleConversationSync {
                conversation_id: conversation_id.to_string(),
                after_conversation_seq: 0,
                cursor: String::new(),
                limit,
                before_conversation_seq: before_seq,
            }),
        );
        self.sender
            .send_sync_and_wait(&sync_event, Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .await
    }

    /// 批量拉取多会话增量（B3 catch-up：把 N 个变化会话合并为一次请求，O(会话)→O(1请求)）。
    pub async fn request_multi_conversation(
        &self,
        conversation_ids: Vec<String>,
        last_conversation_seq_per_conversation: std::collections::HashMap<String, u64>,
        limit_per_conversation: i32,
    ) -> Result<SyncRes> {
        let sync_event = sync_wire(
            &self.device_id,
            SyncPayload::MultiConversation(MultiConversationSync {
                conversation_ids,
                last_conversation_seq_per_conversation,
                limit_per_conversation,
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

    /// 显式建群：把整张成员表一次性交服务端建群,之后消息不再携带成员表。
    pub async fn request_ensure_conversation(
        &self,
        req: EnsureConversationSync,
    ) -> Result<SyncRes> {
        let sync_event = sync_wire(&self.device_id, SyncPayload::EnsureConversation(req));
        self.sender
            .send_sync_and_wait(&sync_event, Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .await
    }

    /// 快照重建（I2 gap-too-large 切换）：增量窗口过期/缺口不可回放时，放弃回放改拉状态快照。
    pub async fn request_sync_snapshot(
        &self,
        req: SyncSnapshotSync,
    ) -> Result<SyncSnapshotSyncRes> {
        let sync_event = sync_wire(&self.device_id, SyncPayload::SyncSnapshot(req));
        let resp = self
            .sender
            .send_sync_and_wait(
                &sync_event,
                Duration::from_secs(CONVERSATIONS_SYNC_TIMEOUT_SECS),
            )
            .await?;
        match resp.payload {
            Some(SyncResPayload::SyncSnapshotRes(response)) => Ok(response),
            _ => Err(FlareError::general_error(
                "unexpected sync snapshot response".to_string(),
            )),
        }
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
