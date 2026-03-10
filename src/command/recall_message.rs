use std::time::Duration;

use crate::error::Result;
use crate::model::message::{MessageRecallEvent, MessageStatus};
use crate::model::event::{Event, EventType};
use crate::protocol::PacketSender;
use crate::store::MessageStore;
use flare_proto::common::event::Payload as EventPayload;

const TIMEOUT: Duration = Duration::from_secs(15);

/// 撤回消息命令
pub struct RecallMessageCommand {
    pub conversation_id: String,
    pub server_msg_id: String,
}

impl RecallMessageCommand {
    pub async fn execute(self, sender: &PacketSender, store: &dyn MessageStore) -> Result<()> {
        let event = Event {
            conversation_id: self.conversation_id,
            r#type: EventType::EventMessageRecall as i32,
            payload: Some(EventPayload::Recall(MessageRecallEvent {
                server_msg_id: self.server_msg_id.clone(),
                ..Default::default()
            })),
            ..Default::default()
        };
        sender.send_event(event, TIMEOUT).await?;
        store.update_status(&self.server_msg_id, MessageStatus::Recalled as i32).await?;
        Ok(())
    }
}
