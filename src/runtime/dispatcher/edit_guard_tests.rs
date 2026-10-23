//! `EVENT_MESSAGE_EDIT` on the realtime path when the stored message cannot be read: the plugin
//! card check needs it, so the edit is not applied and the event is rolled back for sync to replay,
//! never applied unchecked.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use async_trait::async_trait;
use flare_proto::common::event::Payload;
use flare_proto::common::message_content::Content;
use flare_proto::common::{
    AppCardContent, Event, EventType, MessageContent, MessageEditEvent, TextContent,
};
use prost::Message as _;
use tokio::sync::RwLock;

use super::Dispatcher;
use crate::application::notification::{NotificationHandlerRegistry, NotificationInboundPipeline};
use crate::application::services::{EventDeduper, MessageDeduper};
use crate::domain::{EditApplyResult, MessageReader, MessageStore, MessageWriter};
use crate::infrastructure::persistence::memory_im::in_memory_im_provider;
use crate::infrastructure::protocol::DownlinkPayload;
use crate::kernel::event::{EventBus, EventReceiver, MessageEvent, SdkEvent};
use crate::model::IMMessage;
use crate::shared::error::{FlareError, Result};
use crate::spi::metrics::MetricsRecorder;

/// The in-memory store, except that `get` fails while `failing` is set; counts applied edits.
struct UnreadableWhileFailing {
    inner: Arc<dyn MessageStore>,
    failing: AtomicBool,
    edits: AtomicUsize,
}

#[async_trait]
impl MessageReader for UnreadableWhileFailing {
    async fn get(&self, message_id: &str) -> Result<Option<IMMessage>> {
        if self.failing.load(Ordering::SeqCst) {
            return Err(FlareError::general_error("simulated read failure"));
        }
        self.inner.get(message_id).await
    }

    async fn get_by_client_msg_id(&self, client_msg_id: &str) -> Result<Option<IMMessage>> {
        self.inner.get_by_client_msg_id(client_msg_id).await
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<IMMessage>> {
        self.inner
            .get_by_conversation(conversation_id, before_seq, limit)
            .await
    }

    async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<IMMessage>> {
        self.inner.search(keyword, limit).await
    }

    async fn search_in_conversation(
        &self,
        conversation_id: &str,
        keyword: &str,
        limit: u32,
    ) -> Result<Vec<IMMessage>> {
        self.inner
            .search_in_conversation(conversation_id, keyword, limit)
            .await
    }
}

#[async_trait]
impl MessageWriter for UnreadableWhileFailing {
    async fn save_batch(&self, messages: &[IMMessage]) -> Result<()> {
        self.inner.save_batch(messages).await
    }

    async fn save_one(&self, message: &IMMessage) -> Result<()> {
        self.inner.save_one(message).await
    }

    async fn update_status(&self, message_id: &str, status: i32) -> Result<u64> {
        self.inner.update_status(message_id, status).await
    }

    async fn update_content(&self, message_id: &str, new_content: Vec<u8>) -> Result<bool> {
        self.inner.update_content(message_id, new_content).await
    }

    async fn delete(&self, message_id: &str) -> Result<()> {
        self.inner.delete(message_id).await
    }

    async fn rewrite_conversation_id(
        &self,
        from_conversation_id: &str,
        to_conversation_id: &str,
    ) -> Result<u64> {
        self.inner
            .rewrite_conversation_id(from_conversation_id, to_conversation_id)
            .await
    }

    async fn update_after_ack(&self, client_msg_id: &str, message: &IMMessage) -> Result<()> {
        self.inner.update_after_ack(client_msg_id, message).await
    }
}

#[async_trait]
impl MessageStore for UnreadableWhileFailing {
    async fn apply_edit_event(
        &self,
        message_id: &str,
        new_content: Vec<u8>,
        event_seq: Option<u64>,
    ) -> Result<EditApplyResult> {
        self.edits.fetch_add(1, Ordering::SeqCst);
        self.inner
            .apply_edit_event(message_id, new_content, event_seq)
            .await
    }
}

fn text(text: &str) -> MessageContent {
    MessageContent {
        content: Some(Content::Text(TextContent {
            text: text.into(),
            ..Default::default()
        })),
    }
}

fn card() -> MessageContent {
    MessageContent {
        content: Some(Content::AppCard(AppCardContent {
            card_type: crate::content::message_elem::PLUGIN_MESSAGE_CARD_TYPE.into(),
            payload: br#"{"pluginId":"example.poll","contributionId":"example.poll.card","schemaVersion":1,"summary":"Poll","payload":{"pollId":"p1"}}"#.to_vec(),
            ..Default::default()
        })),
    }
}

fn stored(server_id: &str, seq: u64, content: MessageContent) -> IMMessage {
    let mut message = IMMessage::new(flare_proto::common::Message {
        server_id: server_id.into(),
        client_msg_id: format!("client-{server_id}"),
        conversation_id: "group-1".into(),
        conversation_seq: seq,
        content: Some(content.clone()),
        ..Default::default()
    });
    message.encoded_content = content.encode_to_vec();
    message
}

fn edit(event_id: &str, seq: u64, server_msg_id: &str, content: MessageContent) -> Event {
    Event {
        event_id: event_id.into(),
        conversation_id: "group-1".into(),
        conversation_seq: seq,
        r#type: EventType::EventMessageEdit as i32,
        payload: Some(Payload::Edit(MessageEditEvent {
            server_msg_id: server_msg_id.into(),
            new_content: Some(content),
            ..Default::default()
        })),
        ..Default::default()
    }
}

fn edited(events: &mut EventReceiver) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let SdkEvent::Message(MessageEvent::Edited { server_msg_id, .. }) = event {
            out.push(server_msg_id);
        }
    }
    out
}

#[tokio::test]
async fn an_edit_whose_message_cannot_be_read_is_not_applied_and_is_replayed() {
    let bus = EventBus::new();
    let mut events = bus.subscribe();
    let mut stores = in_memory_im_provider();
    let messages = Arc::new(UnreadableWhileFailing {
        inner: stores.messages.clone(),
        failing: AtomicBool::new(false),
        edits: AtomicUsize::new(0),
    });
    stores.messages = messages.clone();
    messages
        .save_one(&stored("card-1", 1, card()))
        .await
        .unwrap();
    messages
        .save_one(&stored("text-1", 2, text("hello")))
        .await
        .unwrap();
    let dispatcher = Dispatcher::new(
        bus.clone(),
        None,
        None,
        None,
        Some(stores),
        Arc::new(RwLock::new("u1".to_string())),
        EventDeduper::new(Some(64)),
        NotificationInboundPipeline::new(
            Arc::new(NotificationHandlerRegistry::new()),
            MessageDeduper::new(Some(64)),
            bus,
        ),
        MetricsRecorder::disabled(),
    );
    let dispatch = |event: Event| dispatcher.dispatch(DownlinkPayload::Event(event));
    let content_of = |id: &'static str| {
        let inner = messages.inner.clone();
        async move { inner.get(id).await.unwrap().unwrap().encoded_content }
    };

    // The store cannot be read: neither edit is applied, so the card cannot be overwritten.
    messages.failing.store(true, Ordering::SeqCst);
    let over_card = edit("e-card", 3, "card-1", text("not a card any more"));
    let on_text = edit("e-text", 4, "text-1", text("hello!"));
    dispatch(over_card.clone()).await.unwrap();
    dispatch(on_text.clone()).await.unwrap();
    assert_eq!(messages.edits.load(Ordering::SeqCst), 0);
    assert!(edited(&mut events).is_empty());
    assert_eq!(content_of("card-1").await, card().encode_to_vec());

    // Readable again, the same deliveries are processed (they were rolled back, not consumed):
    // the edit over the card is refused by the card check, the text edit applies.
    messages.failing.store(false, Ordering::SeqCst);
    dispatch(over_card).await.unwrap();
    dispatch(on_text).await.unwrap();
    assert_eq!(messages.edits.load(Ordering::SeqCst), 1);
    assert_eq!(edited(&mut events), ["text-1"]);
    assert_eq!(content_of("card-1").await, card().encode_to_vec());
    assert_eq!(content_of("text-1").await, text("hello!").encode_to_vec());
}
