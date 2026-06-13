use crate::domain::{MessageActor, MessageContentPolicy};
use crate::model::message::IMMessage;
use crate::shared::error::{ErrorCode, FlareError, Result};
use crate::shared::util::id;

pub struct MessageDraftService {
    content_policy: MessageContentPolicy,
}

impl Default for MessageDraftService {
    fn default() -> Self {
        Self {
            content_policy: MessageContentPolicy,
        }
    }
}

impl MessageDraftService {
    pub fn prepare_outbound_message(
        &self,
        actor: &MessageActor,
        mut message: IMMessage,
    ) -> Result<IMMessage> {
        if message.conversation_id.trim().is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "conversation_id is empty",
            ));
        }
        if message.client_msg_id.trim().is_empty() {
            message.client_msg_id = id::generate_client_msg_id();
        }
        if message.sender_id.trim().is_empty() || message.sender_id != actor.user_id {
            message.sender_id = actor.user_id.clone();
        }
        if message.created_at == 0 {
            message.created_at = id::now_millis();
        }
        if message.client_created_at == 0 {
            message.client_created_at = message.created_at;
        }
        self.content_policy.validate_outbound_message(&message)?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::{MessageActor, MessageDraftService};
    use crate::model::IMMessage;
    use flare_proto::common::Message;

    #[test]
    fn draft_service_fills_required_identity_fields() {
        let service = MessageDraftService::default();
        let actor = MessageActor::require("user-1".to_string()).unwrap();
        let mut message = IMMessage::new(Message::default());
        message.conversation_id = "conv-1".to_string();

        let prepared = service.prepare_outbound_message(&actor, message).unwrap();

        assert_eq!(prepared.sender_id, "user-1");
        assert!(!prepared.client_msg_id.is_empty());
        assert!(prepared.created_at > 0);
        assert_eq!(prepared.client_created_at, prepared.created_at);
    }
}
