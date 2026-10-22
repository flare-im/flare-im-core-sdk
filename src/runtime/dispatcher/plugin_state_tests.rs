//! `EVENT_MESSAGE_PLUGIN_STATE` on the realtime path: the shared layer is applied by version,
//! announced once, and an event that arrives before its message is replayed after it.
use super::Dispatcher;
use crate::application::notification::{NotificationHandlerRegistry, NotificationInboundPipeline};
use crate::application::services::{EventDeduper, MessageDeduper};
use crate::domain::{MessageReader, MessageWriter};
use crate::infrastructure::persistence::memory_im::in_memory_im_provider;
use crate::infrastructure::protocol::DownlinkPayload;
use crate::kernel::event::{EventBus, EventReceiver, MessageEvent, SdkEvent};
use crate::model::IMMessage;
use crate::spi::metrics::MetricsRecorder;
use flare_proto::common::event::Payload;
use flare_proto::common::{Event, EventType, PluginStateEvent};
use std::sync::Arc;
use tokio::sync::RwLock;

fn card(server_id: &str, plugin_id: &str) -> IMMessage {
    let envelope = serde_json::json!({
        "pluginId": plugin_id, "contributionId": format!("{plugin_id}.card"), "schemaVersion": 1,
        "summary": "Poll", "payload": {"pollId": "poll-1"}
    });
    let mut message = IMMessage::new(flare_proto::common::Message::default());
    message.server_id = server_id.into();
    message.client_msg_id = format!("client-{server_id}");
    message.conversation_id = "group-1".into();
    message.conversation_seq = 1;
    message.content = Some(crate::model::Elem::Custom(
        crate::content::message_elem::CustomElem {
            r#type: "flare.plugin.message.v1".into(),
            payload: serde_json::to_vec(&envelope).unwrap(),
            description: String::new(),
            metadata: Default::default(),
        },
    ));
    message
}

fn state_event(event_id: &str, seq: u64, version: u64, values_json: &str) -> Event {
    Event {
        event_id: event_id.into(),
        conversation_id: "group-1".into(),
        conversation_seq: seq,
        r#type: EventType::EventMessagePluginState as i32,
        payload: Some(Payload::PluginState(PluginStateEvent {
            server_msg_id: "server-1".into(),
            plugin_id: "example.poll".into(),
            entity: "poll-1".into(),
            version,
            values_json: values_json.into(),
        })),
        ..Default::default()
    }
}

fn changes(events: &mut EventReceiver) -> Vec<(String, String, u64)> {
    let mut out = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let SdkEvent::Message(MessageEvent::PluginStateChanged {
            conversation_id,
            message_id,
            version,
        }) = event
        {
            out.push((conversation_id, message_id, version));
        }
    }
    out
}

#[tokio::test]
async fn plugin_state_event_applies_by_version_and_replays_after_its_message() {
    let bus = EventBus::new();
    let mut events = bus.subscribe();
    let stores = in_memory_im_provider();
    let messages = stores.messages.clone();
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

    // The state arrives before its card: nothing is applied and the event stays replayable.
    let first = state_event("e-3", 11, 3, r#"{"counts":[2,1],"voterCount":3}"#);
    dispatch(first.clone()).await.unwrap();
    assert!(changes(&mut events).is_empty());

    messages
        .save_one(&card("server-1", "example.poll"))
        .await
        .unwrap();
    dispatch(first.clone()).await.unwrap();
    assert_eq!(
        changes(&mut events),
        [("group-1".to_string(), "server-1".to_string(), 3)]
    );
    let stored = messages.get("server-1").await.unwrap().unwrap();
    let shared = stored
        .plugin_shared_state(None)
        .expect("shared layer stored");
    assert_eq!((shared.entity.as_str(), shared.version), ("poll-1", 3));
    assert_eq!(shared.values["counts"], serde_json::json!([2, 1]));

    // The same delivery, a duplicate of the version and an older version change nothing.
    dispatch(first).await.unwrap();
    dispatch(state_event("e-3b", 12, 3, r#"{"counts":[9,9]}"#))
        .await
        .unwrap();
    dispatch(state_event("e-2", 10, 2, r#"{"counts":[1,1]}"#))
        .await
        .unwrap();
    assert!(changes(&mut events).is_empty());
    assert_eq!(
        messages
            .get("server-1")
            .await
            .unwrap()
            .unwrap()
            .plugin_shared_state(None)
            .unwrap()
            .values["counts"],
        serde_json::json!([2, 1])
    );

    // Malformed snapshots and another plugin's state are dropped, never applied.
    dispatch(state_event("e-bad", 13, 4, r#"{"nested":{"a":1}}"#))
        .await
        .unwrap();
    let mut foreign = state_event("e-foreign", 14, 5, r#"{"counts":[0,0]}"#);
    if let Some(Payload::PluginState(state)) = foreign.payload.as_mut() {
        state.plugin_id = "example.other".into();
    }
    dispatch(foreign).await.unwrap();
    assert!(changes(&mut events).is_empty());

    dispatch(state_event(
        "e-6",
        15,
        6,
        r#"{"counts":[3,1],"closed":true}"#,
    ))
    .await
    .unwrap();
    assert_eq!(
        changes(&mut events),
        [("group-1".to_string(), "server-1".to_string(), 6)]
    );
}
