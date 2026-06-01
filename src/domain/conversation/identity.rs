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

    /// 单聊 `channel_id` 须为对端 user_id。历史壳层可能误存为当前用户 id 或昵称等非 id 字符串。
    pub fn repair_single_chat_channel(
        conversation: &mut Conversation,
        current_user_id: &str,
        peer_hint: Option<&str>,
    ) -> bool {
        if !conversation.conversation_type.is_single_chat_conversation() {
            return false;
        }
        let me = current_user_id.trim();
        if me.is_empty() {
            return false;
        }
        if let Some(peer) = peer_hint
            .map(str::trim)
            .filter(|p| !p.is_empty() && *p != me)
        {
            if conversation.channel_id != peer {
                conversation.channel_id = peer.to_string();
                return true;
            }
            return false;
        }
        let ch = conversation.channel_id.trim();
        if !ch.is_empty() && ch != me {
            return false;
        }
        if let Some(sender) = conversation
            .last_sender_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != me)
        {
            conversation.channel_id = sender.to_string();
            return true;
        }
        false
    }

    pub fn merge_or_create(
        &self,
        existing: Option<Conversation>,
        conversation_id: String,
        current_user_id: &str,
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
                if Self::repair_single_chat_channel(
                    &mut conversation,
                    current_user_id,
                    Some(source_id),
                ) {
                    needs_persist = true;
                }
                (conversation, needs_persist)
            }
            None => {
                let mut summary = Conversation::from_conversation_id(conversation_id);
                summary.conversation_type = *conversation_type;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::conversation::ConversationType;

    #[test]
    fn repair_single_chat_channel_overwrites_wrong_peer_with_hint() {
        let mut conversation = Conversation::from_conversation_id("1Atest".into());
        conversation.conversation_type = ConversationType::Single;
        conversation.channel_id = "123456".into();

        let changed = ConversationIdentityService::repair_single_chat_channel(
            &mut conversation,
            "me",
            Some("317501061667487232"),
        );
        assert!(changed);
        assert_eq!(conversation.channel_id, "317501061667487232");
    }
}
