mod common;

use flare_im_core_sdk::model::conversation::ConversationType;
use flare_im_core_sdk::model::preview_storage::{PreviewStoragePayload, keys};
use flare_im_core_sdk::model::{Conversation, Elem, MessageStatus};
use flare_im_core_sdk::prelude::*;

#[test]
fn builder_requires_explicit_store_provider() {
    let result = IMClient::builder().build();

    assert!(result.is_err());
    let err = result.err().expect("builder without stores should fail");
    assert!(err.to_string().contains("StoreProvider"));
}

#[tokio::test]
async fn builder_does_not_expose_connected_facades_without_active_session() {
    let client = common::create_test_client_no_connect().await;

    assert_eq!(client.current_user_id().await, None);
    assert_eq!(client.state(), SdkState::Disconnected);
    assert!(client.connected_apis().await.is_err());
    assert!(client.message().is_err());
    assert!(client.message_build().is_err());
    assert!(client.conversation().is_err());
    assert!(client.media().is_err());
    assert!(client.capability().is_err());
    assert!(client.presence().is_err());
    assert!(client.capability_registry().is_err());
}

#[tokio::test]
async fn memory_stores_follow_current_message_and_conversation_contracts() {
    let client = common::create_test_client_no_connect().await;
    let stores = client.stores_async().await.expect("stores");

    let mut conversation = Conversation {
        conversation_id: "conv_store".to_string(),
        conversation_type: ConversationType::Group,
        channel_id: "group_store".to_string(),
        display_name: "Store Contract".to_string(),
        max_seq: 41,
        unread_count: 2,
        ..Default::default()
    };
    conversation.ext.insert("scope".into(), "core".into());
    stores
        .conversations
        .save_one(&conversation)
        .await
        .expect("save conversation");

    let loaded = stores
        .conversations
        .get("conv_store")
        .await
        .expect("load conversation")
        .expect("conversation row");
    assert_eq!(loaded.conversation_type, ConversationType::Group);
    assert_eq!(loaded.max_seq, 41);

    let mut message =
        common::build_single_text("conv_store", "user_a", "group_store", "stored text");
    message.server_id = "server_store".to_string();
    message.conversation_seq = 42;
    message.status = MessageStatus::Persisted as i32;
    stores
        .messages
        .save_one(&message)
        .await
        .expect("save message");

    let by_client = stores
        .messages
        .get_by_client_msg_id(&message.client_msg_id)
        .await
        .expect("load message")
        .expect("message row");
    assert_eq!(by_client.conversation_seq, 42);
    let preview: PreviewStoragePayload =
        serde_json::from_str(&by_client.text_preview).expect("preview payload");
    assert_eq!(preview.k, keys::USER_TEXT);
    assert_eq!(
        preview.a.get("t").and_then(|value| value.as_str()),
        Some("stored text")
    );
    let Some(Elem::Text(text)) = by_client.content.as_ref() else {
        panic!("stored text message should decode to Elem::Text");
    };
    assert_eq!(text.text, "stored text");

    let page = stores
        .messages
        .get_by_conversation("conv_store", 0, 10)
        .await
        .expect("list messages");
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].server_id, "server_store");
}
