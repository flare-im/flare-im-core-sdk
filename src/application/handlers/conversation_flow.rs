//! 会话业务处理器（Conversation Handler）
//!
//! EventBus 下与 Sync 协同的一支：会话列表、未读、置顶、删除等；
//! 依赖 Query 与 Repository，不直接碰 Network，通过 Store 与 SyncManager 下推一致。

use std::sync::Arc;

use crate::application::handlers::ConversationQueryHandler;
use crate::application::queries::{GetConversationQuery, GetConversationsQuery};
use crate::conversation;
use crate::core::CurrentUserIdStore;
use crate::domain::UserReader;
use crate::error::{ErrorCode, FlareError, Result};
use crate::model::Conversation;
use crate::model::conversation::ConversationType;
use crate::store::ConversationStore;

pub struct ConversationFlow {
    pub(super) store: Arc<dyn ConversationStore>,
    pub(super) query_handler: Arc<ConversationQueryHandler>,
    pub(super) current_user_id: CurrentUserIdStore,
    pub(super) profile_reader: Arc<dyn UserReader>,
}

impl ConversationFlow {
    pub fn new(
        store: Arc<dyn ConversationStore>,
        query_handler: Arc<ConversationQueryHandler>,
        current_user_id: CurrentUserIdStore,
        profile_reader: Arc<dyn UserReader>,
    ) -> Self {
        Self {
            store,
            query_handler,
            current_user_id,
            profile_reader,
        }
    }

    pub async fn current_user_id(&self) -> Result<String> {
        let uid = self.current_user_id.read().await.clone();
        if uid.is_empty() {
            return Err(FlareError::localized(ErrorCode::NotConnected, "未连接"));
        }
        Ok(uid)
    }

    pub fn single_chat_id(&self, user1: &str, user2: &str) -> String {
        conversation::generate_single_chat_conversation_id(user1, user2)
    }

    pub fn group_id(&self, group_id: &str) -> String {
        conversation::generate_group_conversation_id(group_id)
    }

    pub fn ai_id(&self, user_id: &str, ai_scope: &str) -> String {
        conversation::generate_ai_conversation_id(user_id, ai_scope)
    }

    pub fn customer_id(&self, customer_id: &str, channel: &str) -> String {
        conversation::generate_customer_conversation_id(customer_id, channel)
    }

    pub fn system_id(&self, system_id: &str, scope: Option<&str>) -> String {
        conversation::generate_system_conversation_id(
            system_id,
            scope.map(std::string::ToString::to_string),
        )
    }

    pub fn temp_id(&self) -> String {
        conversation::generate_temp_conversation_id()
    }

    /// 按类型与源 ID 取会话：有则返回，无则创建并落库后返回。
    ///
    /// **新建**：`channel_id` 恒为本次传入的 `source_id`（与 `conversation_type` 无关：单聊为对端 user_id，群为群业务 id 等）。
    /// **已存在且 `channel_id` 为空**：用本次 `source_id` 写入并落库（若 `save`），便于同步未带齐路由字段时的修复。
    pub async fn get_one(
        &self,
        source_id: &str,
        conversation_type: &ConversationType,
        save: bool,
    ) -> Result<Conversation> {
        let uid = self.current_user_id().await?;
        let conversation_id = match conversation_type {
            ConversationType::Single => self.single_chat_id(&uid, source_id),
            ConversationType::Group => self.group_id(source_id),
            ConversationType::Ai => self.ai_id(&uid, source_id),
            ConversationType::Customer => self.customer_id(&uid, source_id),
            ConversationType::System => self.system_id(source_id, None),
            ConversationType::Temp => self.temp_id(),
            _ => {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "不支持的会话类型",
                ));
            }
        };
        let existing = self
            .query_handler
            .handle_get_conversation(GetConversationQuery {
                conversation_id: conversation_id.clone(),
            })
            .await?;

        let mut needs_persist = false;
        let mut conv = if let Some(mut conv) = existing {
            if conv.channel_id.is_empty() {
                conv.channel_id = source_id.to_string();
                needs_persist = true;
            }
            conv
        } else {
            needs_persist = true;
            let mut summary = Conversation::from_conversation_id(conversation_id);
            summary.conversation_type = conversation_type.clone();
            summary.channel_id = source_id.to_string();
            summary.display_name = source_id.to_string();
            summary.business_type = conversation_type.as_str().to_string();
            summary
        };

        if save && needs_persist {
            self.store.save_batch(&[conv.clone()]).await?;
        }

        if let Some(ref last) = conv.last_message() {
            if !last.sender_id.is_empty() {
                if let Ok(Some(profile)) = self.profile_reader.get(&last.sender_id).await {
                    conv = conv.with_last_sender(profile.display_name(), &profile.avatar_url);
                }
            }
        }
        Ok(conv)
    }
    /// 会话列表：置顶优先，再按 last_message_at 倒序
    pub async fn list(&self) -> Result<Vec<Conversation>> {
        let list: Vec<Conversation> = self
            .query_handler
            .handle_get_conversations(GetConversationsQuery)
            .await?;
        let mut views = Vec::with_capacity(list.len());
        for mut conv in list {
            if let Some(ref last) = conv.last_message() {
                if !last.sender_id.is_empty() {
                    if let Ok(Some(profile)) = self.profile_reader.get(&last.sender_id).await {
                        conv = conv.with_last_sender(profile.display_name(), &profile.avatar_url);
                    }
                }
            }
            views.push(conv);
        }
        Ok(views)
    }

    pub async fn get(&self, conversation_id: &str) -> Result<Option<Conversation>> {
        let conv = self
            .query_handler
            .handle_get_conversation(GetConversationQuery {
                conversation_id: conversation_id.into(),
            })
            .await?;
        let Some(mut conv) = conv else {
            return Ok(None);
        };
        if let Some(ref last) = conv.last_message() {
            if !last.sender_id.is_empty() {
                if let Ok(Some(profile)) = self.profile_reader.get(&last.sender_id).await {
                    conv = conv.with_last_sender(profile.display_name(), &profile.avatar_url);
                }
            }
        }
        Ok(Some(conv))
    }

    /// 批量获取会话（按 id 列表，顺序与 ids 一致；不存在的跳过）
    pub async fn get_multiple(&self, conversation_ids: &[String]) -> Result<Vec<Conversation>> {
        let mut out = Vec::with_capacity(conversation_ids.len());
        for id in conversation_ids {
            let existing = self
                .query_handler
                .handle_get_conversation(GetConversationQuery {
                    conversation_id: id.clone(),
                })
                .await?;
            if let Some(mut conv) = existing {
                if let Some(ref last) = conv.last_message() {
                    if !last.sender_id.is_empty() {
                        if let Ok(Some(profile)) = self.profile_reader.get(&last.sender_id).await {
                            conv =
                                conv.with_last_sender(profile.display_name(), &profile.avatar_url);
                        }
                    }
                }
                out.push(conv);
            }
        }
        Ok(out)
    }

    /// 分页会话列表：先 list 再按 cursor（会话 id，表示从此 id 之后开始）与 limit 截取
    pub async fn list_paginated(
        &self,
        cursor: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<Conversation>> {
        let list = self.list().await?;
        let skip = cursor
            .and_then(|c| list.iter().position(|conv| conv.conversation_id == c))
            .map(|i| i + 1)
            .unwrap_or(0);
        let take = limit.map(|l| l as usize).unwrap_or(usize::MAX);
        Ok(list.into_iter().skip(skip).take(take).collect())
    }

    pub async fn list_raw(&self) -> Result<Vec<Conversation>> {
        self.query_handler
            .handle_get_conversations(GetConversationsQuery)
            .await
    }

    pub async fn mark_read(&self, conversation_id: &str, read_seq: u64) -> Result<()> {
        self.store.update_unread(conversation_id, 0, read_seq).await
    }

    pub async fn mark_all_read(&self) -> Result<()> {
        let list = self.list().await?;
        for c in list {
            let _ = self.mark_read(c.conversation_id(), c.max_seq()).await;
        }
        Ok(())
    }

    pub async fn delete(&self, conversation_id: &str) -> Result<()> {
        self.store.delete(conversation_id).await
    }

    pub async fn set_pinned(&self, conversation_id: &str, pinned: bool) -> Result<()> {
        self.store.set_pinned(conversation_id, pinned).await
    }

    /// 更新会话草稿（本地）
    pub async fn update_draft(&self, conversation_id: &str, draft: Option<&str>) -> Result<()> {
        self.store.update_draft(conversation_id, draft).await
    }

    pub async fn ensure_local_conversation(
        &self,
        conversation_id: &str,
        display_name: Option<&str>,
        conversation_type: ConversationType,
        business_type: &str,
        channel_id: String,
    ) -> Result<()> {
        if self.get(conversation_id).await?.is_some() {
            return Ok(());
        }
        let mut summary = Conversation::from_conversation_id(conversation_id.to_string());
        summary.conversation_type = conversation_type;
        summary.business_type = business_type.to_string();
        summary.display_name = display_name.unwrap_or(channel_id.as_str()).to_string();
        summary.channel_id = channel_id;
        self.store.save_batch(&[summary]).await?;
        Ok(())
    }
}
