//! 会话 Facade — 直接编排会话读写 usecase，并在关键写操作后发布会话域事件。

use std::sync::Arc;

use crate::application::usecases::{ConversationCommandUseCase, ConversationViewAssembler};
use crate::error::Result;
use crate::event::{ConversationEvent, EventBus, SdkEvent};
use crate::model::Conversation;
use crate::model::conversation::ConversationType;

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
        self.view_assembler.list().await
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
        self.view_assembler.list_paginated(cursor, limit).await
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
            .await
    }

    /// 设置会话草稿（本地）
    pub async fn update_draft(&self, conversation_id: &str, draft: Option<&str>) -> Result<()> {
        self.command_use_case
            .update_draft(conversation_id, draft)
            .await
    }
}
