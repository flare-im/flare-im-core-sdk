use std::time::Duration;

use tracing::info;

use crate::error::Result;
use crate::event::{EventBus, SdkEvent, ConversationEvent};
use crate::model::ConversationSyncAllRequest;
use crate::protocol::PacketSender;
use crate::store::StoreProvider;

const SYNC_TIMEOUT: Duration = Duration::from_secs(30);

/// 内置会话同步逻辑
pub struct ConversationSync;

impl ConversationSync {
    /// 全量拉取会话列表 → 存储 + 发布事件
    pub async fn sync_all(
        sender: &PacketSender,
        stores: &StoreProvider,
        bus: &EventBus,
    ) -> Result<()> {
        let resp = sender
            .sync_conversations_all(ConversationSyncAllRequest::default(), SYNC_TIMEOUT)
            .await?;

        let conversations = resp.conversations;
        info!(count = conversations.len(), "synced conversations");

        stores.conversations.save_batch(&conversations).await?;
        bus.publish(SdkEvent::Conversation(ConversationEvent::Synced {
            conversations,
        }));

        Ok(())
    }
}
