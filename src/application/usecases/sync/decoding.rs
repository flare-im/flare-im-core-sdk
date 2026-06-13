use flare_proto::common::{
    SingleConversationSyncRes, event::Payload as DomainEventPayload,
    sync_slice_item::Payload as SyncSliceItemPayload,
};

use super::models::DecodedSingleConversationItems;

pub(crate) fn decode_single_conversation_items(
    response: &SingleConversationSyncRes,
    known_seq: u64,
) -> DecodedSingleConversationItems {
    let mut events = Vec::new();
    let mut messages = Vec::new();
    let mut applied_item_seqs = Vec::new();
    let mut has_decoded_items = false;

    for item in &response.items {
        match &item.payload {
            Some(SyncSliceItemPayload::Skip(_)) | Some(SyncSliceItemPayload::Tombstone(_)) => {
                if item.conversation_seq > known_seq {
                    applied_item_seqs.push(item.conversation_seq);
                    has_decoded_items = true;
                }
            }
            Some(SyncSliceItemPayload::Event(event)) => {
                if !is_valid_sync_event(event) {
                    continue;
                }
                has_decoded_items = true;
                let item_seq = sync_item_seq(item.conversation_seq, event.conversation_seq);
                if let Some(DomainEventPayload::Message(message)) = &event.payload {
                    if item_seq > known_seq {
                        applied_item_seqs.push(item_seq);
                        messages.push(crate::model::IMMessage::new(message.clone()));
                    }
                } else {
                    if item_seq > known_seq {
                        applied_item_seqs.push(item_seq);
                    }
                    events.push(event.clone());
                }
            }
            Some(SyncSliceItemPayload::Message(message)) => {
                if !is_valid_sync_message(message) {
                    continue;
                }
                has_decoded_items = true;
                let item_seq = sync_item_seq(item.conversation_seq, message.conversation_seq);
                if item_seq > known_seq {
                    applied_item_seqs.push(item_seq);
                    messages.push(crate::model::IMMessage::new(message.clone()));
                }
            }
            None => {
                tracing::warn!(
                    seq = item.conversation_seq,
                    "同步切片 item 缺少 payload，已拒绝猜测式解码"
                );
            }
        }
    }

    DecodedSingleConversationItems {
        messages,
        events,
        applied_item_seqs,
        has_decoded_items,
    }
}

fn sync_item_seq(item_seq: u64, payload_seq: u64) -> u64 {
    if item_seq > 0 { item_seq } else { payload_seq }
}

fn is_valid_sync_event(event: &flare_proto::common::Event) -> bool {
    event.payload.is_some() && event.r#type != 0
}

fn is_valid_sync_message(message: &flare_proto::common::Message) -> bool {
    message.conversation_seq > 0
        || !message.server_id.trim().is_empty()
        || !message.client_msg_id.trim().is_empty()
        || !message.conversation_id.trim().is_empty()
}
#[cfg(test)]
mod tests {
    use super::decode_single_conversation_items;
    use flare_proto::common::{
        SingleConversationSyncRes, SyncSkipItem, SyncSliceItem, sync_slice_item,
    };

    #[test]
    fn typed_skip_item_advances_seq_without_message() {
        let decoded = decode_single_conversation_items(
            &SingleConversationSyncRes {
                conversation_id: "conv-1".to_string(),
                items: vec![SyncSliceItem {
                    conversation_seq: 42,
                    created_at: 0,
                    payload: Some(sync_slice_item::Payload::Skip(SyncSkipItem {
                        reason: "visibility_filtered".to_string(),
                    })),
                }],
                max_conversation_seq: 42,
                ..Default::default()
            },
            41,
        );

        assert!(decoded.has_decoded_items);
        assert_eq!(decoded.applied_item_seqs, vec![42]);
        assert!(decoded.messages.is_empty());
        assert!(decoded.events.is_empty());
    }

    #[test]
    fn unspecified_item_is_not_guessed_from_payload() {
        let decoded = decode_single_conversation_items(
            &SingleConversationSyncRes {
                conversation_id: "conv-1".to_string(),
                items: vec![SyncSliceItem {
                    conversation_seq: 43,
                    created_at: 0,
                    payload: None,
                }],
                max_conversation_seq: 43,
                ..Default::default()
            },
            42,
        );

        assert!(!decoded.has_decoded_items);
        assert!(decoded.applied_item_seqs.is_empty());
        assert!(decoded.messages.is_empty());
        assert!(decoded.events.is_empty());
    }
}
