use std::sync::Arc;

use crate::event::ConversationEvent;
use crate::store::ConversationStore;

/// 会话推送处理器
pub struct ConversationHandler {
    store: Arc<dyn ConversationStore>,
}

impl ConversationHandler {
    pub fn new(store: Arc<dyn ConversationStore>) -> Self {
        Self { store }
    }

    pub async fn handle(&self, event: &ConversationEvent) {
        match event {
            ConversationEvent::Synced { conversations } => {
                let _ = self.store.save_batch(conversations).await;
            }
            ConversationEvent::Deleted { conversation_id } => {
                let _ = self.store.delete(conversation_id).await;
            }
            ConversationEvent::Patched { .. } => {
                // patch 处理需要根据 patch_type 做不同更新
            }
            ConversationEvent::Updated { conversation_id, event } => {
                if event.unread_count > 0 {
                    let _ = self.store.update_unread(conversation_id, event.unread_count as u32, 0).await;
                }
            }
        }
    }
}
