use std::time::Duration;

use crate::error::Result;
use crate::middleware::MiddlewareChain;
use crate::model::message::{Message, MessageStatus, SendAck};
use crate::protocol::PacketSender;
use crate::store::MessageStore;

const TIMEOUT: Duration = Duration::from_secs(15);

/// 发送消息命令
pub struct SendMessageCommand {
    pub message: Message,
}

impl SendMessageCommand {
    pub fn new(message: Message) -> Self {
        Self { message }
    }

    pub async fn execute(
        self,
        sender: &PacketSender,
        store: &dyn MessageStore,
        chain: &MiddlewareChain,
    ) -> Result<SendAck> {
        let mut msg = self.message;

        if chain.has_message_interceptors() {
            msg = chain.intercept_outgoing(msg).await?;
        }

        let ack = sender.send_message(msg.clone(), TIMEOUT).await?;

        if ack.success {
            let mut persisted = msg;
            persisted.server_id = ack.server_msg_id.clone();
            persisted.seq = ack.seq;
            persisted.status = MessageStatus::Sent as i32;
            store.save_batch(&[persisted]).await?;
        }

        Ok(ack)
    }
}
