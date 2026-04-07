mod decoding;
mod event_applier;
mod models;

use crate::application::conversation_projection_applier::ConversationProjectionApplier;
use crate::application::event_deduper::EventDeduper;
use crate::application::incoming_message_converger::IncomingMessageConverger;
use crate::application::message_deduper::MessageDeduper;
use decoding::{decode_single_conversation_items, patches_to_summaries};
use event_applier::SyncEventApplier;
use models::{AppliedConversationIncremental, AppliedSingleConversationPage, ReplayMode};

use crate::domain::SyncCursorVo;
use crate::error::Result;
use crate::event::{ConversationEvent, EventBus, MessageEvent, SdkEvent};
use crate::store::StoreProvider;
use flare_proto::common::{ConversationsIncrementalSyncRes, SingleConversationSyncRes};

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
        _conversation_id: &str,
        user_id: &str,
        known_seq: u64,
        response: &SingleConversationSyncRes,
    ) -> Result<AppliedSingleConversationPage> {
        let decoded = decode_single_conversation_items(response, known_seq);

        self.event_applier
            .apply_events(user_id, &decoded.events, ReplayMode::SingleConversation)
            .await;

        let mut deduped_messages = Vec::with_capacity(decoded.messages.len());
        for message in decoded.messages {
            if self.message_deduper.record_if_new(&message).await {
                deduped_messages.push(message);
            }
        }
        let fresh_messages = self
            .incoming_message_converger
            .converge_messages(user_id, deduped_messages)
            .await?;

        if !fresh_messages.is_empty() {
            self.stores.messages.save_batch(&fresh_messages).await?;
            self.conversation_projection_applier
                .apply_messages(&fresh_messages, user_id)
                .await?;
            for message in &fresh_messages {
                self.bus.publish(SdkEvent::Message(MessageEvent::Received {
                    message: message.clone(),
                }));
            }
        }

        Ok(AppliedSingleConversationPage {
            has_decoded_items: decoded.has_decoded_items,
            max_seq: response.max_seq,
            has_more: response.has_more,
            next_cursor: response.next_cursor.clone(),
        })
    }

    pub async fn apply_critical_events(
        &self,
        user_id: &str,
        events: &[flare_proto::common::Event],
    ) {
        self.event_applier
            .apply_events(user_id, events, ReplayMode::CriticalEvents)
            .await;
    }

    pub async fn apply_conversation_incremental(
        &self,
        user_id: &str,
        response: &ConversationsIncrementalSyncRes,
    ) -> Result<AppliedConversationIncremental> {
        let conversation_ids: Vec<String> = response
            .patches
            .iter()
            .map(|patch| patch.conversation_id.clone())
            .collect();
        let summaries = patches_to_summaries(&response.patches);

        if !summaries.is_empty() {
            let conversations: Vec<crate::model::Conversation> =
                summaries.into_iter().map(crate::model::Conversation::from).collect();
            if let Err(error) = self.stores.conversations.save_batch(&conversations).await {
                tracing::error!(%error, count = conversations.len(), "保存会话失败");
            } else {
                for conversation in &conversations {
                    let _ = self
                        .stores
                        .conversations
                        .recompute_unread_for_user(&conversation.conversation_id, user_id)
                        .await;
                }
            }
        } else {
            tracing::warn!("响应中没有会话数据");
        }

        self.bus.publish(SdkEvent::Conversation(ConversationEvent::Synced {
            conversation_ids,
        }));

        Ok(AppliedConversationIncremental {
            has_more: response.has_more,
            server_cursor_ms: crate::util::date::prost_timestamp_to_ms(
                response.server_conversation_cursor.as_ref(),
            ),
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
        if let Err(error) = update_remote(
            user_id.to_string(),
            conversation_id.to_string(),
            last_seq,
        )
        .await
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
