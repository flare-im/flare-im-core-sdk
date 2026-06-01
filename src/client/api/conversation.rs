//! 会话 Facade — 直接编排会话读写 usecase，并在关键写操作后发布会话域事件。
//!
//! **边界**：会话列表、置顶、免打扰、归档、已读/未读、草稿均属 IM core 域；
//! Social SDK 仅负责好友/群 REST 与 IM 资料投影，不得在此重复实现会话逻辑。

use std::sync::Arc;

use crate::application::usecases::{ConversationCommandUseCase, ConversationViewAssembler};
use crate::error::Result;
use crate::event::{ConversationEvent, EventBus, SdkEvent};
use crate::model::conversation::ConversationType;
use crate::model::{Conversation, ConversationListQuery};

/// 会话命令与查询入口（直接委托 application usecases；部分写操作经 `EventBus` 推事件）。
#[derive(Clone)]
pub struct ConversationApi {
    command_use_case: Arc<ConversationCommandUseCase>,
    view_assembler: Arc<ConversationViewAssembler>,
    bus: EventBus,
}

impl ConversationApi {
    pub fn new(
        command_use_case: Arc<ConversationCommandUseCase>,
        view_assembler: Arc<ConversationViewAssembler>,
        bus: EventBus,
    ) -> Self {
        Self {
            command_use_case,
            view_assembler,
            bus,
        }
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
        let conversation = self
            .command_use_case
            .get_group_by_user_ids(user_ids, display_name)
            .await?;
        Ok(self.view_assembler.hydrate_conversation(conversation).await)
    }

    pub async fn list(&self) -> Result<Vec<Conversation>> {
        self.view_assembler.list(false).await
    }

    /// 飞书式会话筛选：未读、@我、单聊/群聊、置顶、免打扰、归档、草稿、标记消息所在会话等。
    pub async fn list_by_query(&self, query: ConversationListQuery) -> Result<Vec<Conversation>> {
        self.view_assembler.list_by_query(&query).await
    }

    /// 含已归档会话的完整列表（飞书「已完成」视图等）
    pub async fn list_including_archived(&self) -> Result<Vec<Conversation>> {
        self.view_assembler.list(true).await
    }

    pub async fn get(&self, conversation_id: &str) -> Result<Option<Conversation>> {
        self.view_assembler.get(conversation_id).await
    }

    /// 批量获取会话（按 id 列表，不存在的跳过）
    pub async fn get_multiple(&self, conversation_ids: &[String]) -> Result<Vec<Conversation>> {
        self.view_assembler.get_multiple(conversation_ids).await
    }

    /// 分页会话列表（cursor 为会话 id 表示从此之后开始，limit 为条数）
    pub async fn list_paginated(
        &self,
        cursor: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<Conversation>> {
        self.view_assembler
            .list_paginated(cursor, limit, false)
            .await
    }

    pub async fn list_raw(&self) -> Result<Vec<Conversation>> {
        self.view_assembler.list_raw().await
    }

    pub async fn mark_read(&self, conversation_id: &str, read_seq: u64) -> Result<()> {
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

    pub async fn mark_all_read(&self) -> Result<()> {
        self.command_use_case.mark_all_read().await
    }

    pub async fn delete(&self, conversation_id: &str) -> Result<()> {
        self.command_use_case.delete(conversation_id).await?;
        self.bus
            .publish(SdkEvent::Conversation(ConversationEvent::Deleted {
                conversation_id: conversation_id.to_string(),
            }));
        Ok(())
    }

    pub async fn set_pinned(&self, conversation_id: &str, pinned: bool) -> Result<()> {
        self.command_use_case
            .set_pinned(conversation_id, pinned)
            .await?;
        self.publish_updated(conversation_id);
        Ok(())
    }

    pub async fn set_muted(&self, conversation_id: &str, muted: bool) -> Result<()> {
        self.command_use_case
            .set_muted(conversation_id, muted)
            .await?;
        self.publish_updated(conversation_id);
        Ok(())
    }

    pub async fn set_archived(&self, conversation_id: &str, archived: bool) -> Result<()> {
        self.command_use_case
            .set_archived(conversation_id, archived)
            .await?;
        self.publish_updated(conversation_id);
        Ok(())
    }

    pub async fn mark_unread(&self, conversation_id: &str) -> Result<u32> {
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
        self.command_use_case
            .update_draft(conversation_id, draft)
            .await?;
        self.publish_updated(conversation_id);
        Ok(())
    }

    /// 清空本地聊天记录并更新会话摘要。
    pub async fn clear_local_chat_history(&self, conversation_id: &str) -> Result<()> {
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
