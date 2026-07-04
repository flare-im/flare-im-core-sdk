use std::sync::atomic::Ordering;

use crate::FlareError;
use crate::kernel::SyncRunContext;
use crate::model::{
    BootstrapHomeTimelineRequest, ConversationHistoryBackfillResponse, ConversationVersion,
    StartupHomeSyncRequest, StartupHomeSyncResponse, SyncConversationSummariesRequest,
    SyncConversationSummariesResponse, TimelineSyncState,
};
use crate::shared::error::{ErrorCode, Result};

use super::{IMClient, spawn_im_background};

const DEFAULT_HISTORY_BACKFILL_LIMIT: i32 = 500;
const MAX_HISTORY_BACKFILL_LIMIT: i32 = 1000;
const DEFAULT_HISTORY_BACKFILL_MAX_PAGES: u32 = 128;
const MAX_HISTORY_BACKFILL_MAX_PAGES: u32 = 512;
const DEFAULT_STARTUP_BACKFILL_MAX_CONVERSATIONS: u32 = 100;
const MAX_STARTUP_BACKFILL_MAX_CONVERSATIONS: u32 = 500;

fn normalize_history_backfill_limit(limit: i32) -> i32 {
    let limit = if limit <= 0 {
        DEFAULT_HISTORY_BACKFILL_LIMIT
    } else {
        limit
    };
    limit.clamp(1, MAX_HISTORY_BACKFILL_LIMIT)
}

fn normalize_history_backfill_max_pages(max_pages: u32) -> u32 {
    if max_pages == 0 {
        DEFAULT_HISTORY_BACKFILL_MAX_PAGES
    } else {
        max_pages.min(MAX_HISTORY_BACKFILL_MAX_PAGES)
    }
}

fn normalize_startup_home_sync_request(
    mut request: StartupHomeSyncRequest,
) -> StartupHomeSyncRequest {
    request.conversation_limit =
        crate::model::normalized_conversation_limit(request.conversation_limit);
    request.history_backfill_limit =
        normalize_history_backfill_limit(request.history_backfill_limit);
    request.history_backfill_max_pages_per_conversation =
        normalize_history_backfill_max_pages(request.history_backfill_max_pages_per_conversation);
    request.history_backfill_max_conversations = if request.history_backfill_max_conversations == 0
    {
        DEFAULT_STARTUP_BACKFILL_MAX_CONVERSATIONS
    } else {
        request
            .history_backfill_max_conversations
            .min(MAX_STARTUP_BACKFILL_MAX_CONVERSATIONS)
    };
    request
}

impl IMClient {
    /// Bootstrap the first usable home screen from local projections, then let core-owned
    /// background convergence close any sync gaps.
    ///
    /// This method is the shared cold/hot-start path for all platform SDKs. It does not
    /// wait for full historical backfill before returning the first home snapshot.
    #[tracing::instrument(skip(self, request), fields(conversation_limit = request.conversation_limit))]
    pub async fn bootstrap_startup_home(
        &self,
        request: StartupHomeSyncRequest,
    ) -> Result<StartupHomeSyncResponse> {
        let request = normalize_startup_home_sync_request(request);
        let generation = self.load_session_generation_snapshot();
        let mut snapshot = self
            .conversation_async()
            .await?
            .bootstrap_home(BootstrapHomeTimelineRequest {
                conversation_limit: request.conversation_limit,
            })
            .await?;
        let served_from_local = !snapshot.conversations.is_empty();
        let mut cold_sync_performed = false;
        let mut degraded_reason = None;

        if !served_from_local {
            match self.sync_conversation_summaries_silent().await {
                Ok(()) => {
                    cold_sync_performed = true;
                    snapshot = self
                        .conversation_async()
                        .await?
                        .bootstrap_home(BootstrapHomeTimelineRequest {
                            conversation_limit: request.conversation_limit,
                        })
                        .await?;
                    snapshot.sync_state = TimelineSyncState::Synced;
                }
                Err(error) => {
                    snapshot.sync_state = TimelineSyncState::Partial;
                    degraded_reason = Some(error.to_string());
                    tracing::warn!(
                        error = %error,
                        "startup home cold summary sync failed; returning local snapshot"
                    );
                }
            }
        }

        let background_convergence_started =
            self.spawn_startup_home_convergence(request, generation);

        Ok(StartupHomeSyncResponse {
            snapshot,
            served_from_local,
            cold_sync_performed,
            background_convergence_started,
            degraded_reason,
        })
    }

    fn spawn_startup_home_convergence(
        &self,
        request: StartupHomeSyncRequest,
        generation: u64,
    ) -> bool {
        if !request.start_background_convergence {
            return false;
        }
        if self
            .startup_convergence_in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }

        let client = self.downgrade();
        spawn_im_background(async move {
            let Some(client) = client.upgrade() else {
                return;
            };
            client
                .run_startup_home_convergence(request, generation)
                .await;
            client
                .startup_convergence_in_flight
                .store(false, Ordering::Release);
        });
        true
    }

    async fn run_startup_home_convergence(&self, request: StartupHomeSyncRequest, generation: u64) {
        if !self.is_generation_current(generation).await {
            return;
        }
        if let Err(error) = self.sync_foreground_convergence_silent().await {
            tracing::debug!(
                session_generation = generation,
                error = %error,
                "startup foreground convergence failed"
            );
        }
        if !request.backfill_visible_histories || !self.is_generation_current(generation).await {
            return;
        }

        let conversations = match self.conversation_async().await {
            Ok(api) => match api.list_including_archived().await {
                Ok(conversations) => conversations,
                Err(error) => {
                    tracing::debug!(
                        session_generation = generation,
                        error = %error,
                        "startup history backfill conversation list failed"
                    );
                    return;
                }
            },
            Err(error) => {
                tracing::debug!(
                    session_generation = generation,
                    error = %error,
                    "startup history backfill conversation api unavailable"
                );
                return;
            }
        };

        for conversation in conversations
            .into_iter()
            .filter(|conversation| !conversation.conversation_id.trim().is_empty())
            .take(request.history_backfill_max_conversations as usize)
        {
            if !self.is_generation_current(generation).await {
                break;
            }
            if let Err(error) = self
                .backfill_conversation_history(
                    &conversation.conversation_id,
                    request.history_backfill_limit,
                    request.history_backfill_max_pages_per_conversation,
                )
                .await
            {
                tracing::debug!(
                    session_generation = generation,
                    conversation_id = %conversation.conversation_id,
                    error = %error,
                    "startup conversation history backfill failed"
                );
            }
        }
    }

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

    /// 打开会话时间线：升为前台，后续收敛优先补齐该会话（秒展示）。未连接时 no-op。
    /// 注意力仅为优先级 hint，非正确性关键——缺口修复保证最终完整。
    pub fn open_timeline(&self, conversation_id: &str) {
        let _ = self.with_engine(|engine| {
            engine
                .sync_manager()
                .attention()
                .open_timeline(conversation_id)
        });
    }

    /// 关闭会话时间线：若为当前前台则清空。未连接时 no-op。
    pub fn close_timeline(&self, conversation_id: &str) {
        let _ = self.with_engine(|engine| {
            engine
                .sync_manager()
                .attention()
                .close_timeline(conversation_id)
        });
    }

    /// 设置可见会话列表窗口（滑动时整窗替换）：这些会话在收敛中次于前台优先。未连接时 no-op。
    pub fn set_visible_conversations(&self, conversation_ids: &[String]) {
        let _ = self.with_engine(|engine| {
            engine
                .sync_manager()
                .attention()
                .set_visible(conversation_ids.iter().cloned())
        });
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

    /// 将单个会话的服务端历史消息补齐到本地消息库。
    ///
    /// 该方法不扩大当前 UI timeline view 的窗口，只复用同步协议写入本地 store，适合登录后
    /// 做跨端历史一致性补偿。
    #[tracing::instrument(skip(self), fields(conversation_id = %conversation_id, limit, max_pages))]
    pub async fn backfill_conversation_history(
        &self,
        conversation_id: &str,
        limit: i32,
        max_pages: u32,
    ) -> Result<ConversationHistoryBackfillResponse> {
        let conversation_id = conversation_id.trim();
        if conversation_id.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "conversationId must not be empty",
            ));
        }

        let limit = normalize_history_backfill_limit(limit);
        let max_pages = normalize_history_backfill_max_pages(max_pages);
        let (runner, stores) = self
            .with_engine_async(|engine| (engine.session_sync_runner(), engine.stores().clone()))
            .await?;
        let runner =
            runner.ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未配置同步"))?;

        let latest_sync_result = runner.request_message_sync(conversation_id).await;

        let oldest_before = stores
            .messages
            .oldest_conversation_seq(conversation_id)
            .await?
            .unwrap_or(0);
        if let Err(error) = latest_sync_result {
            let conversation = stores.conversations.get(conversation_id).await?;
            let expects_server_messages = conversation
                .as_ref()
                .map(|conversation| conversation.max_seq > conversation.visible_after_seq)
                .unwrap_or(false);
            if oldest_before == 0 && expects_server_messages {
                return Err(error);
            }
            tracing::warn!(
                conversation_id = %conversation_id,
                oldest_before,
                error = %error,
                "latest message sync failed before history backfill; continuing from local history"
            );
        }

        if oldest_before <= 1 {
            return Ok(ConversationHistoryBackfillResponse {
                conversation_id: conversation_id.to_string(),
                pages_loaded: 0,
                oldest_seq_before: oldest_before,
                oldest_seq_after: oldest_before,
                has_more: false,
                completed: true,
            });
        }

        let mut cursor = oldest_before;
        let mut pages_loaded = 0;
        let mut has_more = true;

        while pages_loaded < max_pages && cursor > 1 {
            has_more = match runner
                .request_message_backfill_before_seq(conversation_id, cursor, limit)
                .await
            {
                Ok(has_more) => has_more,
                Err(error) => {
                    tracing::warn!(
                        conversation_id = %conversation_id,
                        cursor,
                        error = %error,
                        "history backfill page request failed; returning partial progress"
                    );
                    break;
                }
            };
            let next_oldest = stores
                .messages
                .oldest_conversation_seq(conversation_id)
                .await?
                .unwrap_or(cursor);

            if next_oldest >= cursor {
                break;
            }

            pages_loaded += 1;
            cursor = next_oldest;

            if !has_more {
                break;
            }
        }

        let oldest_after = stores
            .messages
            .oldest_conversation_seq(conversation_id)
            .await?
            .unwrap_or(cursor);
        let completed = oldest_after <= 1 || !has_more;

        Ok(ConversationHistoryBackfillResponse {
            conversation_id: conversation_id.to_string(),
            pages_loaded,
            oldest_seq_before: oldest_before,
            oldest_seq_after: oldest_after,
            has_more,
            completed,
        })
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
