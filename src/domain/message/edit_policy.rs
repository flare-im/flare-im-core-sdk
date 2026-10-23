//! 入站编辑的纵深防御。
//!
//! 插件卡（顶层 APP_CARD `flare.plugin.message.v1`）由平台代发并绑定插件实体，Core 不允许编辑它，
//! 也不允许把别的消息编辑成它。SDK 应用入站编辑事件（同步回放与实时下发）时再拦一道：一条编辑
//! 若改的是插件卡，或会把普通消息变成插件卡，就不落库、不发 `Edited`，事件按已处理计入游标
//! （重放也只会得到同样的结论）。
use flare_proto::common::MessageContent;

use crate::model::IMMessage;

/// 本地 `stored` 是否可以按 `new_content` 应用入站编辑：两边都不是顶层插件卡。
pub(crate) fn inbound_edit_allowed(stored: &IMMessage, new_content: &MessageContent) -> bool {
    !stored.is_top_level_plugin_card() && !IMMessage::content_is_plugin_card(new_content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flare_proto::common::message_content::Content;
    use flare_proto::common::{AppCardContent, Message, TextContent};
    use prost::Message as _;

    fn card_content() -> MessageContent {
        MessageContent {
            content: Some(Content::AppCard(AppCardContent {
                card_type: crate::content::message_elem::PLUGIN_MESSAGE_CARD_TYPE.into(),
                payload: br#"{"pluginId":"example.poll","contributionId":"example.poll.card","schemaVersion":1,"summary":"Poll","payload":{"pollId":"p1"}}"#.to_vec(),
                ..Default::default()
            })),
        }
    }
    fn text_content(text: &str) -> MessageContent {
        MessageContent {
            content: Some(Content::Text(TextContent {
                text: text.into(),
                ..Default::default()
            })),
        }
    }
    fn stored(content: MessageContent) -> IMMessage {
        let mut message = IMMessage::new(Message {
            server_id: "s1".into(),
            conversation_id: "conv-1".into(),
            conversation_seq: 1,
            content: Some(content.clone()),
            ..Default::default()
        });
        message.encoded_content = content.encode_to_vec();
        message
    }

    #[test]
    fn edits_never_make_or_unmake_a_plugin_card() {
        let text = stored(text_content("hello"));
        let card = stored(card_content());
        assert!(!text.is_top_level_plugin_card() && card.is_top_level_plugin_card());
        assert!(inbound_edit_allowed(&text, &text_content("hello!")));
        assert!(
            !inbound_edit_allowed(&text, &card_content()),
            "a text turned into a card"
        );
        assert!(
            !inbound_edit_allowed(&card, &text_content("gone")),
            "a card turned into text"
        );
        assert!(
            !inbound_edit_allowed(&card, &card_content()),
            "a card is not editable"
        );
        // A same-named Custom is not a plugin card.
        let custom = MessageContent {
            content: Some(Content::Custom(flare_proto::common::CustomContent {
                r#type: crate::content::message_elem::PLUGIN_MESSAGE_CARD_TYPE.into(),
                ..Default::default()
            })),
        };
        assert!(inbound_edit_allowed(&text, &custom));
    }
}
