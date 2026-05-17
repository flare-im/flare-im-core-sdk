//! 同步模块测试：以 Listener 方式注册同步各状态回调并打日志。
//!
//! - 单元测试：用 on_sync_* 类型化回调注册，发布事件后由回调打印日志
//! - 集成测试（需服务端）：连接前注册同步 Listener，连接后各阶段回调打印日志
//!
//! 运行：
//!   cargo test --test sync_test
//!   cargo test --test sync_test --features integration-tests -- --ignored

mod common;

use flare_im_core_sdk::event::{SdkEvent, SyncNotify, SyncPhase};
use flare_im_core_sdk::prelude::*;

fn test_sync_run() -> SyncRunContext {
    SyncRunContext::initial_login()
}

// =============================================================================
// 单元测试：Listener 方式订阅同步事件并打日志
// =============================================================================

#[tokio::test]
async fn test_sync_logging() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    // 在 spawn_blocking 中注册回调并发布事件，避免阻塞 tokio 运行时
    tokio::task::spawn_blocking(|| {
        let bus = EventBus::new();

        // 使用 Listener 方式注册同步各状态回调
        let _ = bus.on_sync_state_changed(|state| {
            tracing::info!(state = ?state, "[sync] SyncStateChanged");
        });
        let _ = bus.on_sync_started(|| {
            tracing::info!("[sync] SyncStarted");
        });
        let _ = bus.on_sync_finished(|phase| {
            tracing::info!(phase = ?phase, "[sync] SyncFinished");
        });
        let _ = bus.on_sync_failed(|task, message| {
            tracing::warn!(task = %task, message = %message, "[sync] SyncFailed");
        });
        let _ = bus.on_sync_progress(|task, progress, detail| {
            tracing::info!(task = %task, progress = %progress, detail = %detail, "[sync] SyncProgress");
        });
        let _ = bus.on_sync_task_completed(|task| {
            tracing::info!(task = %task, "[sync] SyncTaskCompleted");
        });

        let run = test_sync_run();

        bus.publish(SdkEvent::Sync(SyncNotify::Started { run: run.clone() }));
        bus.publish(SdkEvent::Sync(SyncNotify::Progress {
            run: run.clone(),
            task: "conversations".into(),
            progress: 0.1,
            detail: "0 / 35".into(),
        }));
        bus.publish(SdkEvent::Sync(SyncNotify::TaskCompleted {
            run: run.clone(),
            task: "conversations".into(),
        }));
        bus.publish(SdkEvent::Sync(SyncNotify::Finished {
            run: run.clone(),
            phase: SyncPhase::Init,
        }));
        bus.publish(SdkEvent::Sync(SyncNotify::Progress {
            run: run.clone(),
            task: "read_states".into(),
            progress: 1.0,
            detail: "35 / 35".into(),
        }));
        bus.publish(SdkEvent::Sync(SyncNotify::TaskCompleted {
            run: run.clone(),
            task: "read_states".into(),
        }));
        bus.publish(SdkEvent::Sync(SyncNotify::Finished {
            run,
            phase: SyncPhase::Background,
        }));

        std::thread::sleep(std::time::Duration::from_millis(100));
    })
    .await
    .expect("spawn_blocking");
}

#[tokio::test]
async fn test_sync_order() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    let run = test_sync_run();

    bus.publish(SdkEvent::Sync(SyncNotify::Started { run: run.clone() }));
    bus.publish(SdkEvent::Sync(SyncNotify::TaskCompleted {
        run: run.clone(),
        task: "conversations".into(),
    }));
    bus.publish(SdkEvent::Sync(SyncNotify::Finished {
        run: run.clone(),
        phase: SyncPhase::Init,
    }));
    bus.publish(SdkEvent::Sync(SyncNotify::Finished {
        run,
        phase: SyncPhase::Background,
    }));

    let e1 = tokio::time::timeout(tokio::time::Duration::from_millis(200), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(e1, SdkEvent::Sync(SyncNotify::Started { .. })));

    let e2 = tokio::time::timeout(tokio::time::Duration::from_millis(200), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        e2,
        SdkEvent::Sync(SyncNotify::TaskCompleted { .. })
    ));

    let e3 = tokio::time::timeout(tokio::time::Duration::from_millis(200), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        e3,
        SdkEvent::Sync(SyncNotify::Finished {
            phase: SyncPhase::Init,
            ..
        })
    ));

    let e4 = tokio::time::timeout(tokio::time::Duration::from_millis(200), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        e4,
        SdkEvent::Sync(SyncNotify::Finished {
            phase: SyncPhase::Background,
            ..
        })
    ));
}

// =============================================================================
// 集成测试：Listener 方式注册会话/同步各状态回调，连接后打日志（需服务端）
// =============================================================================

#[cfg(feature = "integration-tests")]
#[tokio::test]
#[ignore]
async fn test_sync_phase_callbacks_with_server() {
    use std::sync::Arc;

    let _ = tracing_subscriber::fmt()
        .with_env_filter("info,flare_im_core_sdk=debug")
        .try_init();

    // builder 已内置注入会话/消息/已读三任务，直接创建 client
    let mut client = common::create_test_client_no_connect().await;

    let done = Arc::new(tokio::sync::Notify::new());
    let done_clone = done.clone();

    let bus = client.bus().clone();

    // 使用 Listener 方式注册各状态回调（blocking_write 在 spawn_blocking 中执行）
    tokio::task::spawn_blocking(move || {
        let _ = bus.on_state_changed(move |state| {
            tracing::info!(state = ?state, "[sync_test] StateChanged");
        });
        let _ = bus.on_sync_state_changed(|state| {
            tracing::info!(state = ?state, "[sync_test] SyncStateChanged");
        });
        let _ = bus.on_sync_started(|| {
            tracing::info!("[sync_test] SyncStarted");
        });
        let _ = bus.on_sync_finished(move |phase| {
            tracing::info!(phase = ?phase, "[sync_test] SyncFinished");
            if matches!(phase, SyncPhase::Background) {
                done_clone.notify_one();
            }
        });
        let _ = bus.on_sync_failed(|task, message| {
            tracing::warn!(task = %task, message = %message, "[sync_test] SyncFailed");
        });
        let _ = bus.on_sync_progress(|task, progress, detail| {
            tracing::info!(
                task = %task,
                progress = %progress,
                detail = %detail,
                "[sync_test] SyncProgress"
            );
        });
        let _ = bus.on_sync_task_completed(|task| {
            tracing::info!(task = %task, "[sync_test] SyncTaskCompleted");
        });
        let _ = bus.on_conversation_synced(|ids| {
            tracing::info!(ids = ?ids, "[sync_test] Conversation synced");
        });
    })
    .await
    .expect("spawn_blocking");

    common::establish_connection(&mut client, "sync_test_user").await;
    let _ = tokio::time::timeout(tokio::time::Duration::from_secs(30), done.notified()).await;
    common::teardown(&mut client).await;
}
