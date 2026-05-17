use std::sync::Arc;

use crate::core::CurrentUserIdStore;
use crate::domain::{ConversationIdentityService, ConversationReadService, ConversationStore};
use crate::error::{ErrorCode, FlareError, Result};
use crate::model::conversation::ConversationType;
use crate::model::{Conversation, ConversationParticipant};

pub struct ConversationCommandUseCase {
    store: Arc<dyn ConversationStore>,
    current_user_id: CurrentUserIdStore,
    identity_service: ConversationIdentityService,
    read_service: ConversationReadService,
}

impl ConversationCommandUseCase {
    pub fn new(store: Arc<dyn ConversationStore>, current_user_id: CurrentUserIdStore) -> Self {
        Self {
            store,
            current_user_id,
            identity_service: ConversationIdentityService,
            read_service: ConversationReadService,
        }
    }

    pub async fn current_user_id(&self) -> Result<String> {
        let uid = self.current_user_id.read().await.clone();
        if uid.is_empty() {
            return Err(FlareError::localized(ErrorCode::NotConnected, "未连接"));
        }
        Ok(uid)
    }

    pub async fn get_one(
        &self,
        source_id: &str,
        conversation_type: &ConversationType,
        save: bool,
    ) -> Result<Conversation> {
        let user_id = self.current_user_id().await?;
        let conversation_id = self.identity_service.resolve_conversation_id(
            &user_id,
            source_id,
            conversation_type,
        )?;
        let existing = self.store.get(&conversation_id).await?;
        let (conversation, needs_persist) = self.identity_service.merge_or_create(
            existing,
            conversation_id,
            source_id,
            conversation_type,
        );
        if save && needs_persist {
            self.store.save_batch(&[conversation.clone()]).await?;
        }
        Ok(conversation)
    }

    pub async fn get_group_by_user_ids(
        &self,
        user_ids: &[String],
        display_name: Option<&str>,
    ) -> Result<Conversation> {
        let current_user_id = self.current_user_id().await?;
        let mut members = user_ids
            .iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();
        members.push(current_user_id.clone());
        members.sort();
        members.dedup();
        if members.len() < 2 {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "群聊至少需要 2 个成员",
            ));
        }

        let group_key = format!("users:{}", members.join(","));
        let conversation_id = self.identity_service.resolve_conversation_id(
            "",
            &group_key,
            &ConversationType::Group,
        )?;
        let existing = self.store.get(&conversation_id).await?;
        let (mut conversation, _) = self.identity_service.merge_or_create(
            existing,
            conversation_id,
            &group_key,
            &ConversationType::Group,
        );
        if let Some(name) = display_name.map(str::trim).filter(|name| !name.is_empty()) {
            conversation.display_name = name.to_string();
        } else if conversation.display_name.trim().is_empty()
            || conversation.display_name == group_key
        {
            conversation.display_name = format!("群聊({})", members.join("、"));
        }
        conversation.members_count = members.len() as u32;
        let participants = members
            .iter()
            .map(|user_id| ConversationParticipant {
                user_id: user_id.clone(),
                roles: if user_id == &current_user_id {
                    vec!["owner".to_string()]
                } else {
                    vec!["member".to_string()]
                },
                ..Default::default()
            })
            .collect::<Vec<_>>();
        conversation.participant_version = participants.len() as u64;
        conversation.member_preview = participants.iter().take(10).cloned().collect();
        conversation.participants = participants;
        conversation
            .ext
            .insert("group_member_ids".to_string(), members.join(","));
        conversation
            .ext
            .insert("group_source".to_string(), "user_ids".to_string());
        self.store.save_batch(&[conversation.clone()]).await?;
        Ok(conversation)
    }

    pub async fn mark_read(&self, conversation_id: &str, read_seq: u64) -> Result<u32> {
        let Some(current) = self.store.get(conversation_id).await? else {
            return Ok(0);
        };
        let local_max_seq = self
            .store
            .get_local_max_seq(conversation_id)
            .await
            .unwrap_or(current.max_seq);
        let decision = self
            .read_service
            .plan_mark_read(&current, local_max_seq, read_seq);
        self.store
            .update_unread(
                conversation_id,
                decision.unread_count,
                decision.next_read_seq,
            )
            .await?;

        let current_user_id = self.current_user_id.read().await.clone();
        if decision.should_recompute_local_unread && !current_user_id.is_empty() {
            let _ = self
                .store
                .recompute_unread_for_user(conversation_id, &current_user_id)
                .await;
            if let Some(updated) = self.store.get(conversation_id).await? {
                return Ok(updated.unread_count);
            }
        }
        Ok(decision.unread_count)
    }

    pub async fn mark_all_read(&self) -> Result<()> {
        let list = self.store.list().await?;
        for conversation in list {
            let _ = self
                .mark_read(conversation.conversation_id(), conversation.max_seq())
                .await;
        }
        Ok(())
    }

    pub async fn delete(&self, conversation_id: &str) -> Result<()> {
        self.store.delete(conversation_id).await
    }

    pub async fn set_pinned(&self, conversation_id: &str, pinned: bool) -> Result<()> {
        self.store.set_pinned(conversation_id, pinned).await
    }

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
        if self.store.get(conversation_id).await?.is_some() {
            return Ok(());
        }
        let summary = self.identity_service.build_local_conversation(
            conversation_id,
            display_name,
            conversation_type,
            business_type,
            channel_id,
        );
        self.store.save_batch(&[summary]).await
    }
}
