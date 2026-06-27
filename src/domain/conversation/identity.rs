use crate::domain::conversation::id as conversation_id;
use crate::model::Conversation;
use crate::model::conversation::ConversationType;
use crate::model::message::IMMessage;
use crate::shared::error::{ErrorCode, FlareError, Result};

pub struct ConversationIdentityService;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationIdRewrite {
    pub from: String,
    pub to: String,
}

impl ConversationIdentityService {
    pub fn resolve_conversation_id(
        &self,
        current_user_id: &str,
        source_id: &str,
        conversation_type: &ConversationType,
    ) -> Result<String> {
        let current_user_id = current_user_id.trim();
        let source_id = source_id.trim();
        if requires_current_user(conversation_type) && current_user_id.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "当前用户不能为空",
            ));
        }
        if requires_source_id(conversation_type) && source_id.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "会话来源不能为空",
            ));
        }

        let conversation_id = match conversation_type {
            ConversationType::Single => {
                conversation_id::generate_single_chat_conversation_id(current_user_id, source_id)
            }
            ConversationType::Group => conversation_id::generate_group_conversation_id(source_id),
            ConversationType::Ai => {
                conversation_id::generate_ai_conversation_id(current_user_id, source_id)
            }
            ConversationType::Customer => {
                conversation_id::generate_customer_conversation_id(current_user_id, source_id)
            }
            ConversationType::System => {
                conversation_id::generate_system_conversation_id(source_id, None)
            }
            ConversationType::Temp => conversation_id::generate_temp_conversation_id(),
            ConversationType::Channel => {
                conversation_id::generate_channel_conversation_id(source_id)
            }
            ConversationType::Broadcast => {
                conversation_id::generate_broadcast_conversation_id(source_id)
            }
            _ => {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "不支持的会话类型",
                ));
            }
        };
        if conversation_id.trim().is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "生成的 conversationId 为空",
            ));
        }
        if let Err(error) = conversation_id::validate_conversation_id(&conversation_id) {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                format!("生成的 conversationId 非法: {error}"),
            ));
        }
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

    pub fn canonicalize_single_chat_conversation(
        conversation: &mut Conversation,
        current_user_id: &str,
    ) -> Option<ConversationIdRewrite> {
        if !conversation.conversation_type.is_single_chat_conversation() {
            return None;
        }
        let me = current_user_id.trim();
        if me.is_empty() {
            return None;
        }
        let peer = single_chat_peer_from_parts(
            me,
            &conversation.channel_id,
            conversation.last_sender_id.as_deref().unwrap_or_default(),
            "",
        )
        .or_else(|| single_chat_peer_from_members(me, conversation))?;
        conversation.channel_id = peer.clone();
        let canonical = conversation_id::generate_single_chat_conversation_id(me, &peer);
        let old = conversation.conversation_id.trim().to_string();
        conversation.conversation_id = canonical.clone();
        if old.is_empty() || old == canonical {
            return None;
        }
        Some(ConversationIdRewrite {
            from: old,
            to: canonical,
        })
    }

    pub fn canonicalize_single_chat_message(
        message: &mut IMMessage,
        current_user_id: &str,
    ) -> Option<ConversationIdRewrite> {
        let conversation_type = ConversationType::from_proto_int(message.conversation_type);
        if !conversation_type.is_single_chat_conversation() {
            return None;
        }
        let me = current_user_id.trim();
        if me.is_empty() {
            return None;
        }
        let peer = single_chat_peer_from_parts(me, &message.channel_id, &message.sender_id, "")?;
        message.channel_id = peer.clone();
        let canonical = conversation_id::generate_single_chat_conversation_id(me, &peer);
        let old = message.conversation_id.trim().to_string();
        message.conversation_id = canonical.clone();
        if old.is_empty() || old == canonical {
            return None;
        }
        Some(ConversationIdRewrite {
            from: old,
            to: canonical,
        })
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

fn single_chat_peer_from_parts(
    current_user_id: &str,
    channel_id: &str,
    sender_id: &str,
    fallback: &str,
) -> Option<String> {
    for candidate in [channel_id, sender_id, fallback] {
        let candidate = candidate.trim();
        if !candidate.is_empty() && candidate != current_user_id {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Single chats where the local user sent the last message — or summaries the
/// server returns without `channel_id` — cannot reveal the peer from
/// `channel_id`/`last_sender_id` alone (both resolve to the current user). Fall
/// back to the membership preview / participant list: the peer is the one member
/// that isn't the current user. Without this, such a summary keeps its
/// server-assigned id and survives as a duplicate row alongside the canonical
/// one, so the conversation list shows an empty "no message" row.
fn single_chat_peer_from_members(
    current_user_id: &str,
    conversation: &Conversation,
) -> Option<String> {
    conversation
        .member_preview
        .iter()
        .chain(conversation.participants.iter())
        .map(|member| member.user_id.trim())
        .find(|user_id| !user_id.is_empty() && *user_id != current_user_id)
        .map(str::to_string)
}

fn requires_current_user(conversation_type: &ConversationType) -> bool {
    matches!(
        conversation_type,
        ConversationType::Single | ConversationType::Ai | ConversationType::Customer
    )
}

fn requires_source_id(conversation_type: &ConversationType) -> bool {
    !matches!(conversation_type, ConversationType::Temp)
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

    #[test]
    fn resolve_conversation_id_rejects_blank_single_source() {
        let service = ConversationIdentityService;

        let err = service
            .resolve_conversation_id("u1", "   ", &ConversationType::Single)
            .expect_err("blank source must be rejected");

        assert_eq!(err.code(), Some(ErrorCode::InvalidParameter));
    }

    #[test]
    fn resolve_conversation_id_returns_canonical_non_empty_id() {
        let service = ConversationIdentityService;

        let conversation_id = service
            .resolve_conversation_id(" u1 ", " u2 ", &ConversationType::Single)
            .expect("valid users should resolve");

        assert!(!conversation_id.trim().is_empty());
        assert!(conversation_id::validate_conversation_id(&conversation_id).is_ok());
    }

    #[test]
    fn canonicalizes_single_chat_conversation_id_from_channel() {
        let mut conversation = Conversation::from_conversation_id("peer-1".to_string());
        conversation.conversation_type = ConversationType::Single;
        conversation.channel_id = "peer-1".to_string();

        let rewrite = ConversationIdentityService::canonicalize_single_chat_conversation(
            &mut conversation,
            "me-1",
        )
        .expect("wrong single chat id should rewrite");

        let expected = conversation_id::generate_single_chat_conversation_id("me-1", "peer-1");
        assert_eq!(conversation.conversation_id, expected);
        assert_eq!(conversation.channel_id, "peer-1");
        assert_eq!(rewrite.from, "peer-1");
        assert_eq!(rewrite.to, expected);
    }

    #[test]
    fn canonicalizes_single_chat_conversation_id_from_members_when_local_user_is_last_sender() {
        use crate::model::conversation::ConversationParticipant;

        // Server summary with no channel_id whose last message the local user
        // sent: peer is recoverable only from the membership preview. Without the
        // member fallback this keeps the server id and survives as a duplicate
        // "no message" row in the conversation list.
        let mut conversation = Conversation::from_conversation_id("srv-conv-1".to_string());
        conversation.conversation_type = ConversationType::Single;
        conversation.channel_id = String::new();
        conversation.last_sender_id = Some("me-1".to_string());
        conversation.member_preview = vec![
            ConversationParticipant {
                user_id: "me-1".to_string(),
                ..Default::default()
            },
            ConversationParticipant {
                user_id: "peer-9".to_string(),
                ..Default::default()
            },
        ];

        let rewrite = ConversationIdentityService::canonicalize_single_chat_conversation(
            &mut conversation,
            "me-1",
        )
        .expect("member-derived peer should rewrite server id to canonical");

        let expected = conversation_id::generate_single_chat_conversation_id("me-1", "peer-9");
        assert_eq!(conversation.conversation_id, expected);
        assert_eq!(conversation.channel_id, "peer-9");
        assert_eq!(rewrite.from, "srv-conv-1");
        assert_eq!(rewrite.to, expected);
    }
}
