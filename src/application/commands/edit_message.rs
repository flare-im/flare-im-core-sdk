use std::sync::Arc;
use std::time::Duration;

use crate::error::Result;
use crate::model::event::{Event, EventType};
use crate::protocol::PacketSender;
use flare_proto::common::MessageEditEvent;
use flare_proto::common::event::Payload as EventPayload;

const TIMEOUT_SECS: u64 = 15;

pub struct EditMessageCommand {
    pub conversation_id: String,
    pub server_msg_id: String,
    pub new_content: Vec<u8>,
}

impl EditMessageCommand {
    pub async fn execute(&self, sender: &Arc<PacketSender>) -> Result<()> {
        let event = Event {
            conversation_id: self.conversation_id.clone(),
            r#type: EventType::EventMessageEdit as i32,
            payload: Some(EventPayload::Edit(MessageEditEvent {
                server_msg_id: self.server_msg_id.clone(),
                new_content: self.new_content.clone(),
                edit_version: 1,
                reason: String::new(),
                show_edited_mark: true,
            })),
            ..Default::default()
        };
        sender
            .send_event(&event, Duration::from_secs(TIMEOUT_SECS))
            .await
    }
}
