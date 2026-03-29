//! 会话 Facade — 委托 [`crate::application::ConversationFlow`]，并在关键写操作后发布会话域事件。

use std::sync::Arc;

use crate::application::ConversationFlow;
use crate::error::Result;
use crate::event::{ConversationEvent, EventBus, SdkEvent};
use crate::model::Conversation;
use crate::model::conversation::ConversationType;

/// 会话命令与查询入口（逻辑在 `ConversationFlow`；部分写操作经 `EventBus` 推事件）。
#[derive(Clone)]
pub struct ConversationApi {
    flow: Arc<ConversationFlow>,
    bus: EventBus,
}

impl ConversationApi {
    pub fn new(flow: Arc<ConversationFlow>, bus: EventBus) -> Self {
        Self { flow, bus }
    }

    pub async fn current_user_id(&self) -> Result<String> {
        self.flow.current_user_id().await
    }

    /// 获取或创建单个会话：有则返回，无则创建并落库后返回，保证 list() 能立即看到新会话。
    /// 新建时 `channel_id` 恒为入参 `source_id`（与类型无关）。
    pub async fn get_one(
        &self,
        source_id: &str,
        conversation_type: &ConversationType,
    ) -> Result<Conversation> {
        self.flow.get_one(source_id, conversation_type, true).await
    }

    pub async fn list(&self) -> Result<Vec<Conversation>> {
        self.flow.list().await
    }

    pub async fn get(&self, conversation_id: &str) -> Result<Option<Conversation>> {
        self.flow.get(conversation_id).await
    }

    /// 批量获取会话（按 id 列表，不存在的跳过）
    pub async fn get_multiple(&self, conversation_ids: &[String]) -> Result<Vec<Conversation>> {
        self.flow.get_multiple(conversation_ids).await
    }

    /// 分页会话列表（cursor 为会话 id 表示从此之后开始，limit 为条数）
    pub async fn list_paginated(
        &self,
        cursor: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<Conversation>> {
        self.flow.list_paginated(cursor, limit).await
    }

    pub async fn list_raw(&self) -> Result<Vec<Conversation>> {
        self.flow.list_raw().await
    }

    pub async fn mark_read(&self, conversation_id: &str, read_seq: u64) -> Result<()> {
        self.flow.mark_read(conversation_id, read_seq).await?;
        self.bus.publish(SdkEvent::Conversation(
            ConversationEvent::UnreadCountChanged {
                conversation_id: conversation_id.to_string(),
                unread_count: 0,
            },
        ));
        Ok(())
    }

    pub async fn mark_all_read(&self) -> Result<()> {
        self.flow.mark_all_read().await
    }

    pub async fn delete(&self, conversation_id: &str) -> Result<()> {
        self.flow.delete(conversation_id).await?;
        self.bus
            .publish(SdkEvent::Conversation(ConversationEvent::Deleted {
                conversation_id: conversation_id.to_string(),
            }));
        Ok(())
    }

    pub async fn set_pinned(&self, conversation_id: &str, pinned: bool) -> Result<()> {
        self.flow.set_pinned(conversation_id, pinned).await
    }

    /// 设置会话草稿（本地）
    pub async fn update_draft(&self, conversation_id: &str, draft: Option<&str>) -> Result<()> {
        self.flow.update_draft(conversation_id, draft).await
    }
}
