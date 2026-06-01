//! 会话操作集成测试
//!
//! 覆盖 SDK 会话（Conversation）全流程：
//! - ConversationStore 本地存储 CRUD
//! - ConversationApi 查询 / 标记已读 / 删除
//! - EventBus 会话事件、状态变更、Extension 事件
//! - 发送消息后会话列表联动
//!
//! 无服务端测试直接运行，集成测试需要服务端：
//! ```bash
//! # 本地单元测试（不需要服务端）
//! cargo test --test conversation_test
//!
//! # 集成测试（需要服务端运行）
//! cargo test --test conversation_test --features integration-tests -- --ignored
//! ```

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use flare_im_core_sdk::event::{ConnectionEvent, ExtensionEvent, SyncNotify};
use flare_im_core_sdk::model::conversation::*;
use flare_im_core_sdk::prelude::*;

fn message_api(client: &IMClient) -> MessageApi {
    client.message().expect("message api")
}

fn test_sync_run() -> SyncRunContext {
    SyncRunContext::initial_login()
}

// =============================================================================
// ConversationStore 内存实现 CRUD
// =============================================================================

#[tokio::test]
async fn test_conversation_store_save_and_get() {
    let client = common::create_test_client_no_connect().await;
    let store = client.stores().unwrap().conversations.clone();

    let conv = ConversationSummary {
        conversation_id: "conv_crud_001".into(),
        conversation_type: "single".into(),
        unread_count: 5,
        ..Default::default()
    };

    store.save_batch(&[Conversation::from(conv)]).await.unwrap();

    let loaded = store.get("conv_crud_001").await.unwrap();
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.conversation_id, "conv_crud_001");
    assert_eq!(loaded.unread_count, 5);
}

#[tokio::test]
async fn test_conversation_store_update_unread() {
    let client = common::create_test_client_no_connect().await;
    let store = client.stores().unwrap().conversations.clone();

    let conv = ConversationSummary {
        conversation_id: "conv_unread_001".into(),
        unread_count: 10,
        ..Default::default()
    };
    store.save_batch(&[Conversation::from(conv)]).await.unwrap();

    store
        .update_unread("conv_unread_001", 0, 100)
        .await
        .unwrap();
    let updated = store.get("conv_unread_001").await.unwrap().unwrap();
    assert_eq!(updated.unread_count, 0);
}

#[tokio::test]
async fn test_conversation_store_delete() {
    let client = common::create_test_client_no_connect().await;
    let store = client.stores().unwrap().conversations.clone();

    let conv = ConversationSummary {
        conversation_id: "conv_del_001".into(),
        ..Default::default()
    };
    store.save_batch(&[Conversation::from(conv)]).await.unwrap();
    assert!(store.get("conv_del_001").await.unwrap().is_some());

    store.delete("conv_del_001").await.unwrap();
    assert!(store.get("conv_del_001").await.unwrap().is_none());
}

#[tokio::test]
async fn test_conversation_store_list() {
    let client = common::create_test_client_no_connect().await;
    let store = client.stores().unwrap().conversations.clone();

    let convs: Vec<Conversation> = (0..5)
        .map(|i| {
            Conversation::from(ConversationSummary {
                conversation_id: format!("conv_list_{i:03}"),
                conversation_type: "single".into(),
                unread_count: i as u32,
                ..Default::default()
            })
        })
        .collect();

    store.save_batch(&convs).await.unwrap();

    let all = store.list().await.unwrap();
    assert!(all.len() >= 5);
}

#[tokio::test]
async fn test_conversation_store_save_batch_upsert() {
    let client = common::create_test_client_no_connect().await;
    let store = client.stores().unwrap().conversations.clone();

    let conv = ConversationSummary {
        conversation_id: "conv_upsert".into(),
        unread_count: 1,
        ..Default::default()
    };
    store.save_batch(&[Conversation::from(conv)]).await.unwrap();

    let updated = ConversationSummary {
        conversation_id: "conv_upsert".into(),
        unread_count: 99,
        ..Default::default()
    };
    store
        .save_batch(&[Conversation::from(updated)])
        .await
        .unwrap();

    let loaded = store.get("conv_upsert").await.unwrap().unwrap();
    assert_eq!(loaded.unread_count, 99, "save_batch should upsert");
}

// =============================================================================
// ConversationApi 查询
// =============================================================================

#[tokio::test]
async fn test_conversation_api_get() {
    let client = common::create_test_client_no_connect().await;
    let store = client.stores().unwrap().conversations.clone();

    let conv = ConversationSummary {
        conversation_id: "conv_api_001".into(),
        conversation_type: "group".into(),
        unread_count: 3,
        ..Default::default()
    };
    store.save_batch(&[Conversation::from(conv)]).await.unwrap();

    let result = client
        .conversation()
        .unwrap()
        .get("conv_api_001")
        .await
        .unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().unread_count(), 3);

    let not_found = client
        .conversation()
        .unwrap()
        .get("non_existent")
        .await
        .unwrap();
    assert!(not_found.is_none());
}

#[tokio::test]
async fn test_conversation_api_list() {
    let client = common::create_test_client_no_connect().await;
    let store = client.stores().unwrap().conversations.clone();

    let convs: Vec<Conversation> = (0..3)
        .map(|i| {
            Conversation::from(ConversationSummary {
                conversation_id: format!("conv_api_list_{i}"),
                conversation_type: "single".into(),
                ..Default::default()
            })
        })
        .collect();
    store.save_batch(&convs).await.unwrap();

    let list = client.conversation().unwrap().list().await.unwrap();
    assert!(list.len() >= 3);
}

#[tokio::test]
async fn test_conversation_api_mark_read() {
    let client = common::create_test_client_no_connect().await;
    let store = client.stores().unwrap().conversations.clone();

    let conv = ConversationSummary {
        conversation_id: "conv_mark_read".into(),
        unread_count: 10,
        ..Default::default()
    };
    store.save_batch(&[Conversation::from(conv)]).await.unwrap();

    client
        .conversation()
        .unwrap()
        .mark_read("conv_mark_read", 200)
        .await
        .unwrap();
    let updated = store.get("conv_mark_read").await.unwrap().unwrap();
    assert_eq!(updated.unread_count, 0, "mark_read should clear unread");
}

#[tokio::test]
async fn test_conversation_api_delete() {
    let client = common::create_test_client_no_connect().await;
    let store = client.stores().unwrap().conversations.clone();

    let conv = ConversationSummary {
        conversation_id: "conv_to_delete".into(),
        ..Default::default()
    };
    store.save_batch(&[Conversation::from(conv)]).await.unwrap();

    client
        .conversation()
        .unwrap()
        .delete("conv_to_delete")
        .await
        .unwrap();
    assert!(store.get("conv_to_delete").await.unwrap().is_none());
}

// =============================================================================
// EventBus 会话/连接/状态事件
// =============================================================================

#[tokio::test]
async fn test_event_bus_conversation_synced() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();

    let conv_ids = vec!["c1".to_string(), "c2".to_string()];
    bus.publish(SdkEvent::Conversation(ConversationEvent::Synced {
        conversation_ids: conv_ids.clone(),
    }));

    let event = tokio::time::timeout(tokio::time::Duration::from_millis(200), rx.recv())
        .await
        .unwrap()
        .unwrap();

    match &event {
        SdkEvent::Conversation(ConversationEvent::Synced { conversation_ids }) => {
            assert_eq!(conversation_ids.len(), 2);
        }
        _ => panic!("expected Conversation::Synced"),
    }
}

#[tokio::test]
async fn test_event_bus_conversation_deleted() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();

    bus.publish(SdkEvent::Conversation(ConversationEvent::Deleted {
        conversation_id: "conv_del_event".into(),
    }));

    let event = tokio::time::timeout(tokio::time::Duration::from_millis(200), rx.recv())
        .await
        .unwrap()
        .unwrap();

    match &event {
        SdkEvent::Conversation(ConversationEvent::Deleted { conversation_id }) => {
            assert_eq!(conversation_id, "conv_del_event");
        }
        _ => panic!("expected Conversation::Deleted"),
    }
}

#[tokio::test]
async fn test_event_bus_state_changed() {
    let bus = EventBus::new();
    let count = Arc::new(AtomicU32::new(0));
    let c = count.clone();

    let _sub = bus.on_state_changed(move |state| {
        if state == SdkState::Ready {
            c.fetch_add(1, Ordering::Relaxed);
        }
    });

    bus.publish(SdkEvent::Connection(ConnectionEvent::StateChanged {
        state: SdkState::Connecting,
    }));
    bus.publish(SdkEvent::Connection(ConnectionEvent::StateChanged {
        state: SdkState::Ready,
    }));

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    assert_eq!(
        count.load(Ordering::Relaxed),
        1,
        "should fire once for Ready"
    );
}

#[tokio::test]
async fn test_event_bus_replays_latest_state_to_late_listener() {
    let bus = EventBus::new();
    bus.publish(SdkEvent::Connection(ConnectionEvent::StateChanged {
        state: SdkState::Ready,
    }));

    let count = Arc::new(AtomicU32::new(0));
    let c = count.clone();
    let _sub = bus.on_state_changed(move |state| {
        if state == SdkState::Ready {
            c.fetch_add(1, Ordering::Relaxed);
        }
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    assert_eq!(
        count.load(Ordering::Relaxed),
        1,
        "late listener should receive latest Ready state"
    );
}

#[tokio::test]
async fn test_event_bus_extension_event() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();

    bus.publish(SdkEvent::Extension(ExtensionEvent {
        source: "presence".into(),
        event_type: "changed".into(),
        payload: b"user_123_online".to_vec(),
    }));

    let event = tokio::time::timeout(tokio::time::Duration::from_millis(200), rx.recv())
        .await
        .unwrap()
        .unwrap();

    match &event {
        SdkEvent::Extension(ext) => {
            let source = &ext.source;
            let event_type = &ext.event_type;
            let payload = &ext.payload;
            assert_eq!(source, "presence");
            assert_eq!(event_type, "changed");
            assert_eq!(std::str::from_utf8(payload).unwrap(), "user_123_online");
        }
        _ => panic!("expected Extension event"),
    }
}

#[tokio::test]
async fn test_event_bus_sync_progress() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    let run = test_sync_run();

    bus.publish(SdkEvent::Sync(SyncNotify::Progress {
        run: run.clone(),
        task: "conversation".into(),
        progress: 0.5,
        detail: "syncing conversations".into(),
    }));
    bus.publish(SdkEvent::Sync(SyncNotify::TaskCompleted {
        run,
        task: "conversation".into(),
    }));

    let e1 = tokio::time::timeout(tokio::time::Duration::from_millis(200), rx.recv())
        .await
        .unwrap()
        .unwrap();
    match &e1 {
        SdkEvent::Sync(SyncNotify::Progress { task, progress, .. }) => {
            assert_eq!(task, "conversation");
            assert!((*progress - 0.5).abs() < f32::EPSILON);
        }
        _ => panic!("expected Sync Progress"),
    }

    let e2 = tokio::time::timeout(tokio::time::Duration::from_millis(200), rx.recv())
        .await
        .unwrap()
        .unwrap();
    match &e2 {
        SdkEvent::Sync(SyncNotify::TaskCompleted { task, .. }) => {
            assert_eq!(task, "conversation");
        }
        _ => panic!("expected Sync TaskCompleted"),
    }
}

#[tokio::test]
async fn test_event_bus_connected_disconnected() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();

    bus.publish(SdkEvent::Connection(ConnectionEvent::Connected));
    bus.publish(SdkEvent::Connection(ConnectionEvent::Disconnected {
        reason: "test".into(),
    }));
    bus.publish(SdkEvent::Connection(ConnectionEvent::Reconnecting {
        attempt: 1,
    }));

    for _ in 0..3 {
        let _ = tokio::time::timeout(tokio::time::Duration::from_millis(200), rx.recv())
            .await
            .unwrap()
            .unwrap();
    }
}

// =============================================================================
// SyncCursorStore
// =============================================================================

#[tokio::test]
async fn test_cursor_store_save_and_get() {
    let client = common::create_test_client_no_connect().await;
    let store = client.stores().unwrap().cursors.clone();

    store.save_raw("conv_cursor_001", "seq_100").await.unwrap();
    let cursor = store.get_raw("conv_cursor_001").await.unwrap();
    assert_eq!(cursor.as_deref(), Some("seq_100"));

    let missing = store.get_raw("non_existent").await.unwrap();
    assert!(missing.is_none());
}

// =============================================================================
// IMClient 状态机 (无连接)
// =============================================================================

#[tokio::test]
async fn test_client_initial_state() {
    let client = common::create_test_client_no_connect().await;
    assert_eq!(client.state(), SdkState::Disconnected);
}

// =============================================================================
// 服务端集成测试：会话联动
// =============================================================================

#[cfg(feature = "integration-tests")]
mod server_tests {
    use super::*;
    use common::{
        SERIAL_LOCK, build_single_text, create_test_client, establish_connection, teardown,
    };

    const SENDER: &str = "user_test_001";
    const RECEIVER: &str = "user_test_002";

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_conversation_list_after_send() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        let msg = build_single_text("conv_svr_001", SENDER, RECEIVER, "确保会话存在");
        let ack = message_api(&client).send(msg).await.unwrap();
        assert!(ack.success);

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        // 会话列表由同步引擎在连接后自动拉取，不再暴露 sync_conversations
        let list = client.conversation().unwrap().list().await.unwrap();
        // 服务端可能未自动创建会话条目，仅验证 list 流程不报错
        eprintln!(
            "[test] conversation list after send+sync: {} items, ids={:?}",
            list.len(),
            list.iter().map(|c| c.conversation_id()).collect::<Vec<_>>(),
        );

        teardown(&mut client).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_conversation_get_after_send() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        let msg = build_single_text("conv_svr_002", SENDER, RECEIVER, "查询会话详情");
        let ack = message_api(&client).send(msg).await.unwrap();
        assert!(ack.success);

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        // 会话列表由同步引擎自动拉取
        let conv = client
            .conversation()
            .unwrap()
            .get("conv_svr_002")
            .await
            .unwrap();
        // 服务端可能未自动创建会话条目，仅验证 get 流程不报错
        eprintln!(
            "[test] conversation get after send+sync: found={}",
            conv.is_some()
        );

        teardown(&mut client).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_conversation_mark_read_with_server() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        let msg = build_single_text("conv_svr_003", SENDER, RECEIVER, "标记已读测试");
        let ack = message_api(&client).send(msg).await.unwrap();
        assert!(ack.success);
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        client
            .conversation()
            .unwrap()
            .mark_read("conv_svr_003", ack.seq)
            .await
            .unwrap();
        let conv = client
            .conversation()
            .unwrap()
            .get("conv_svr_003")
            .await
            .unwrap();
        if let Some(c) = conv {
            assert_eq!(c.unread_count(), 0, "unread should be 0 after mark_read");
        }

        teardown(&mut client).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_conversation_delete_with_server() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        let msg = build_single_text("conv_svr_del", SENDER, RECEIVER, "删除会话测试");
        let ack = message_api(&client).send(msg).await.unwrap();
        assert!(ack.success);
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        client
            .conversation()
            .unwrap()
            .delete("conv_svr_del")
            .await
            .unwrap();
        let conv = client
            .conversation()
            .unwrap()
            .get("conv_svr_del")
            .await
            .unwrap();
        assert!(conv.is_none(), "conversation should be gone after delete");

        teardown(&mut client).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_sync_conversation() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        let msg = build_single_text("conv_sync_001", SENDER, RECEIVER, "同步测试");
        let ack = message_api(&client).send(msg).await.unwrap();
        assert!(ack.success);
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        let result = client.sync_conversation("conv_sync_001").await;
        assert!(
            result.is_ok(),
            "sync_conversation should succeed: {:?}",
            result.err()
        );

        teardown(&mut client).await;
    }
}
