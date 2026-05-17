use crate::domain::{MessageActor, MessageTransportAction, ResolvedMessage};
use crate::error::{ErrorCode, FlareError, Result};
use crate::model::message::{MarkType, ReactionAction};

const DELETE_TYPE_SOFT: i32 = 1;
const DELETE_SCOPE_USER_PRIVATE: i32 = 1;
const DELETE_SCOPE_CONVERSATION_GLOBAL: i32 = 2;

#[derive(Debug, Clone)]
pub enum MessageLocalUpdate {
    None,
    UpdateContent {
        message_id: String,
        new_content: Vec<u8>,
    },
    Delete {
        message_id: String,
    },
}

#[derive(Debug, Clone)]
pub struct MessageMutationPlan {
    pub transport_action: MessageTransportAction,
    pub local_update: MessageLocalUpdate,
}

impl MessageMutationPlan {
    pub fn conversation_id(&self) -> &str {
        self.transport_action.conversation_id()
    }

    pub fn server_msg_id(&self) -> Option<&str> {
        self.transport_action.server_msg_id()
    }
}

pub struct MessageMutationService;

impl MessageMutationService {
    pub fn plan_recall(&self, target: &ResolvedMessage) -> MessageMutationPlan {
        MessageMutationPlan {
            transport_action: MessageTransportAction::Recall {
                conversation_id: target.conversation_id().to_string(),
                server_msg_id: target.server_id().to_string(),
            },
            local_update: MessageLocalUpdate::None,
        }
    }

    pub fn plan_edit(
        &self,
        conversation_id: &str,
        target: &ResolvedMessage,
        new_content: Vec<u8>,
    ) -> MessageMutationPlan {
        MessageMutationPlan {
            transport_action: MessageTransportAction::Edit {
                conversation_id: conversation_id.to_string(),
                server_msg_id: target.server_id().to_string(),
                new_content: new_content.clone(),
                edit_version: 0,
                reason: String::new(),
                show_edited_mark: true,
            },
            local_update: MessageLocalUpdate::UpdateContent {
                message_id: target.server_id().to_string(),
                new_content,
            },
        }
    }

    pub fn plan_delete_for_self(
        &self,
        actor: &MessageActor,
        target: &ResolvedMessage,
        reason: Option<String>,
    ) -> MessageMutationPlan {
        MessageMutationPlan {
            transport_action: MessageTransportAction::Delete {
                conversation_id: target.conversation_id().to_string(),
                server_msg_id: target.server_id().to_string(),
                delete_type: DELETE_TYPE_SOFT,
                scope: DELETE_SCOPE_USER_PRIVATE,
                reason,
                notify_others: false,
                target_user_id: Some(actor.user_id.clone()),
            },
            local_update: MessageLocalUpdate::Delete {
                message_id: target.server_id().to_string(),
            },
        }
    }

    pub fn plan_delete_for_everyone(
        &self,
        actor: &MessageActor,
        target: &ResolvedMessage,
        reason: Option<String>,
    ) -> Result<MessageMutationPlan> {
        if target.sender_id() != actor.user_id {
            return Err(FlareError::localized(
                ErrorCode::PermissionDenied,
                "sdk.message.delete.for_everyone.not_allowed",
            ));
        }
        Ok(MessageMutationPlan {
            transport_action: MessageTransportAction::Delete {
                conversation_id: target.conversation_id().to_string(),
                server_msg_id: target.server_id().to_string(),
                delete_type: DELETE_TYPE_SOFT,
                scope: DELETE_SCOPE_CONVERSATION_GLOBAL,
                reason,
                notify_others: true,
                target_user_id: None,
            },
            local_update: MessageLocalUpdate::Delete {
                message_id: target.server_id().to_string(),
            },
        })
    }

    pub fn plan_read_receipt(
        &self,
        actor: &MessageActor,
        conversation_id: &str,
        message_ids: Vec<String>,
        read_seq: u64,
    ) -> MessageMutationPlan {
        MessageMutationPlan {
            transport_action: MessageTransportAction::ReadReceipt {
                conversation_id: conversation_id.to_string(),
                user_id: actor.user_id.clone(),
                message_ids,
                read_seq,
            },
            local_update: MessageLocalUpdate::None,
        }
    }

    pub fn plan_typing(
        &self,
        actor: &MessageActor,
        conversation_id: &str,
        typing: bool,
    ) -> MessageMutationPlan {
        MessageMutationPlan {
            transport_action: MessageTransportAction::Typing {
                conversation_id: conversation_id.to_string(),
                user_id: actor.user_id.clone(),
                typing,
            },
            local_update: MessageLocalUpdate::None,
        }
    }

    pub fn plan_reaction(
        &self,
        actor: &MessageActor,
        conversation_id: &str,
        server_msg_id: &str,
        emoji: &str,
        action: ReactionAction,
    ) -> MessageMutationPlan {
        MessageMutationPlan {
            transport_action: MessageTransportAction::Reaction {
                conversation_id: conversation_id.to_string(),
                server_msg_id: server_msg_id.to_string(),
                user_id: actor.user_id.clone(),
                emoji: emoji.to_string(),
                action: action as i32,
            },
            local_update: MessageLocalUpdate::None,
        }
    }

    pub fn plan_pin(
        &self,
        actor: &MessageActor,
        conversation_id: &str,
        server_msg_id: &str,
    ) -> MessageMutationPlan {
        MessageMutationPlan {
            transport_action: MessageTransportAction::Pin {
                conversation_id: conversation_id.to_string(),
                server_msg_id: server_msg_id.to_string(),
                pinned_by: actor.user_id.clone(),
            },
            local_update: MessageLocalUpdate::None,
        }
    }

    pub fn plan_unpin(&self, conversation_id: &str, server_msg_id: &str) -> MessageMutationPlan {
        MessageMutationPlan {
            transport_action: MessageTransportAction::Unpin {
                conversation_id: conversation_id.to_string(),
                server_msg_id: server_msg_id.to_string(),
            },
            local_update: MessageLocalUpdate::None,
        }
    }

    pub fn plan_mark(
        &self,
        actor: &MessageActor,
        conversation_id: &str,
        server_msg_id: &str,
        mark_type: MarkType,
        color: &str,
    ) -> MessageMutationPlan {
        MessageMutationPlan {
            transport_action: MessageTransportAction::Mark {
                conversation_id: conversation_id.to_string(),
                server_msg_id: server_msg_id.to_string(),
                user_id: actor.user_id.clone(),
                mark_type: mark_type as i32,
                color: color.to_string(),
            },
            local_update: MessageLocalUpdate::None,
        }
    }

    pub fn plan_unmark(
        &self,
        actor: &MessageActor,
        conversation_id: &str,
        server_msg_id: &str,
        mark_type: MarkType,
    ) -> MessageMutationPlan {
        MessageMutationPlan {
            transport_action: MessageTransportAction::Unmark {
                conversation_id: conversation_id.to_string(),
                server_msg_id: server_msg_id.to_string(),
                user_id: actor.user_id.clone(),
                mark_type: mark_type as i32,
            },
            local_update: MessageLocalUpdate::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MessageLocalUpdate, MessageMutationService};
    use crate::domain::{MessageActor, MessageTransportAction, ResolvedMessage};
    use crate::model::IMMessage;
    use flare_proto::common::Message;

    #[test]
    fn delete_for_everyone_requires_sender_match_actor() {
        let service = MessageMutationService;
        let actor = MessageActor::require("u1".to_string()).unwrap();
        let mut message = IMMessage::new(Message::default());
        message.conversation_id = "c1".to_string();
        message.server_id = "s1".to_string();
        message.sender_id = "u2".to_string();

        let result = service.plan_delete_for_everyone(&actor, &ResolvedMessage::new(message), None);

        assert!(result.is_err());
    }

    #[test]
    fn delete_for_self_produces_delete_transport_and_local_delete() {
        let service = MessageMutationService;
        let actor = MessageActor::require("u1".to_string()).unwrap();
        let mut message = IMMessage::new(Message::default());
        message.conversation_id = "c1".to_string();
        message.server_id = "s1".to_string();
        message.sender_id = "u1".to_string();

        let plan = service.plan_delete_for_self(
            &actor,
            &ResolvedMessage::new(message),
            Some("r".to_string()),
        );

        match plan.transport_action {
            MessageTransportAction::Delete {
                conversation_id,
                server_msg_id,
                notify_others,
                ..
            } => {
                assert_eq!(conversation_id, "c1");
                assert_eq!(server_msg_id, "s1");
                assert!(!notify_others);
            }
            _ => panic!("expected delete transport action"),
        }

        match plan.local_update {
            MessageLocalUpdate::Delete { message_id } => assert_eq!(message_id, "s1"),
            _ => panic!("expected local delete update"),
        }
    }
}
