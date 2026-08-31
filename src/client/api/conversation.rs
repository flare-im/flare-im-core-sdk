//! 会话 Facade — 直接编排会话读写 usecase，并在关键写操作后发布会话域事件。
//!
//! **边界**：会话列表、置顶、免打扰、归档、已读/未读、草稿均属 IM core 域；
//! Social SDK 仅负责好友/群 REST 与 IM 资料投影，不得在此重复实现会话逻辑。

use std::sync::Arc;

use crate::application::usecases::{
    ConversationCommandUseCase, ConversationViewAssembler, MessageViewAssembler,
};
use crate::kernel::CurrentUserIdStore;
use crate::kernel::event::{ConversationEvent, EventBus, SdkEvent};
use crate::model::conversation::ConversationType;
use crate::model::{
    BootstrapHomeTimelineRequest, Conversation, ConversationListQuery,
    ConversationTimelineSnapshot, HomeTimelineSnapshot, OpenConversationTimelineRequest,
    TimelineSyncState, normalized_conversation_limit, normalized_message_limit,
};
use crate::shared::error::{ErrorCode, FlareError, Result};

/// 会话命令与查询入口（直接委托 application usecases；部分写操作经 `EventBus` 推事件）。
#[derive(Clone)]
pub struct ConversationApi {
    command_use_case: Arc<ConversationCommandUseCase>,
    view_assembler: Arc<ConversationViewAssembler>,
    message_view_assembler: Arc<MessageViewAssembler>,
    bus: EventBus,
    current_user_id: CurrentUserIdStore,
}

impl ConversationApi {
    pub fn new(
        command_use_case: Arc<ConversationCommandUseCase>,
        view_assembler: Arc<ConversationViewAssembler>,
        message_view_assembler: Arc<MessageViewAssembler>,
        bus: EventBus,
        current_user_id: CurrentUserIdStore,
    ) -> Self {
        Self {
            command_use_case,
            view_assembler,
            message_view_assembler,
            bus,
            current_user_id,
        }
    }

    async fn ensure_session_active(&self) -> Result<()> {
        if self.current_user_id.read().await.trim().is_empty() {
            return Err(FlareError::localized(ErrorCode::NotConnected, "未连接"));
        }
        Ok(())
    }

    pub async fn current_user_id(&self) -> Result<String> {
        self.command_use_case.current_user_id().await
    }

    /// 获取或创建单个会话：有则返回，无则创建并落库后返回，保证 list() 能立即看到新会话。
    /// 新建时 `channel_id` 恒为入参 `source_id`（与类型无关）。
    pub async fn get_one(
        &self,
        source_id: &str,
        conversation_type: &ConversationType,
    ) -> Result<Conversation> {
        self.ensure_session_active().await?;
        let conversation = self
            .command_use_case
            .get_one(source_id, conversation_type, true)
            .await?;
        Ok(self.view_assembler.hydrate_conversation(conversation).await)
    }

    /// 通过用户 ID 列表获取或创建群聊。本机会话会保存成员 ID，发送群消息时用于服务端补齐参与者。
    pub async fn get_group_by_user_ids(
        &self,
        user_ids: &[String],
        display_name: Option<&str>,
    ) -> Result<Conversation> {
        self.ensure_session_active().await?;
        let conversation = self
            .command_use_case
            .get_group_by_user_ids(user_ids, display_name)
            .await?;
        Ok(self.view_assembler.hydrate_conversation(conversation).await)
    }

    pub async fn list(&self) -> Result<Vec<Conversation>> {
        self.ensure_session_active().await?;
        self.view_assembler.list(false).await
    }

    /// 飞书式会话筛选：未读、@我、单聊/群聊、置顶、免打扰、归档、草稿、标记消息所在会话等。
    pub async fn list_by_query(&self, query: ConversationListQuery) -> Result<Vec<Conversation>> {
        self.ensure_session_active().await?;
        self.view_assembler.list_by_query(&query).await
    }

    /// 含已归档会话的完整列表（飞书「已完成」视图等）
    pub async fn list_including_archived(&self) -> Result<Vec<Conversation>> {
        self.ensure_session_active().await?;
        self.view_assembler.list(true).await
    }

    pub async fn get(&self, conversation_id: &str) -> Result<Option<Conversation>> {
        self.ensure_session_active().await?;
        self.view_assembler.get(conversation_id).await
    }

    /// 批量获取会话（按 id 列表，不存在的跳过）
    pub async fn get_multiple(&self, conversation_ids: &[String]) -> Result<Vec<Conversation>> {
        self.ensure_session_active().await?;
        self.view_assembler.get_multiple(conversation_ids).await
    }

    /// 分页会话列表（cursor 为会话 id 表示从此之后开始，limit 为条数）
    pub async fn list_paginated(
        &self,
        cursor: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<Conversation>> {
        self.ensure_session_active().await?;
        self.view_assembler
            .list_paginated(cursor, limit, false)
            .await
    }

    pub async fn list_raw(&self) -> Result<Vec<Conversation>> {
        self.ensure_session_active().await?;
        self.view_assembler.list_raw().await
    }

    /// SDK 统一首页快照：会话查询会在仓储层修复本地消息投影，业务端无需自行回写 lastMessage。
    pub async fn bootstrap_home(
        &self,
        request: BootstrapHomeTimelineRequest,
    ) -> Result<HomeTimelineSnapshot> {
        self.ensure_session_active().await?;
        let mut conversations = self.view_assembler.list(false).await?;
        conversations.truncate(normalized_conversation_limit(request.conversation_limit) as usize);
        let total_unread = conversations
            .iter()
            .map(|conversation| conversation.unread_count as u64)
            .sum();
        Ok(HomeTimelineSnapshot {
            conversations,
            total_unread,
            sync_state: TimelineSyncState::LocalReady,
        })
    }

    /// SDK 统一会话快照：读取最近消息窗口，并依赖 conversation.get 的投影修复返回一致会话摘要。
    pub async fn open_timeline(
        &self,
        request: OpenConversationTimelineRequest,
    ) -> Result<ConversationTimelineSnapshot> {
        self.ensure_session_active().await?;
        let conversation_id = request.conversation_id.trim();
        let limit = normalized_message_limit(request.message_limit);
        if conversation_id.is_empty() {
            return Ok(ConversationTimelineSnapshot {
                conversation: None,
                messages: Vec::new(),
                has_more: false,
            });
        }
        let mut messages = self
            .message_view_assembler
            .list(conversation_id, 0, limit)
            .await?;
        messages.sort_by(crate::model::IMMessage::compare_for_timeline_asc);
        let conversation = self.view_assembler.get(conversation_id).await?;
        // has_more 不能只看"本地这一页装满没有"。
        //
        // list() 读的是本地库，新设备/清过缓存的客户端本地可能只有几十条，
        // 于是 len() < limit 就报 has_more=false，UI 直接显示"没有更多消息了"，
        // 用户再也翻不到历史——而服务端可能有上百万条。load_older 本身有
        // request_message_backfill_before_seq 的回填能力，只是永远没机会被调到。
        //
        // 真正的判据是"最旧的那条是不是已经到了会话起点"：没到就说明更早的
        // 还在服务端。宁可多放行一次（回填拉回空页后 has_more 自然转 false），
        // 也不能把用户永久挡在历史之外。
        let reached_start = messages
            .first()
            .map(|first| first.conversation_seq <= Self::TIMELINE_FIRST_SEQ)
            .unwrap_or(true);
        let has_more = messages.len() >= limit as usize || !reached_start;
        Ok(ConversationTimelineSnapshot {
            conversation,
            messages,
            has_more,
        })
    }

    /// 会话内第一条消息的 seq。服务端从 1 开始分配，因此本地最旧一条的 seq
    /// 若大于它，就说明更早的历史还没同步下来。
    const TIMELINE_FIRST_SEQ: u64 = 1;

    pub(crate) async fn hydrate_timeline_messages(
        &self,
        messages: &mut [crate::model::IMMessage],
    ) -> Result<()> {
        self.message_view_assembler
            .hydrate_messages_for_view(messages)
            .await
    }

    pub(crate) async fn timeline_page(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<crate::model::IMMessage>> {
        self.ensure_session_active().await?;
        let conversation_id = conversation_id.trim();
        if conversation_id.is_empty() {
            return Ok(Vec::new());
        }
        self.message_view_assembler
            .list(conversation_id, before_seq, normalized_message_limit(limit))
            .await
    }

    pub async fn mark_read(&self, conversation_id: &str, read_seq: u64) -> Result<()> {
        self.ensure_session_active().await?;
        let unread_count = self
            .command_use_case
            .mark_read(conversation_id, read_seq)
            .await?;
        self.bus.publish(SdkEvent::Conversation(
            ConversationEvent::UnreadCountChanged {
                conversation_id: conversation_id.to_string(),
                unread_count,
            },
        ));
        Ok(())
    }

    pub async fn delete(&self, conversation_id: &str) -> Result<()> {
        self.ensure_session_active().await?;
        self.command_use_case.delete(conversation_id).await?;
        self.bus
            .publish(SdkEvent::Conversation(ConversationEvent::Deleted {
                conversation_id: conversation_id.to_string(),
            }));
        Ok(())
    }

    pub async fn set_pinned(&self, conversation_id: &str, pinned: bool) -> Result<()> {
        self.ensure_session_active().await?;
        self.command_use_case
            .set_pinned(conversation_id, pinned)
            .await?;
        self.publish_updated(conversation_id);
        Ok(())
    }

    pub async fn set_muted(&self, conversation_id: &str, muted: bool) -> Result<()> {
        self.ensure_session_active().await?;
        self.command_use_case
            .set_muted(conversation_id, muted)
            .await?;
        self.publish_updated(conversation_id);
        Ok(())
    }

    pub async fn set_archived(&self, conversation_id: &str, archived: bool) -> Result<()> {
        self.ensure_session_active().await?;
        self.command_use_case
            .set_archived(conversation_id, archived)
            .await?;
        self.publish_updated(conversation_id);
        Ok(())
    }

    pub async fn mark_unread(&self, conversation_id: &str) -> Result<u32> {
        self.ensure_session_active().await?;
        let unread_count = self.command_use_case.mark_unread(conversation_id).await?;
        self.bus.publish(SdkEvent::Conversation(
            ConversationEvent::UnreadCountChanged {
                conversation_id: conversation_id.to_string(),
                unread_count,
            },
        ));
        Ok(unread_count)
    }

    /// 设置会话草稿（本地）
    pub async fn update_draft(&self, conversation_id: &str, draft: Option<&str>) -> Result<()> {
        self.ensure_session_active().await?;
        self.command_use_case
            .update_draft(conversation_id, draft)
            .await?;
        self.publish_updated(conversation_id);
        Ok(())
    }

    /// 清空本地聊天记录并更新会话摘要。
    pub async fn clear_local_chat_history(&self, conversation_id: &str) -> Result<()> {
        self.ensure_session_active().await?;
        self.command_use_case
            .clear_local_chat_history(conversation_id)
            .await?;
        self.publish_updated(conversation_id);
        Ok(())
    }

    fn publish_updated(&self, conversation_id: &str) {
        self.bus
            .publish(SdkEvent::Conversation(ConversationEvent::Updated {
                conversation_id: conversation_id.to_string(),
            }));
    }
}

#[cfg(test)]
mod timeline_has_more_tests {
    use super::*;

    /// 复刻 open_timeline 里的判据。直接调 open_timeline 需要活的会话与本地库，
    /// 而这里要锁的是"怎么算 has_more"这条规则本身。
    fn has_more(loaded: usize, limit: usize, oldest_seq: Option<u64>) -> bool {
        let reached_start = oldest_seq
            .map(|seq| seq <= ConversationApi::TIMELINE_FIRST_SEQ)
            .unwrap_or(true);
        loaded >= limit || !reached_start
    }

    /// 本地只同步了一小段时，绝不能报"没有更多"。
    ///
    /// 这是修复前的真实故障：服务端 100 万条，新设备本地只有 36 条，
    /// len() < limit 就判 has_more=false，UI 显示"没有更多消息了"，
    /// 用户永远翻不到历史——而 load_older 的服务端回填能力一次都没被调用。
    #[test]
    fn partial_local_window_still_reports_more_history() {
        assert!(
            has_more(36, 40, Some(999_965)),
            "本地最旧一条 seq=999965 远未到会话起点，必须放行回填"
        );
    }

    /// 真到起点了才收口，否则用户会看到永远转不完的"加载更多"。
    #[test]
    fn reaching_first_seq_stops_paging() {
        assert!(
            !has_more(36, 40, Some(1)),
            "最旧一条已是 seq=1，确实没有更早的了"
        );
    }

    /// 空会话不该声称还有历史。
    #[test]
    fn empty_timeline_has_no_more() {
        assert!(!has_more(0, 40, None));
    }

    /// 页装满时无条件放行——这是修复前唯一生效的分支，不能回归。
    #[test]
    fn full_page_still_reports_more() {
        assert!(has_more(40, 40, Some(1)), "整页装满说明本地还有更多可翻");
    }
}
