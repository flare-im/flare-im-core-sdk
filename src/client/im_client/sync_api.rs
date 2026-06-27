use crate::FlareError;
use crate::kernel::SyncRunContext;
use crate::model::{
    ConversationVersion, SyncConversationSummariesRequest, SyncConversationSummariesResponse,
};
use crate::shared::error::{ErrorCode, Result};

use super::IMClient;

impl IMClient {
    /// 触发指定会话的增量消息同步（从服务端拉取该会话最新数据）。
    #[tracing::instrument(skip(self), fields(conversation_id = %conversation_id))]
    pub async fn sync_conversation(&self, conversation_id: &str) -> Result<()> {
        let runner = self
            .with_engine_async(|e| e.session_sync_runner())
            .await?
            .ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未配置同步"))?;
        runner.request_message_sync(conversation_id).await
    }

    /// 静默拉取会话列表摘要（多端补偿，不阻塞 UI）。
    pub async fn sync_conversation_summaries_silent(&self) -> Result<()> {
        let sync = self
            .with_engine_async(|engine| engine.conversation_summary_sync())
            .await?
            .ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未配置同步"))?;
        let user_id = self.current_user_id().await.unwrap_or_default();
        if user_id.is_empty() {
            return Err(FlareError::localized(ErrorCode::NotConnected, "未连接"));
        }
        sync.sync_conversation_summaries(
            &user_id,
            SyncRunContext::silent_multidevice_private_data(),
        )
        .await
    }

    /// 静默拉取会话列表摘要，并返回调用方缺失或版本落后的会话。
    pub async fn sync_conversation_summaries_with_versions(
        &self,
        request: SyncConversationSummariesRequest,
    ) -> Result<SyncConversationSummariesResponse> {
        self.sync_conversation_summaries_silent().await?;
        let conversations = self.conversation_async().await?.list_raw().await?;
        let current_versions = conversations
            .into_iter()
            .map(|conversation| ConversationVersion {
                conversation_id: conversation.conversation_id,
                version: conversation.version,
            });

        Ok(SyncConversationSummariesResponse::from_current_versions(
            &request.known_versions,
            current_versions,
        ))
    }

    /// 按 task id 静默触发 Background 同步任务。
    pub async fn spawn_background_sync_tasks(&self, task_ids: &[&str]) -> Result<()> {
        let (sync_manager, store, bus) = self
            .with_engine_async(|engine| {
                (
                    engine.sync_manager(),
                    engine.stores().clone(),
                    engine.bus().clone(),
                )
            })
            .await?;
        let user_id = self.current_user_id().await.unwrap_or_default();
        if user_id.is_empty() {
            return Err(FlareError::localized(ErrorCode::NotConnected, "未连接"));
        }
        sync_manager.spawn_background_tasks_by_ids(&user_id, task_ids, store, bus);
        Ok(())
    }

    pub async fn sync_conversation_participants(
        &self,
        conversation_id: &str,
        limit: i32,
    ) -> Result<Vec<crate::model::ConversationParticipant>> {
        let runner = self
            .with_engine_async(|e| e.session_sync_runner())
            .await?
            .ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未配置同步"))?;
        runner
            .request_participants_sync(conversation_id, limit)
            .await
    }

    /// 从指定序列号开始拉取会话消息。
    ///
    /// `last_seq` 为客户端已知游标，`limit` 为单次请求上限。
    #[tracing::instrument(skip(self), fields(conversation_id = %conversation_id, last_seq, limit))]
    pub async fn sync_messages(
        &self,
        conversation_id: &str,
        last_seq: u64,
        limit: i32,
    ) -> Result<()> {
        let runner = self
            .with_engine_async(|e| e.session_sync_runner())
            .await?
            .ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未配置同步"))?;
        runner
            .request_message_sync_from_seq(conversation_id, last_seq, limit)
            .await
    }

    /// 设置会话输入状态（typing/not typing）。
    pub async fn set_conversation_input_state(
        &self,
        conversation_id: &str,
        is_typing: bool,
    ) -> Result<()> {
        self.message_async()
            .await?
            .typing(conversation_id, is_typing)
            .await
    }
}
