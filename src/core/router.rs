use std::sync::Arc;

use tracing::warn;

use crate::event::{EventBus, SdkEvent, MessageEvent, ConversationEvent};
use crate::store::{MessageStore, ConversationStore};

/// Router — 监听 EventBus 并将事件路由到本地 Store
///
/// 覆盖所有需要本地副作用的事件：
/// - MessageReceived → save to MessageStore
/// - MessageRecalled → update status to Recalled
/// - MessageEdited → update content
/// - MessageDeleted → delete from store
/// - ConversationSynced → save to ConversationStore
/// - ConversationUpdated → update conversation (unread)
/// - ConversationDeleted → delete from store
pub struct Router {
    messages: Arc<dyn MessageStore>,
    conversations: Arc<dyn ConversationStore>,
}

impl Router {
    pub fn new(
        messages: Arc<dyn MessageStore>,
        conversations: Arc<dyn ConversationStore>,
    ) -> Self {
        Self { messages, conversations }
    }

    pub fn start(&self, bus: &EventBus) -> tokio::task::JoinHandle<()> {
        let messages = self.messages.clone();
        let conversations = self.conversations.clone();
        let mut rx = bus.subscribe();

        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                match &*event {
                    // ── 消息 → Store ─────────────────────
                    SdkEvent::Message(MessageEvent::Received { message }) => {
                        if let Err(e) = messages.save_batch(&[message.clone()]).await {
                            warn!(error = %e, "failed to save received message");
                        }
                    }
                    SdkEvent::Message(MessageEvent::Recalled { event, .. }) => {
                        if let Err(e) = messages.update_status(
                            &event.server_msg_id,
                            flare_proto::common::MessageStatus::Recalled as i32,
                        ).await {
                            warn!(error = %e, msg_id = %event.server_msg_id, "failed to update recalled status");
                        }
                    }
                    SdkEvent::Message(MessageEvent::Edited { event, .. }) => {
                        if let Err(e) = messages.update_content(
                            &event.server_msg_id,
                            event.new_content.clone(),
                        ).await {
                            warn!(error = %e, msg_id = %event.server_msg_id, "failed to update edited content");
                        }
                    }
                    SdkEvent::Message(MessageEvent::Deleted { event, .. }) => {
                        if let Err(e) = messages.delete(&event.server_msg_id).await {
                            warn!(error = %e, msg_id = %event.server_msg_id, "failed to delete message");
                        }
                    }

                    // ── 会话 → Store ─────────────────────
                    SdkEvent::Conversation(ConversationEvent::Synced { conversations: convs }) => {
                        if let Err(e) = conversations.save_batch(convs).await {
                            warn!(error = %e, "failed to save synced conversations");
                        }
                    }
                    SdkEvent::Conversation(ConversationEvent::Updated { conversation_id, event }) => {
                        if event.unread_count > 0 {
                            if let Err(e) = conversations.update_unread(
                                conversation_id,
                                event.unread_count as u32,
                                0,
                            ).await {
                                warn!(error = %e, conv_id = %conversation_id, "failed to update conversation unread");
                            }
                        }
                    }
                    SdkEvent::Conversation(ConversationEvent::Deleted { conversation_id }) => {
                        if let Err(e) = conversations.delete(conversation_id).await {
                            warn!(error = %e, conv_id = %conversation_id, "failed to delete conversation");
                        }
                    }

                    _ => {}
                }
            }
        })
    }
}
