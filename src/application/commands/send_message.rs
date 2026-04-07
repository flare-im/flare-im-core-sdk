use std::sync::Arc;
use std::time::Duration;

use crate::error::Result;
use crate::middleware::MiddlewareChain;
use crate::model::message::{IMMessage, SendAck};
use crate::protocol::PacketSender;
use crate::reliable_queue::ReliableSendQueue;
use crate::domain::MessageStore;

const TIMEOUT_SECS: u64 = 15;

pub struct SendMessageCommand {
    pub message: IMMessage,
}

impl SendMessageCommand {
    pub fn new(message: IMMessage) -> Self {
        Self { message }
    }

    /// 直接发送并落库（不走可靠队列，仅当未配置 PendingSend Reader/Writer 时使用）
    pub async fn execute(
        &self,
        sender: &Arc<PacketSender>,
        store: &dyn MessageStore,
        _chain: &MiddlewareChain,
    ) -> Result<SendAck> {
        let mut msg = self.message.clone();
        msg.materialize_content_bytes_from_elem();
        let proto = msg.to_proto();
        sender
            .send_message(&proto, Duration::from_secs(TIMEOUT_SECS))
            .await?;
        store.save_batch(&[msg]).await?;
        Ok(SendAck {
            client_msg_id: self.message.client_msg_id.clone(),
            server_msg_id: self.message.server_id.clone(),
            seq: self.message.seq,
            success: true,
            ..Default::default()
        })
    }

    /// 经可靠队列入队（SendAck 由调用方通过 EventBus 等待）
    pub async fn execute_via_queue(&self, queue: &ReliableSendQueue) -> Result<()> {
        let mut msg = self.message.clone();
        msg.materialize_content_bytes_from_elem();
        queue.enqueue(msg).await
    }
}
