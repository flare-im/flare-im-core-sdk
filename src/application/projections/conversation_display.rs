//! Conversation display projection.
//!
//! Core owns the deterministic conversation title/avatar projection. Business SDKs
//! provide typed snapshots; UI should render `Conversation.display_name` directly.

use crate::infrastructure::persistence::StoreProvider;
use crate::model::Conversation;
use crate::model::conversation::ConversationType;
use crate::shared::error::Result;

#[derive(Clone, Debug)]
pub struct ConversationDisplaySnapshot {
    pub conversation_id: String,
    pub conversation_type: ConversationType,
    pub business_type: String,
    pub channel_id: String,
    pub display_name: String,
    pub avatar_url: String,
    /// `None` means leave remark unchanged; `Some("")` clears remark.
    pub remark: Option<String>,
    pub members_count: Option<u32>,
    pub create_if_missing: bool,
}

impl ConversationDisplaySnapshot {
    pub fn new(
        conversation_id: impl Into<String>,
        conversation_type: ConversationType,
        channel_id: impl Into<String>,
    ) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            conversation_type,
            business_type: String::new(),
            channel_id: channel_id.into(),
            display_name: String::new(),
            avatar_url: String::new(),
            remark: None,
            members_count: None,
            create_if_missing: false,
        }
    }
}

pub struct ConversationDisplayProjectionApplier;

impl ConversationDisplayProjectionApplier {
    pub async fn apply(
        stores: &StoreProvider,
        snapshot: ConversationDisplaySnapshot,
    ) -> Result<Option<String>> {
        let conversation_id = snapshot.conversation_id.trim();
        if conversation_id.is_empty() {
            return Ok(None);
        }

        let existing = stores.conversations.get(conversation_id).await?;
        if existing.is_none() && !snapshot.create_if_missing {
            return Ok(None);
        }

        let mut conversation = existing
            .clone()
            .unwrap_or_else(|| Conversation::from_conversation_id(conversation_id.to_string()));
        let before = conversation.clone();

        if conversation.conversation_type == ConversationType::Unspecified {
            conversation.conversation_type = snapshot.conversation_type;
        }
        if !snapshot.business_type.trim().is_empty() {
            conversation.business_type = snapshot.business_type.trim().to_string();
        } else if conversation.business_type.trim().is_empty()
            && snapshot.conversation_type != ConversationType::Unspecified
        {
            conversation.business_type = snapshot.conversation_type.as_str().to_string();
        }
        if !snapshot.channel_id.trim().is_empty() {
            conversation.channel_id = snapshot.channel_id.trim().to_string();
        }
        if let Some(count) = snapshot.members_count {
            conversation.members_count = count;
        }

        let peer_profile = if conversation.conversation_type.is_single_chat_conversation()
            && !conversation.channel_id.trim().is_empty()
        {
            stores
                .user_profiles_or_memory()
                .get(conversation.channel_id.trim())
                .await?
        } else {
            None
        };

        if let Some(remark) = snapshot.remark.as_ref() {
            conversation.remark = normalize_text(remark);
        }
        if let Some(avatar) = normalize_text(&snapshot.avatar_url) {
            conversation.avatar_url = avatar;
        } else if conversation.avatar_url.trim().is_empty()
            && let Some(profile) = peer_profile.as_ref()
            && !profile.avatar_url.trim().is_empty()
        {
            conversation.avatar_url = profile.avatar_url.trim().to_string();
        }

        let profile_name = peer_profile
            .as_ref()
            .map(|profile| profile.display_name())
            .and_then(normalize_text);
        let existing_name_allowed =
            snapshot.display_name.trim().is_empty() && snapshot.remark.is_none();
        let primary_name = normalize_text(&snapshot.display_name)
            .or(profile_name)
            .or_else(|| {
                if existing_name_allowed {
                    normalize_text(&conversation.display_name)
                } else {
                    None
                }
            });
        conversation.display_name = resolve_display_name(
            conversation.conversation_type,
            conversation.remark.as_deref(),
            primary_name.as_deref(),
            &conversation.channel_id,
            &conversation.conversation_id,
        );

        let now = crate::shared::util::id::now_millis();
        if conversation.created_at == 0 {
            conversation.created_at = now;
        }
        if conversation.updated_at == 0 || conversation_display_changed(&before, &conversation) {
            conversation.updated_at = now;
        }

        if existing.is_none() || conversation_display_changed(&before, &conversation) {
            stores.conversations.save_one(&conversation).await?;
            return Ok(Some(conversation.conversation_id));
        }
        Ok(None)
    }
}

fn conversation_display_changed(left: &Conversation, right: &Conversation) -> bool {
    left.conversation_type != right.conversation_type
        || left.business_type != right.business_type
        || left.channel_id != right.channel_id
        || left.display_name != right.display_name
        || left.avatar_url != right.avatar_url
        || left.remark != right.remark
        || left.members_count != right.members_count
}

fn normalize_text(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

pub fn resolve_display_name(
    conversation_type: ConversationType,
    remark: Option<&str>,
    primary_name: Option<&str>,
    channel_id: &str,
    conversation_id: &str,
) -> String {
    if let Some(value) = remark.and_then(normalize_text) {
        return value;
    }
    if let Some(value) = primary_name.and_then(normalize_text) {
        return value;
    }
    if let Some(value) = normalize_text(channel_id) {
        return value;
    }
    if let Some(value) = normalize_text(conversation_id) {
        return value;
    }
    match conversation_type {
        ConversationType::Group => "群聊",
        ConversationType::Ai => "AI",
        ConversationType::System => "系统通知",
        ConversationType::Customer => "客服",
        ConversationType::Temp => "临时会话",
        ConversationType::Channel => "频道",
        ConversationType::Broadcast => "广播",
        ConversationType::Single | ConversationType::Unspecified => "单聊",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::resolve_display_name;
    use crate::model::conversation::ConversationType;

    #[test]
    fn remark_has_highest_priority() {
        let name = resolve_display_name(
            ConversationType::Single,
            Some("备注名"),
            Some("好友昵称"),
            "u2",
            "cid",
        );
        assert_eq!(name, "备注名");
    }

    #[test]
    fn group_name_wins_before_channel_id() {
        let name = resolve_display_name(ConversationType::Group, None, Some("研发群"), "g1", "cid");
        assert_eq!(name, "研发群");
    }

    #[test]
    fn single_falls_back_to_channel_id() {
        let name = resolve_display_name(ConversationType::Single, None, None, "u2", "cid");
        assert_eq!(name, "u2");
    }
}
