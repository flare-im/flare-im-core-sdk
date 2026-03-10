use std::time::Duration;

use tracing::warn;

use crate::error::Result;
use crate::model::SyncRequest;
use crate::protocol::PacketSender;
use crate::store::StoreProvider;

const SYNC_TIMEOUT: Duration = Duration::from_secs(30);
const BATCH_SIZE: i32 = 200;

/// 内置消息同步逻辑
pub struct MessageSync;

impl MessageSync {
    /// 同步所有会话的消息
    pub async fn sync_all(sender: &PacketSender, stores: &StoreProvider) -> Result<()> {
        let conversations = stores.conversations.list().await?;

        for conv in &conversations {
            let cid = &conv.conversation_id;
            let cursor_key = format!("conv:{cid}");
            let last_seq = stores.cursors.get(&cursor_key).await?
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);

            if conv.max_seq <= last_seq {
                continue;
            }

            match Self::pull_messages(sender, stores, cid, last_seq).await {
                Ok(new_seq) => {
                    if new_seq > last_seq {
                        stores.cursors.save(&cursor_key, &new_seq.to_string()).await?;
                    }
                }
                Err(e) => {
                    warn!(conversation_id = cid, error = %e, "message sync failed, skipping");
                }
            }
        }

        Ok(())
    }

    /// 增量同步单个会话
    pub async fn sync_one(
        sender: &PacketSender,
        stores: &StoreProvider,
        conversation_id: &str,
    ) -> Result<()> {
        let cursor_key = format!("conv:{conversation_id}");
        let last_seq = stores.cursors.get(&cursor_key).await?
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        let new_seq = Self::pull_messages(sender, stores, conversation_id, last_seq).await?;
        if new_seq > last_seq {
            stores.cursors.save(&cursor_key, &new_seq.to_string()).await?;
        }
        Ok(())
    }

    async fn pull_messages(
        sender: &PacketSender,
        stores: &StoreProvider,
        conversation_id: &str,
        from_seq: u64,
    ) -> Result<u64> {
        let mut current = from_seq;
        loop {
            let resp = sender.sync_messages(
                SyncRequest {
                    conversation_id: conversation_id.to_string(),
                    last_seq: current,
                    limit: BATCH_SIZE,
                    ..Default::default()
                },
                SYNC_TIMEOUT,
            ).await?;

            let Some(ref env) = resp.envelope else { break };

            let mut messages = Vec::new();
            for ev in &env.events {
                if let Some(flare_proto::common::event::Payload::Message(msg)) = &ev.payload {
                    messages.push(msg.clone());
                }
            }
            if !messages.is_empty() {
                stores.messages.save_batch(&messages).await?;
            }
            if env.max_seq > current {
                current = env.max_seq;
            }
            if !env.has_more {
                break;
            }
        }
        Ok(current)
    }
}
