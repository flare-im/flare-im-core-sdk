use flare_proto::common::{
    ConversationLight, ConversationPatch, ConversationSummary, SingleConversationSyncRes,
    event::Payload as DomainEventPayload,
};
use prost::Message;

use super::models::DecodedSingleConversationItems;

pub(crate) fn decode_single_conversation_items(
    response: &SingleConversationSyncRes,
    known_seq: u64,
) -> DecodedSingleConversationItems {
    let mut events = Vec::new();
    let mut messages = Vec::new();
    let mut has_decoded_items = false;

    for item in &response.items {
        if let Ok(message) = flare_proto::common::Message::decode(item.payload.as_slice()) {
            has_decoded_items = true;
            if message.seq > known_seq {
                messages.push(crate::model::IMMessage::new(message));
            }
            continue;
        }
        if let Ok(event) = flare_proto::common::Event::decode(item.payload.as_slice()) {
            has_decoded_items = true;
            if let Some(DomainEventPayload::Message(message)) = &event.payload {
                if message.seq > known_seq {
                    messages.push(crate::model::IMMessage::new(message.clone()));
                }
            } else {
                events.push(event);
            }
        }
    }

    DecodedSingleConversationItems {
        messages,
        events,
        has_decoded_items,
    }
}

pub(crate) fn patches_to_summaries(
    patches: &[ConversationPatch],
) -> Vec<ConversationSummary> {
    patches
        .iter()
        .filter_map(|patch| {
            if let Some(summary) = &patch.summary {
                return Some(summary.clone());
            }
            patch
                .light
                .as_ref()
                .map(|light| light_to_summary(light, &patch.conversation_id))
        })
        .collect()
}

fn light_to_summary(light: &ConversationLight, conversation_id: &str) -> ConversationSummary {
    let ext = light.ext.clone();
    ConversationSummary {
        conversation_id: conversation_id.to_string(),
        conversation_type: light.conversation_type.clone(),
        unread_count: light.unread_count,
        max_seq: light.max_seq,
        last_read_seq: light.last_read_seq,
        is_muted: light.is_muted,
        is_pinned: light.is_pinned,
        updated_at: light.updated_at.clone(),
        last_message: light.preview.clone(),
        channel_id: light.channel_id.clone(),
        ext,
        ..Default::default()
    }
}
