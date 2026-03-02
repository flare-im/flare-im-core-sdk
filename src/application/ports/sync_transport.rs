use async_trait::async_trait;
use std::time::Duration;

#[async_trait]
pub trait SyncTransport: Send + Sync {
    async fn sync_conversations_all(
        &self,
        req: flare_proto::common::ConversationSyncAllRequest,
        timeout: Duration,
    ) -> anyhow::Result<flare_proto::common::ConversationSyncAllResponse>;

    /// 按会话同步事件（长连接：SyncRequest → SyncResponse，消息从 envelope.events 提取）
    async fn sync_messages(
        &self,
        req: flare_proto::common::SyncRequest,
        timeout: Duration,
    ) -> anyhow::Result<flare_proto::common::SyncResponse>;
}
