use crate::conversation;
use crate::error::{ErrorCode, FlareError, Result};
use crate::model::Conversation;
use crate::model::conversation::ConversationType;

pub struct ConversationIdentityService;

impl ConversationIdentityService {
    pub fn resolve_conversation_id(
        &self,
        current_user_id: &str,
        source_id: &str,
        conversation_type: &ConversationType,
    ) -> Result<String> {
        let conversation_id = match conversation_type {
            ConversationType::Single => {
                conversation::generate_single_chat_conversation_id(current_user_id, source_id)
            }
            ConversationType::Group => conversation::generate_group_conversation_id(source_id),
            ConversationType::Ai => {
                conversation::generate_ai_conversation_id(current_user_id, source_id)
            }
            ConversationType::Customer => {
                conversation::generate_customer_conversation_id(current_user_id, source_id)
            }
            ConversationType::System => {
                conversation::generate_system_conversation_id(source_id, None)
            }
            ConversationType::Temp => conversation::generate_temp_conversation_id(),
            _ => {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "不支持的会话类型",
                ));
            }
        };
        Ok(conversation_id)
    }

    pub fn merge_or_create(
        &self,
        existing: Option<Conversation>,
        conversation_id: String,
        source_id: &str,
        conversation_type: &ConversationType,
    ) -> (Conversation, bool) {
        match existing {
            Some(mut conversation) => {
                let mut needs_persist = false;
                if conversation.channel_id.is_empty() {
                    conversation.channel_id = source_id.to_string();
                    needs_persist = true;
                }
                (conversation, needs_persist)
            }
            None => {
                let mut summary = Conversation::from_conversation_id(conversation_id);
                summary.conversation_type = conversation_type.clone();
                summary.channel_id = source_id.to_string();
                summary.display_name = source_id.to_string();
                summary.business_type = conversation_type.as_str().to_string();
                (summary, true)
            }
        }
    }

    pub fn build_local_conversation(
        &self,
        conversation_id: &str,
        display_name: Option<&str>,
        conversation_type: ConversationType,
        business_type: &str,
        channel_id: String,
    ) -> Conversation {
        let mut summary = Conversation::from_conversation_id(conversation_id.to_string());
        summary.conversation_type = conversation_type;
        summary.business_type = business_type.to_string();
        summary.display_name = display_name.unwrap_or(channel_id.as_str()).to_string();
        summary.channel_id = channel_id;
        summary
    }
}
