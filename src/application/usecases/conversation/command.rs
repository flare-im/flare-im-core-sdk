use std::sync::Arc;

use crate::application::adapters::SyncProtocolAdapter;
use crate::application::sync_task::ConversationUserSettingsPatch;
use crate::application::{ConversationLocalLifecycle, LocalConversationVisibility};
use crate::domain::{
    ConversationIdentityService, ConversationReadService, ConversationStore, SyncCursorStore,
};
use crate::kernel::{CurrentUserIdStore, SessionSyncRunner};
use crate::model::conversation::ConversationType;
use crate::model::{
    Conversation, ConversationParticipant, mark_settings_dirty, user_settings_version,
};
use crate::shared::error::{ErrorCode, FlareError, Result};

pub struct ConversationCommandUseCase {
    store: Arc<dyn ConversationStore>,
    cursors: Option<Arc<dyn SyncCursorStore>>,
    current_user_id: CurrentUserIdStore,
    identity_service: ConversationIdentityService,
    read_service: ConversationReadService,
    settings_sync: Option<Arc<SyncProtocolAdapter>>,
    read_ack_sync: Option<Arc<dyn SessionSyncRunner>>,
}

impl ConversationCommandUseCase {
    pub fn new(
        store: Arc<dyn ConversationStore>,
        current_user_id: CurrentUserIdStore,
        settings_sync: Option<Arc<SyncProtocolAdapter>>,
        cursors: Option<Arc<dyn SyncCursorStore>>,
    ) -> Self {
        Self {
            store,
            cursors,
            current_user_id,
            identity_service: ConversationIdentityService,
            read_service: ConversationReadService,
            read_ack_sync: settings_sync
                .as_ref()
                .map(|sync| Arc::clone(sync) as Arc<dyn SessionSyncRunner>),
            settings_sync,
        }
    }

    async fn after_user_settings_write(
        &self,
        conversation_id: &str,
        patch: ConversationUserSettingsPatch,
    ) -> Result<()> {
        let Some(mut conversation) = self.store.get(conversation_id).await? else {
            return Ok(());
        };
        mark_settings_dirty(&mut conversation);
        self.store.save_batch(&[conversation.clone()]).await?;
        if let Some(sync) = &self.settings_sync {
            let base = user_settings_version(&conversation);
            sync.push_conversation_user_settings(conversation_id, base, patch)
                .await?;
        }
        Ok(())
    }

    pub async fn current_user_id(&self) -> Result<String> {
        let uid = self.current_user_id.read().await.trim().to_string();
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
            &user_id,
            source_id,
            conversation_type,
        );
        if save && needs_persist {
            self.store
                .save_batch(std::slice::from_ref(&conversation))
                .await?;
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
            &current_user_id,
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
        if read_seq == 0 {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "read_seq must be greater than 0",
            ));
        }
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

        if decision.next_read_seq > 0
            && let Some(sync) = &self.read_ack_sync
        {
            sync.send_read_ack(conversation_id, decision.next_read_seq)
                .await?;
        }

        self.store
            .update_unread(
                conversation_id,
                decision.unread_count,
                decision.next_read_seq,
            )
            .await?;

        let current_user_id = self.current_user_id.read().await.trim().to_string();
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

    pub async fn delete(&self, conversation_id: &str) -> Result<()> {
        self.current_user_id().await?;
        self.store.delete(conversation_id).await
    }

    pub async fn set_pinned(&self, conversation_id: &str, pinned: bool) -> Result<()> {
        self.current_user_id().await?;
        self.store.set_pinned(conversation_id, pinned).await?;
        self.after_user_settings_write(
            conversation_id,
            ConversationUserSettingsPatch {
                is_pinned: Some(pinned),
                ..Default::default()
            },
        )
        .await
    }

    pub async fn set_muted(&self, conversation_id: &str, muted: bool) -> Result<()> {
        self.current_user_id().await?;
        self.store.set_muted(conversation_id, muted).await?;
        self.after_user_settings_write(
            conversation_id,
            ConversationUserSettingsPatch {
                is_muted: Some(muted),
                ..Default::default()
            },
        )
        .await
    }

    pub async fn set_archived(&self, conversation_id: &str, archived: bool) -> Result<()> {
        self.current_user_id().await?;
        self.store.set_archived(conversation_id, archived).await?;
        self.after_user_settings_write(
            conversation_id,
            ConversationUserSettingsPatch {
                is_archived: Some(archived),
                ..Default::default()
            },
        )
        .await
    }

    pub async fn mark_unread(&self, conversation_id: &str) -> Result<u32> {
        self.current_user_id().await?;
        self.store.mark_unread(conversation_id).await
    }

    pub async fn update_draft(&self, conversation_id: &str, draft: Option<&str>) -> Result<()> {
        self.current_user_id().await?;
        self.store.update_draft(conversation_id, draft).await?;
        self.after_user_settings_write(
            conversation_id,
            ConversationUserSettingsPatch {
                draft: Some(draft.unwrap_or("").to_string()),
                ..Default::default()
            },
        )
        .await
    }

    /// 清空本地聊天记录（保留会话；已清空 seq 及更早消息不再同步落库）。
    pub async fn clear_local_chat_history(&self, conversation_id: &str) -> Result<()> {
        let user_id = self.current_user_id().await?;
        let cleared = ConversationLocalLifecycle::clear_history_boundary_with_ports(
            self.store.as_ref(),
            self.cursors.as_deref(),
            &user_id,
            conversation_id,
            LocalConversationVisibility::Keep,
        )
        .await?;
        if cleared.is_none() {
            return Err(FlareError::general_error("conversation not found"));
        }
        Ok(())
    }

    pub async fn ensure_local_conversation(
        &self,
        conversation_id: &str,
        display_name: Option<&str>,
        conversation_type: ConversationType,
        business_type: &str,
        channel_id: String,
    ) -> Result<()> {
        self.current_user_id().await?;
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

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;

    use tokio::sync::RwLock;

    use super::*;
    use crate::domain::{ConversationReader, ConversationWriter};
    use crate::infrastructure::persistence::memory_im::MemoryConversationStore;
    use crate::model::Conversation;

    struct FakeSessionSyncRunner {
        fail_read_ack: bool,
    }

    impl SessionSyncRunner for FakeSessionSyncRunner {
        fn request_message_sync(
            &self,
            _conversation_id: &str,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn request_message_sync_from_seq(
            &self,
            _conversation_id: &str,
            _last_seq: u64,
            _limit: i32,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn send_read_ack(
            &self,
            _conversation_id: &str,
            _read_seq: u64,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            Box::pin(async move {
                if self.fail_read_ack {
                    Err(FlareError::localized(
                        ErrorCode::NetworkConnectionLost,
                        "read ack failed",
                    ))
                } else {
                    Ok(())
                }
            })
        }

        fn request_participants_sync(
            &self,
            _conversation_id: &str,
            _limit: i32,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<ConversationParticipant>>> + Send + '_>>
        {
            Box::pin(async { Ok(Vec::new()) })
        }
    }

    fn command_use_case(
        store: Arc<MemoryConversationStore>,
        read_ack_sync: Arc<dyn SessionSyncRunner>,
    ) -> ConversationCommandUseCase {
        ConversationCommandUseCase {
            store,
            cursors: None,
            current_user_id: Arc::new(RwLock::new("hugo".to_string())),
            identity_service: ConversationIdentityService,
            read_service: ConversationReadService,
            settings_sync: None,
            read_ack_sync: Some(read_ack_sync),
        }
    }

    async fn save_unread_conversation(store: &MemoryConversationStore) {
        let conversation = Conversation {
            conversation_id: "conv-read".to_string(),
            max_seq: 8,
            last_read_seq: 3,
            unread_count: 5,
            ..Default::default()
        };
        store.save_batch(&[conversation]).await.unwrap();
    }

    #[tokio::test]
    async fn mark_read_does_not_clear_local_unread_when_read_ack_fails() {
        let store = Arc::new(MemoryConversationStore::new());
        save_unread_conversation(&store).await;
        let use_case = command_use_case(
            Arc::clone(&store),
            Arc::new(FakeSessionSyncRunner {
                fail_read_ack: true,
            }),
        );

        let err = use_case.mark_read("conv-read", 8).await.unwrap_err();

        assert_eq!(err.code(), Some(ErrorCode::NetworkConnectionLost));
        let stored = store.get("conv-read").await.unwrap().unwrap();
        assert_eq!(stored.unread_count, 5);
        assert_eq!(stored.last_read_seq, 3);
    }

    #[tokio::test]
    async fn mark_read_clears_local_unread_after_read_ack_succeeds() {
        let store = Arc::new(MemoryConversationStore::new());
        save_unread_conversation(&store).await;
        let use_case = command_use_case(
            Arc::clone(&store),
            Arc::new(FakeSessionSyncRunner {
                fail_read_ack: false,
            }),
        );

        let unread = use_case.mark_read("conv-read", 8).await.unwrap();

        assert_eq!(unread, 0);
        let stored = store.get("conv-read").await.unwrap().unwrap();
        assert_eq!(stored.unread_count, 0);
        assert_eq!(stored.last_read_seq, 8);
    }
}
