mod decoding;
mod event_applier;
mod models;

use crate::application::projections::ConversationProjectionApplier;
use crate::application::services::EventDeduper;
use crate::application::services::IncomingMessageConverger;
use decoding::decode_single_conversation_items;
use event_applier::SyncEventApplier;
use models::{AppliedConversationIncremental, AppliedSingleConversationPage, ReplayMode};

use crate::application::notification::{
    NotificationInboundPipeline, partition_notification_durability,
};
use crate::domain::{
    ConversationIdRewrite, ConversationIdentityService, SyncCursorVo, filter_messages_after_clear,
    local_cleared_through_seq,
};
use crate::infrastructure::persistence::StoreProvider;
use crate::kernel::ReliableSendQueuePort;
use crate::kernel::event::{ConversationEvent, EventBus, SdkEvent};
use crate::model::IMMessage;
use crate::shared::error::Result;
use flare_proto::common::{ConversationsSyncRes, SingleConversationSyncRes};

pub struct SyncApplyUseCase {
    stores: StoreProvider,
    bus: EventBus,
    event_applier: SyncEventApplier,
    notification_pipeline: NotificationInboundPipeline,
    incoming_message_converger: IncomingMessageConverger,
    conversation_projection_applier: ConversationProjectionApplier,
}

impl SyncApplyUseCase {
    pub fn new(
        stores: StoreProvider,
        bus: EventBus,
        event_deduper: EventDeduper,
        notification_pipeline: NotificationInboundPipeline,
    ) -> Self {
        let event_applier = SyncEventApplier::new(stores.clone(), bus.clone(), event_deduper);
        let incoming_message_converger = IncomingMessageConverger::new(
            stores.messages.clone(),
            stores.conversations.clone(),
            bus.clone(),
            None,
        );
        let conversation_projection_applier =
            ConversationProjectionApplier::new(stores.clone(), bus.clone());
        Self {
            stores,
            bus,
            event_applier,
            notification_pipeline,
            incoming_message_converger,
            conversation_projection_applier,
        }
    }

    pub fn set_reliable_queue(
        &self,
        reliable_queue: Option<std::sync::Arc<dyn ReliableSendQueuePort>>,
    ) {
        self.incoming_message_converger
            .set_reliable_queue(reliable_queue);
    }

    async fn local_message_sync_start_seq(
        &self,
        user_id: &str,
        conversation_id: &str,
    ) -> Result<u64> {
        let cursor_seq = self
            .stores
            .cursors
            .get_conversation_cursor(user_id, conversation_id)
            .await?
            .map(|cursor| cursor.last_seq)
            .unwrap_or_default();
        let local_max_seq = self
            .stores
            .conversations
            .get_local_max_seq(conversation_id)
            .await?;
        let cleared_floor = self
            .stores
            .conversations
            .get(conversation_id)
            .await?
            .map(|conversation| {
                local_cleared_through_seq(&conversation.ext).max(conversation.visible_after_seq)
            })
            .unwrap_or_default();
        Ok(local_message_sync_start_seq(
            cursor_seq,
            local_max_seq,
            cleared_floor,
        ))
    }

    pub async fn apply_single_conversation_page(
        &self,
        conversation_id: &str,
        user_id: &str,
        known_seq: u64,
        response: &SingleConversationSyncRes,
    ) -> Result<AppliedSingleConversationPage> {
        let decoded = decode_single_conversation_items(response, known_seq);
        let applied_item_seqs = decoded.applied_item_seqs.clone();

        let _ = self
            .event_applier
            .apply_events(user_id, &decoded.events, ReplayMode::SingleConversation)
            .await;

        let cleared_floor = self
            .stores
            .conversations
            .get(conversation_id)
            .await?
            .map(|c| local_cleared_through_seq(&c.ext).max(c.visible_after_seq))
            .unwrap_or(0);

        let fresh_messages = filter_messages_after_clear(
            self.incoming_message_converger
                .converge_messages(user_id, decoded.messages)
                .await?,
            cleared_floor,
        );

        if !fresh_messages.is_empty() {
            let (durable_messages, ephemeral_messages): (Vec<IMMessage>, Vec<IMMessage>) =
                partition_notification_durability(fresh_messages);
            if !durable_messages.is_empty() {
                self.stores.messages.save_batch(&durable_messages).await?;
                self.conversation_projection_applier
                    .apply_synced_messages(&durable_messages, user_id)
                    .await?;
            }
            let mut inbound = durable_messages;
            inbound.extend(ephemeral_messages);
            self.notification_pipeline.finish_batch(inbound).await;
        }

        let safe_max_seq = max_contiguous_seq(known_seq, &applied_item_seqs);
        let has_seq_gap = response.max_conversation_seq > safe_max_seq;
        if has_seq_gap {
            tracing::warn!(
                conversation_id = %conversation_id,
                known_seq,
                safe_max_seq,
                remote_max_seq = response.max_conversation_seq,
                item_count = response.items.len(),
                "消息同步响应存在非连续 seq，游标只推进到已落库连续位点"
            );
        }

        Ok(AppliedSingleConversationPage {
            has_decoded_items: decoded.has_decoded_items,
            max_seq: safe_max_seq,
            remote_max_seq: response.max_conversation_seq,
            has_more: response.has_more || has_seq_gap,
            next_cursor: response.next_cursor.clone(),
            has_seq_gap,
        })
    }

    pub async fn apply_critical_events(
        &self,
        user_id: &str,
        events: &[flare_proto::common::Event],
    ) -> Vec<u64> {
        self.event_applier
            .apply_events(user_id, events, ReplayMode::CriticalEvents)
            .await
    }

    pub async fn apply_conversations(
        &self,
        user_id: &str,
        response: &ConversationsSyncRes,
    ) -> Result<AppliedConversationIncremental> {
        let mut conversation_ids = Vec::new();
        let mut message_sync_conversation_ids = Vec::new();
        let mut conversations = Vec::new();

        for summary in response
            .conversations
            .iter()
            .filter(|summary| !is_hidden_internal_conversation(&summary.conversation_id))
            .cloned()
        {
            let mut conversation = crate::model::Conversation::from(summary);
            if let Some(rewrite) =
                ConversationIdentityService::canonicalize_single_chat_conversation(
                    &mut conversation,
                    user_id,
                )
            {
                self.apply_conversation_identity_rewrite(&rewrite).await?;
            }
            if is_hidden_internal_conversation(&conversation.conversation_id) {
                tracing::warn!(
                    conversation_id = %conversation.conversation_id,
                    "drop invalid or hidden conversation summary before local save"
                );
                continue;
            }
            conversation_ids.push(conversation.conversation_id.clone());
            conversations.push(conversation);
        }

        if !conversations.is_empty() {
            for conversation in &conversations {
                if conversation.conversation_id.trim().is_empty() {
                    continue;
                }
                let remote_target_seq = remote_conversation_target_seq(conversation);
                if remote_target_seq == 0 {
                    continue;
                }
                let local_message_seq = self
                    .local_message_sync_start_seq(user_id, &conversation.conversation_id)
                    .await?;
                if remote_target_seq > local_message_seq {
                    message_sync_conversation_ids.push(conversation.conversation_id.clone());
                }
            }
            let mut before_save = std::collections::HashMap::new();
            for conversation in &conversations {
                if let Ok(Some(prev)) = self
                    .stores
                    .conversations
                    .get(&conversation.conversation_id)
                    .await
                {
                    before_save.insert(conversation.conversation_id.clone(), prev);
                }
            }
            for conversation in &conversations {
                let previous_unread = before_save
                    .get(&conversation.conversation_id)
                    .map(|c| c.unread_count)
                    .unwrap_or_default();
                if should_sync_messages_for_unread_delta(previous_unread, conversation.unread_count)
                    && !message_sync_conversation_ids
                        .iter()
                        .any(|id| id == &conversation.conversation_id)
                {
                    message_sync_conversation_ids.push(conversation.conversation_id.clone());
                }
            }
            if let Err(error) = self.stores.conversations.save_batch(&conversations).await {
                tracing::error!(%error, count = conversations.len(), "保存会话失败");
            } else {
                for conversation in &conversations {
                    let previous_unread = before_save
                        .get(&conversation.conversation_id)
                        .map(|c| c.unread_count)
                        .unwrap_or_default();
                    if let Ok(Some(updated)) = self
                        .stores
                        .conversations
                        .get(&conversation.conversation_id)
                        .await
                        && updated.unread_count != previous_unread
                    {
                        self.bus.publish(SdkEvent::Conversation(
                            ConversationEvent::UnreadCountChanged {
                                conversation_id: conversation.conversation_id.clone(),
                                unread_count: updated.unread_count,
                            },
                        ));
                    }
                    if let Err(error) = self
                        .stores
                        .messages
                        .reconcile_outgoing_read_by_peer_seq(
                            &conversation.conversation_id,
                            user_id,
                            conversation.peer_read_seq,
                        )
                        .await
                    {
                        tracing::warn!(
                            conversation_id = %conversation.conversation_id,
                            peer_read_seq = conversation.peer_read_seq,
                            error = %error,
                            "同步会话时修正对端已读位点失败"
                        );
                    }
                }
            }
        } else {
            tracing::debug!(
                has_more = response.has_more,
                server_cursor = %response.next_cursor,
                "会话增量同步无变更"
            );
        }

        self.bus
            .publish(SdkEvent::Conversation(ConversationEvent::Synced {
                conversation_ids: conversation_ids.clone(),
            }));

        Ok(AppliedConversationIncremental {
            has_more: response.has_more,
            message_sync_conversation_ids,
            synced_conversation_ids: conversation_ids,
        })
    }

    async fn apply_conversation_identity_rewrite(
        &self,
        rewrite: &ConversationIdRewrite,
    ) -> Result<()> {
        if rewrite.from == rewrite.to {
            return Ok(());
        }
        let moved = self
            .stores
            .messages
            .rewrite_conversation_id(&rewrite.from, &rewrite.to)
            .await?;
        self.stores
            .conversations
            .merge_conversation_identity(&rewrite.from, &rewrite.to)
            .await?;
        tracing::info!(
            from = %rewrite.from,
            to = %rewrite.to,
            moved,
            "canonicalized conversation summary identity"
        );
        Ok(())
    }

    pub async fn save_cursor_with_remote<F, Fut>(
        &self,
        user_id: &str,
        conversation_id: &str,
        last_seq: u64,
        update_remote: F,
    ) -> Result<()>
    where
        F: FnOnce(String, String, u64) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        self.stores
            .cursors
            .save_conversation_cursor(&SyncCursorVo {
                user_id: user_id.to_string(),
                conversation_id: conversation_id.to_string(),
                last_seq,
                synced_at: now_ms(),
            })
            .await?;
        if let Err(error) =
            update_remote(user_id.to_string(), conversation_id.to_string(), last_seq).await
        {
            tracing::warn!(
                user_id = %user_id,
                conversation_id = %conversation_id,
                error = %error,
                "update remote cursor failed"
            );
        }
        Ok(())
    }
}

fn max_contiguous_seq(known_seq: u64, seqs: &[u64]) -> u64 {
    let mut sorted = seqs
        .iter()
        .copied()
        .filter(|seq| *seq > known_seq)
        .collect::<Vec<_>>();
    sorted.sort_unstable();
    sorted.dedup();

    let mut cursor = known_seq;
    for seq in sorted {
        if seq == cursor + 1 {
            cursor = seq;
        } else if seq > cursor + 1 {
            break;
        }
    }
    cursor
}

pub(crate) fn local_message_sync_start_seq(
    cursor_last_seq: u64,
    local_max_seq: u64,
    cleared_floor: u64,
) -> u64 {
    let materialized_seq = if cursor_last_seq == 0 || local_max_seq == 0 {
        0
    } else {
        cursor_last_seq.min(local_max_seq)
    };
    materialized_seq.max(cleared_floor)
}

fn remote_conversation_target_seq(conversation: &crate::model::Conversation) -> u64 {
    let unread_tail_seq = conversation
        .last_read_seq
        .saturating_add(conversation.unread_count as u64);
    conversation
        .max_seq
        .max(unread_tail_seq)
        .max(conversation.visible_after_seq)
}

fn should_sync_messages_for_unread_delta(previous_unread: u32, next_unread: u32) -> bool {
    next_unread > previous_unread
}

/// Social SyncSignal 内部路由会话；不得进入本地会话列表。
fn is_hidden_internal_conversation(conversation_id: &str) -> bool {
    let conversation_id = conversation_id.trim();
    conversation_id.is_empty() || conversation_id.starts_with("sync:")
}

fn now_ms() -> u64 {
    crate::shared::util::now_millis()
}

#[cfg(test)]
mod tests {
    use super::{
        is_hidden_internal_conversation, local_message_sync_start_seq,
        remote_conversation_target_seq, should_sync_messages_for_unread_delta,
    };
    use crate::model::Conversation;

    #[test]
    fn local_sync_start_ignores_polluted_cursor_ahead_of_local_messages() {
        assert_eq!(local_message_sync_start_seq(100, 0, 0), 0);
        assert_eq!(local_message_sync_start_seq(100, 12, 0), 12);
    }

    #[test]
    fn local_sync_start_uses_persisted_cursor_when_materialized() {
        assert_eq!(local_message_sync_start_seq(80, 100, 0), 80);
    }

    #[test]
    fn local_sync_start_respects_local_clear_floor() {
        assert_eq!(local_message_sync_start_seq(0, 0, 50), 50);
        assert_eq!(local_message_sync_start_seq(80, 100, 90), 90);
    }

    #[test]
    fn conversation_sync_target_infers_tail_from_unread_state() {
        let stale_summary = Conversation {
            conversation_id: "conv-1".to_string(),
            max_seq: 1,
            last_read_seq: 0,
            unread_count: 2,
            ..Default::default()
        };
        assert_eq!(remote_conversation_target_seq(&stale_summary), 2);

        let read_based_tail = Conversation {
            conversation_id: "conv-1".to_string(),
            max_seq: 1,
            last_read_seq: 10,
            unread_count: 2,
            ..Default::default()
        };
        assert_eq!(remote_conversation_target_seq(&read_based_tail), 12);

        let normal_summary = Conversation {
            conversation_id: "conv-1".to_string(),
            max_seq: 20,
            last_read_seq: 10,
            unread_count: 0,
            ..Default::default()
        };
        assert_eq!(remote_conversation_target_seq(&normal_summary), 20);
    }

    #[test]
    fn unread_increase_forces_message_sync() {
        assert!(should_sync_messages_for_unread_delta(0, 1));
        assert!(should_sync_messages_for_unread_delta(1, 2));
        assert!(!should_sync_messages_for_unread_delta(2, 2));
        assert!(!should_sync_messages_for_unread_delta(2, 1));
    }

    #[test]
    fn hidden_conversation_filter_rejects_blank_and_internal_ids() {
        assert!(is_hidden_internal_conversation(""));
        assert!(is_hidden_internal_conversation("   "));
        assert!(is_hidden_internal_conversation("sync:conversation"));
        assert!(!is_hidden_internal_conversation("conv-1"));
    }
}
