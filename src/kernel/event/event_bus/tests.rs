use super::{EventBus, PublishOutcome, RecoverableRwLock};
use crate::kernel::SyncRunContext;
use crate::kernel::event::{
    ConnectionEvent, ConnectionEventType, CustomEventDefinition, MessageEvent, MessageEventType,
    SdkEvent, SyncNotify,
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;
use tokio::sync::mpsc::{self, error::TryRecvError};

#[tokio::test]
async fn publish_drops_silent_sync_events_before_raw_subscribers() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe_raw();

    let outcome = bus.publish(SdkEvent::Sync(SyncNotify::Started {
        run: SyncRunContext::silent_gap_repair(),
    }));

    assert_eq!(outcome, PublishOutcome::DroppedSilentSync);
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
}

#[tokio::test]
async fn publish_keeps_user_visible_sync_events() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe_raw();

    let outcome = bus.publish(SdkEvent::Sync(SyncNotify::Started {
        run: SyncRunContext::initial_login(),
    }));

    assert!(matches!(
        outcome,
        PublishOutcome::Published { receiver_count } if receiver_count >= 1
    ));
    let received = rx.try_recv().expect("user visible sync event emitted");
    assert!(matches!(
        received,
        SdkEvent::Sync(SyncNotify::Started { .. })
    ));
}

#[tokio::test]
async fn raw_subscribers_share_event_and_cached_json() {
    let bus = EventBus::new();
    let mut first_rx = bus.subscribe_shared_raw();
    let mut second_rx = bus.subscribe_shared_raw();

    let outcome = bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));

    assert!(matches!(
        outcome,
        PublishOutcome::Published { receiver_count } if receiver_count == 2
    ));
    let first = first_rx
        .try_recv()
        .expect("first raw subscriber receives event");
    let second = second_rx
        .try_recv()
        .expect("second raw subscriber receives event");
    assert!(Arc::ptr_eq(&first, &second));

    let serialize_count = AtomicU64::new(0);
    let first_json = first.cached_json(|_| {
        serialize_count.fetch_add(1, Ordering::Relaxed);
        "{}".to_string()
    });
    let second_json = second.cached_json(|_| {
        serialize_count.fetch_add(1, Ordering::Relaxed);
        "{\"unexpected\":true}".to_string()
    });

    assert!(Arc::ptr_eq(&first_json, &second_json));
    assert_eq!(serialize_count.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn raw_subscriber_queue_honors_capacity_hint() {
    let bus = EventBus::with_capacity(1);
    let mut rx = bus.subscribe_raw();

    let first_outcome = bus.publish(SdkEvent::Sync(SyncNotify::Started {
        run: SyncRunContext::initial_login(),
    }));
    let second_outcome = bus.publish(SdkEvent::Sync(SyncNotify::Finished {
        run: SyncRunContext::initial_login(),
        phase: crate::kernel::event::SyncPhase::Init,
    }));

    assert!(matches!(
        first_outcome,
        PublishOutcome::Published { receiver_count } if receiver_count == 1
    ));
    assert_eq!(second_outcome, PublishOutcome::NoReceivers);

    let resync = rx.try_recv().expect("overflow emits resync marker first");
    assert!(matches!(
        resync,
        SdkEvent::Sync(SyncNotify::ResyncNeeded {
            scope,
            reason,
            dropped_events: 1,
        }) if scope == "global" && reason == "event_queue_lagged"
    ));
    let first = rx.try_recv().expect("first event retained");
    assert!(matches!(first, SdkEvent::Sync(SyncNotify::Started { .. })));
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
}

#[tokio::test]
async fn raw_subscriber_queue_drops_overflow_for_slow_consumers() {
    let bus = EventBus::with_capacity(2);
    let mut rx = bus.subscribe_raw();
    let total = 10_000usize;

    for i in 0..total {
        bus.publish(SdkEvent::Connection(ConnectionEvent::Disconnected {
            reason: format!("network-{i}"),
        }));
    }

    let resync = rx
        .try_recv()
        .expect("resync marker emitted before retained events");
    assert!(matches!(
        resync,
        SdkEvent::Sync(SyncNotify::ResyncNeeded {
            dropped_events,
            ..
        }) if dropped_events == (total - 2) as u64
    ));
    let received = rx.try_recv().expect("first burst event retained");
    assert!(matches!(
        received,
        SdkEvent::Connection(ConnectionEvent::Disconnected { reason })
            if reason == "network-0"
    ));
    let second = rx.try_recv().expect("second burst event retained");
    assert!(matches!(
        second,
        SdkEvent::Connection(ConnectionEvent::Disconnected { reason })
            if reason == "network-1"
    ));
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
}

#[tokio::test]
async fn shared_raw_subscriber_queue_emits_resync_after_overflow() {
    let bus = EventBus::with_capacity(1);
    let mut rx = bus.subscribe_shared_raw();

    bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));
    bus.publish(SdkEvent::Connection(ConnectionEvent::Disconnected {
        reason: "network".to_string(),
    }));

    let resync = rx
        .try_recv()
        .expect("shared raw overflow emits resync marker first");
    assert!(matches!(
        resync.event(),
        SdkEvent::Sync(SyncNotify::ResyncNeeded {
            scope,
            reason,
            dropped_events: 1,
        }) if scope == "global" && reason == "event_queue_lagged"
    ));
    let retained = rx.try_recv().expect("first shared event retained");
    assert!(matches!(
        retained.event(),
        SdkEvent::Connection(ConnectionEvent::Connected)
    ));
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
}

#[tokio::test]
async fn filtered_receiver_gets_resync_marker_after_own_queue_overflows() {
    let bus = EventBus::with_capacity(1);
    let mut rx = bus.subscribe_event_type(ConnectionEventType::Connected.into());

    bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));
    bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));

    let resync = rx.try_recv().expect("resync marker emitted first");
    assert!(matches!(
        resync,
        SdkEvent::Sync(SyncNotify::ResyncNeeded {
            dropped_events: 1,
            ..
        })
    ));
    assert!(matches!(
        rx.try_recv().expect("retained matching event follows"),
        SdkEvent::Connection(ConnectionEvent::Connected)
    ));
}

#[tokio::test]
async fn filtered_subscribers_are_isolated_under_burst() {
    let bus = EventBus::new();
    let mut connection_rx = bus.subscribe_event_type(ConnectionEventType::Connected.into());
    let mut custom_rx = bus.subscribe_event_type(MessageEventType::Custom.into());
    let total = 1_000usize;

    for i in 0..total {
        bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));
        bus.publish(SdkEvent::Message(MessageEvent::Custom {
            conversation_id: "c1".into(),
            event: CustomEventDefinition::new("app.iso", format!("event_{i}")).build(vec![]),
        }));
    }

    for _ in 0..total {
        assert!(matches!(
            connection_rx.try_recv().expect("connection event retained"),
            SdkEvent::Connection(ConnectionEvent::Connected)
        ));
    }
    assert!(matches!(connection_rx.try_recv(), Err(TryRecvError::Empty)));

    for i in 0..total {
        assert!(matches!(
            custom_rx.try_recv().expect("custom event retained"),
            SdkEvent::Message(MessageEvent::Custom { event, .. })
                if event.name == format!("event_{i}")
        ));
    }
    assert!(matches!(custom_rx.try_recv(), Err(TryRecvError::Empty)));
}

#[test]
fn closed_raw_subscriber_is_pruned_on_next_publish() {
    let bus = EventBus::new();
    let rx = bus.subscribe_raw();

    assert_eq!(bus.subscribers.safe_read("event_bus").len(), 1);
    drop(rx);
    bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));
    assert_eq!(bus.subscribers.safe_read("event_bus").len(), 0);
}

#[test]
fn dropping_subscription_removes_registered_callback() {
    let bus = EventBus::new();
    let subscription = bus.on_connected(|| {});

    assert_eq!(bus.on_connected.safe_read("event_bus").len(), 1);
    drop(subscription);
    assert_eq!(bus.on_connected.safe_read("event_bus").len(), 0);
}

#[test]
fn callback_dispatch_gates_track_subscription_lifecycle() {
    let bus = EventBus::new();

    assert!(!bus.has_typed_callbacks());
    assert!(!bus.has_route_callbacks());
    assert!(!bus.has_any_callbacks());

    let typed = bus.on_connected(|| {});
    let route = bus.on_event_type(ConnectionEventType::Connected.into(), |_| {});
    let any = bus.on_any(|_| {});

    assert!(bus.has_typed_callbacks());
    assert!(bus.has_route_callbacks());
    assert!(bus.has_any_callbacks());
    assert_eq!(bus.typed_callback_count.load(Ordering::Acquire), 1);
    assert_eq!(bus.route_callback_count.load(Ordering::Acquire), 1);
    assert_eq!(bus.any_callback_count.load(Ordering::Acquire), 1);

    drop(typed);
    drop(route);
    drop(any);

    assert!(!bus.has_typed_callbacks());
    assert!(!bus.has_route_callbacks());
    assert!(!bus.has_any_callbacks());
}

#[tokio::test]
async fn raw_only_publish_keeps_callback_gates_closed() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe_raw();

    let outcome = bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));

    assert!(matches!(
        outcome,
        PublishOutcome::Published { receiver_count } if receiver_count == 1
    ));
    assert!(!bus.has_typed_callbacks());
    assert!(!bus.has_route_callbacks());
    assert!(!bus.has_any_callbacks());
    assert!(matches!(
        rx.try_recv().expect("raw subscriber receives event"),
        SdkEvent::Connection(ConnectionEvent::Connected)
    ));
}

#[test]
fn dropped_route_and_any_callbacks_leave_no_receivers() {
    let bus = EventBus::new();
    let route = bus.on_event_type(ConnectionEventType::Connected.into(), |_| {});
    let any = bus.on_any(|_| {});

    assert!(matches!(
        bus.publish(SdkEvent::Connection(ConnectionEvent::Connected)),
        PublishOutcome::Published { receiver_count } if receiver_count == 2
    ));

    drop(route);
    drop(any);

    assert_eq!(
        bus.publish(SdkEvent::Connection(ConnectionEvent::Connected)),
        PublishOutcome::NoReceivers
    );
}

#[tokio::test]
async fn connected_callback_replays_last_state_after_registration() {
    let bus = EventBus::new();
    bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));

    let (tx, mut rx) = mpsc::channel(1);
    let _subscription = bus.on_connected(move || {
        let _ = tx.try_send(());
    });

    tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("connected replay callback invoked")
        .expect("connected replay value");
}

#[test]
fn dropping_filtered_subscription_removes_registered_route() {
    let bus = EventBus::new();
    let subscription = bus.on_event_type(ConnectionEventType::Connected.into(), |_| {});

    assert_eq!(bus.routes.safe_read("event_bus").len(), 1);
    drop(subscription);
    assert_eq!(bus.routes.safe_read("event_bus").len(), 0);
}

#[tokio::test]
async fn filtered_receiver_skips_unmatched_events() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe_event_type(ConnectionEventType::Connected.into());

    bus.publish(SdkEvent::Connection(ConnectionEvent::Disconnected {
        reason: "network".into(),
    }));
    bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));

    let received = rx.try_recv().expect("connected event emitted");
    assert!(matches!(
        received,
        SdkEvent::Connection(ConnectionEvent::Connected)
    ));
}

#[tokio::test]
async fn event_type_routes_fan_out_to_multiple_handlers() {
    let bus = EventBus::new();
    let (tx, mut rx) = mpsc::unbounded_channel();

    let tx1 = tx.clone();
    let _sub1 = bus.on_event_type(ConnectionEventType::Connected.into(), move |_| {
        tx1.send("first").expect("first route sends");
    });
    let _sub2 = bus.on_event_type(ConnectionEventType::Connected.into(), move |_| {
        tx.send("second").expect("second route sends");
    });

    bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));

    let mut seen = vec![
        tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("first route invoked")
            .expect("first route value"),
        tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("second route invoked")
            .expect("second route value"),
    ];
    seen.sort_unstable();
    assert_eq!(seen, vec!["first", "second"]);
}

#[tokio::test]
async fn callbacks_fire_in_publish_order() {
    let bus = EventBus::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let _sub = bus.on_event_type(ConnectionEventType::Disconnected.into(), move |ev| {
        if let SdkEvent::Connection(ConnectionEvent::Disconnected { reason }) = ev.as_ref() {
            let _ = tx.send(reason.clone());
        }
    });

    const N: u32 = 64;
    for i in 0..N {
        bus.publish(SdkEvent::Connection(ConnectionEvent::Disconnected {
            reason: i.to_string(),
        }));
    }

    let mut got = Vec::with_capacity(N as usize);
    for _ in 0..N {
        let reason = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("callback invoked within window")
            .expect("callback value");
        got.push(reason);
    }
    let expected: Vec<String> = (0..N).map(|i| i.to_string()).collect();
    assert_eq!(
        got, expected,
        "single dispatch thread must preserve publish order"
    );
}

#[tokio::test]
async fn custom_event_definition_builds_and_filters_user_event() {
    let bus = EventBus::new();
    let definition = CustomEventDefinition::new("app.orders", "order_paid").with_version("v1");
    let mut rx = bus.subscribe_event_type(definition.event_type());

    bus.publish(SdkEvent::Message(MessageEvent::Custom {
        conversation_id: "c1".into(),
        event: CustomEventDefinition::new("app.orders", "order_cancelled")
            .with_version("v1")
            .build(Vec::new()),
    }));
    bus.publish(SdkEvent::Message(MessageEvent::Custom {
        conversation_id: "c1".into(),
        event: definition.build(b"{\"order_id\":\"o1\"}".to_vec()),
    }));

    let received = rx.try_recv().expect("custom event emitted");
    assert!(matches!(
        received,
        SdkEvent::Message(MessageEvent::Custom { event, .. })
            if event.namespace == "app.orders"
                && event.name == "order_paid"
                && event.version == "v1"
    ));
}

#[tokio::test]
async fn broad_custom_event_filter_matches_all_custom_events() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe_event_type(MessageEventType::Custom.into());

    bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));
    bus.publish(SdkEvent::Message(MessageEvent::Custom {
        conversation_id: "c1".into(),
        event: CustomEventDefinition::new("app.any", "anything").build(Vec::new()),
    }));

    let received = rx.try_recv().expect("custom event emitted");
    assert!(matches!(
        received,
        SdkEvent::Message(MessageEvent::Custom { .. })
    ));
}
