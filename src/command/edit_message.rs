use std::time::Duration;

use crate::error::Result;
use crate::model::message::MessageEditEvent;
use crate::model::event::{Event, EventType};
use crate::protocol::PacketSender;
use flare_proto::common::event::Payload as EventPayload;

const TIMEOUT: Duration = Duration::from_secs(15);

/// 编辑消息命令
pub struct EditMessageCommand {
    pub conversation_id: String,
    pub server_msg_id: String,
    pub new_content: Vec<u8>,
}

impl EditMessageCommand {
    pub async fn execute(self, sender: &PacketSender) -> Result<()> {
        let event = Event {
            conversation_id: self.conversation_id,
            r#type: EventType::EventMessageEdit as i32,
            payload: Some(EventPayload::Edit(MessageEditEvent {
                server_msg_id: self.server_msg_id,
                new_content: self.new_content,
                ..Default::default()
            })),
            ..Default::default()
        };
        sender.send_event(event, TIMEOUT).await?;
        Ok(())
    }
}
