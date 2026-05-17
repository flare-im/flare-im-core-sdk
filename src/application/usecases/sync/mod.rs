mod decoding;
mod event_applier;
mod models;

use crate::application::conversation_projection_applier::ConversationProjectionApplier;
use crate::application::event_deduper::EventDeduper;
use crate::application::incoming_message_converger::IncomingMessageConverger;
use crate::application::message_deduper::MessageDeduper;
use decoding::decode_single_conversation_items;
use event_applier::SyncEventApplier;
use models::{AppliedConversationIncremental, AppliedSingleConversationPage, ReplayMode};

use crate::domain::SyncCursorVo;
use crate::error::Result;
use crate::event::{ConversationEvent, EventBus, MessageEvent, SdkEvent};
use crate::store::StoreProvider;
use flare_proto::common::{ConversationsSyncRes, SingleConversationSyncRes};

pub struct SyncApplyUseCase {
    stores: StoreProvider,
    bus: EventBus,
    event_applier: SyncEventApplier,
    message_deduper: MessageDeduper,
    incoming_message_converger: IncomingMessageConverger,
    conversation_projection_applier: ConversationProjectionApplier,
}

impl SyncApplyUseCase {
    pub fn new(
        stores: StoreProvider,
        bus: EventBus,
        event_deduper: EventDeduper,
        message_deduper: MessageDeduper,
    ) -> Self {
        let event_applier = SyncEventApplier::new(stores.clone(), bus.clone(), event_deduper);
        let incoming_message_converger =
            IncomingMessageConverger::new(stores.messages.clone(), bus.clone(), None);
        let conversation_projection_applier =
            ConversationProjectionApplier::new(stores.clone(), bus.clone());
        Self {
            stores,
            bus,
            event_applier,
            message_deduper,
            incoming_message_converger,
            conversation_projection_applier,
        }
    }

    pub fn set_reliable_queue(
        &self,
        reliable_queue: Option<std::sync::Arc<crate::reliable_queue::ReliableSendQueue>>,
    ) {
        self.incoming_message_converger
            .set_reliable_queue(reliable_queue);
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

        let fresh_messages = self
            .incoming_message_converger
            .converge_messages(user_id, decoded.messages)
            .await?;

        if !fresh_messages.is_empty() {
            self.stores.messages.save_batch(&fresh_messages).await?;
            self.conversation_projection_applier
                .apply_messages(&fresh_messages, user_id)
                .await?;
            for message in &fresh_messages {
                if self.message_deduper.record_if_new(message).await {
                    self.bus.publish(SdkEvent::Message(MessageEvent::Received {
                        message: message.clone(),
                    }));
                }
            }
        }

        let safe_max_seq = max_contiguous_seq(known_seq, &applied_item_seqs);
        let has_seq_gap = response.max_seq > safe_max_seq;
        if has_seq_gap {
            tracing::warn!(
                conversation_id = %conversation_id,
                known_seq,
                safe_max_seq,
                remote_max_seq = response.max_seq,
                item_count = response.items.len(),
                "消息同步响应存在非连续 seq，游标只推进到已落库连续位点"
            );
        }

        Ok(AppliedSingleConversationPage {
            has_decoded_items: decoded.has_decoded_items,
            max_seq: safe_max_seq,
            remote_max_seq: response.max_seq,
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
        let conversation_ids: Vec<String> = response
            .conversations
            .iter()
            .map(|summary| summary.conversation_id.clone())
            .collect();
        let summaries = response.conversations.clone();
        let mut message_sync_conversation_ids = Vec::new();

        if !summaries.is_empty() {
            let conversations: Vec<crate::model::Conversation> = summaries
                .into_iter()
                .map(crate::model::Conversation::from)
                .collect();
            for conversation in &conversations {
                if conversation.conversation_id.trim().is_empty() || conversation.max_seq == 0 {
                    continue;
                }
                let local_message_seq = self
                    .stores
                    .cursors
                    .get_conversation_cursor(user_id, &conversation.conversation_id)
                    .await?
                    .map(|cursor| cursor.last_seq)
                    .unwrap_or_default();
                if conversation.max_seq > local_message_seq {
                    message_sync_conversation_ids.push(conversation.conversation_id.clone());
                }
            }
            if let Err(error) = self.stores.conversations.save_batch(&conversations).await {
                tracing::error!(%error, count = conversations.len(), "保存会话失败");
            } else {
                for conversation in &conversations {
                    let _ = self
                        .stores
                        .conversations
                        .recompute_unread_for_user(&conversation.conversation_id, user_id)
                        .await;
                    if conversation.peer_read_seq > 0 {
                        if let Err(error) = self
                            .stores
                            .messages
                            .mark_outgoing_read_upto_seq(
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
                                "同步会话时回填消息已读失败"
                            );
                        }
                    }
                }
            }
        } else {
            tracing::debug!(
                has_more = response.has_more,
                server_cursor_ms = crate::util::date::prost_timestamp_to_ms(
                    response.server_conversation_cursor.as_ref()
                ),
                "会话增量同步无变更"
            );
        }

        self.bus
            .publish(SdkEvent::Conversation(ConversationEvent::Synced {
                conversation_ids,
            }));

        Ok(AppliedConversationIncremental {
            has_more: response.has_more,
            server_cursor_ms: crate::util::date::prost_timestamp_to_ms(
                response.server_conversation_cursor.as_ref(),
            ),
            message_sync_conversation_ids,
        })
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
