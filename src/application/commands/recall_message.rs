use std::sync::Arc;
use std::time::Duration;

use crate::error::Result;
use crate::model::event::{Event, EventType};
use crate::protocol::PacketSender;
use crate::domain::MessageStore;
use flare_proto::common::MessageRecallEvent;
use flare_proto::common::event::Payload as EventPayload;

const TIMEOUT_SECS: u64 = 15;

pub struct RecallMessageCommand {
    pub conversation_id: String,
    pub server_msg_id: String,
}

impl RecallMessageCommand {
    pub async fn execute(
        &self,
        sender: &Arc<PacketSender>,
        store: &dyn MessageStore,
    ) -> Result<()> {
        let event = Event {
            conversation_id: self.conversation_id.clone(),
            r#type: EventType::EventMessageRecall as i32,
            payload: Some(EventPayload::Recall(MessageRecallEvent {
                server_msg_id: self.server_msg_id.clone(),
                reason: String::new(),
                time_limit_seconds: None,
                allow_admin_recall: None,
            })),
            ..Default::default()
        };
        sender
            .send_event(&event, Duration::from_secs(TIMEOUT_SECS))
            .await?;
        store
            .update_status(
                &self.server_msg_id,
                flare_proto::common::MessageStatus::Recalled as i32,
            )
            .await?;
        Ok(())
    }
}
