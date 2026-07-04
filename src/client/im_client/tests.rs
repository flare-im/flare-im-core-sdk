use std::sync::Arc;
use std::time::Duration;

use super::{
    HeartbeatAppState, IMClient, NetworkChangeEvent, SdkConfigOverlay, SdkState,
    reconnect_delay_bounds_secs, reconnect_delay_secs, should_skip_reconnect_for_disconnect_reason,
};
use crate::infrastructure::persistence::in_memory_empty_im_provider;
use crate::infrastructure::transport::http::HttpRequestContext;
use crate::shared::error::ErrorCode;
use crate::shared::util::CoreTokenConfig;

#[test]
fn reconnect_delay_uses_capped_exponential_backoff_with_jitter_window() {
    assert_eq!(reconnect_delay_bounds_secs(5, 1), (4, 6));
    assert_eq!(reconnect_delay_bounds_secs(5, 2), (8, 12));
    assert_eq!(reconnect_delay_bounds_secs(5, 3), (16, 24));
    assert_eq!(reconnect_delay_bounds_secs(5, 4), (24, 30));
    assert_eq!(reconnect_delay_bounds_secs(5, 10), (24, 30));

    for attempt in 1..=10 {
        let (min, max) = reconnect_delay_bounds_secs(5, attempt);
        let actual = reconnect_delay_secs(5, attempt);
        assert!(
            (min..=max).contains(&actual),
            "attempt {attempt} delay {actual} outside {min}..={max}"
        );
    }
}

#[tokio::test]
async fn heartbeat_app_state_snapshot_updates_without_session() {
    let client = IMClient::new();
    assert!(client.is_app_foreground_snapshot());

    client
        .set_heartbeat_app_state(HeartbeatAppState::Background)
        .await
        .expect("background app state should be accepted before login");
    assert!(!client.is_app_foreground_snapshot());

    client
        .set_heartbeat_app_state(HeartbeatAppState::Foreground)
        .await
        .expect("foreground app state should be accepted before login");
    assert!(client.is_app_foreground_snapshot());
}

#[test]
fn local_client_disconnect_reasons_do_not_schedule_reconnect() {
    assert!(should_skip_reconnect_for_disconnect_reason(
        "Client disconnected"
    ));
    assert!(should_skip_reconnect_for_disconnect_reason(
        "Closed by client"
    ));
    assert!(should_skip_reconnect_for_disconnect_reason(
        " transport: Client disconnected "
    ));
    assert!(should_skip_reconnect_for_disconnect_reason(
        "websocket Closed by client"
    ));
}

#[test]
fn generate_core_token_requires_explicit_signing_config() {
    let err = IMClient::generate_core_token(CoreTokenConfig {
        secret: String::new(),
        issuer: "flare-im-core".to_string(),
        user_id: "alice".to_string(),
        ttl_secs: 3600,
        device_id: None,
        tenant_id: None,
    })
    .expect_err("production build must not mint unsigned or default-signed tokens");

    assert_eq!(
        err.code(),
        Some(crate::shared::error::ErrorCode::ConfigurationError)
    );
}

#[tokio::test]
async fn uninit_clears_init_configuration() {
    let client = IMClient::new();
    let data_root =
        std::env::temp_dir().join(format!("flare-im-uninit-test-{}", std::process::id()));
    client
        .init(
            Some("dev".to_string()),
            Some(SdkConfigOverlay {
                data_url: Some(format!("file://{}", data_root.display())),
                ws_url: Some("ws://localhost:60051".to_string()),
                ..SdkConfigOverlay::default()
            }),
        )
        .await
        .expect("init sdk");

    assert!(client.data_root().await.is_some());
    client.uninit().await.expect("uninit sdk");

    let (environment, sdk_config) = client.config_snapshot().await;
    assert!(environment.is_none());
    assert!(sdk_config.is_none());
    assert!(client.data_root().await.is_none());
    assert!(!client.session_active_sync());
    let _ = tokio::fs::remove_dir_all(data_root).await;
}

#[tokio::test]
async fn session_active_sync_is_false_for_prepared_but_disconnected_user() {
    let client = IMClient::new();
    {
        let mut inner = client.inner.write().await;
        inner.current_user_id = Some("alice".to_string());
    }

    assert!(!client.session_active_sync());
}

#[tokio::test]
async fn network_change_is_noop_without_session() {
    let client = IMClient::new();
    let reconnected = client
        .notify_network_change(NetworkChangeEvent {
            available: true,
            interface: Some("wifi".to_string()),
            ..Default::default()
        })
        .await
        .expect("network change");
    assert!(!reconnected);
}

#[test]
fn network_change_reconnect_is_single_flight() {
    let client = IMClient::new();

    assert!(client.try_begin_network_reconnect());
    assert!(!client.try_begin_network_reconnect());

    client.finish_network_reconnect();
    assert!(client.try_begin_network_reconnect());
    client.finish_network_reconnect();
}

#[test]
fn weak_client_upgrade_fails_after_last_strong_handle_drops() {
    let client = IMClient::new();
    let weak = client.downgrade();

    {
        let upgraded = weak.upgrade();
        assert!(upgraded.is_some());
    }

    drop(client);

    assert!(weak.upgrade().is_none());
}

#[tokio::test]
async fn view_api_refresh_worker_does_not_keep_released_api_alive() {
    for iteration in 0..32 {
        let weak_view = {
            let client = IMClient::builder()
                .stores(in_memory_empty_im_provider())
                .build()
                .expect("build client");
            let view_api = {
                let inner = client.inner.read().await;
                inner.view_api.clone().expect("view api")
            };
            let weak_view = Arc::downgrade(&view_api);
            drop(view_api);
            drop(client);
            weak_view
        };

        for _ in 0..10 {
            if weak_view.upgrade().is_none() {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        assert!(
            weak_view.upgrade().is_none(),
            "ViewApi stayed alive after release on iteration {iteration}"
        );
    }
}

#[tokio::test]
async fn session_active_sync_requires_connected_api_snapshot_and_active_state() {
    let client = IMClient::builder()
        .stores(in_memory_empty_im_provider())
        .build()
        .expect("build client");

    assert!(!client.session_active_sync());

    let apis = {
        let inner = client.inner.read().await;
        IMClient::connected_apis_from_inner(&inner).expect("connected apis")
    };
    client.store_connected_apis_snapshot(apis);
    assert!(!client.session_active_sync());

    client.store_state_snapshot(SdkState::Ready);
    assert!(client.session_active_sync());

    client.clear_session_snapshot();
    assert!(!client.session_active_sync());
}

#[tokio::test]
async fn api_getters_use_session_snapshot_when_inner_lock_is_busy() {
    let client = IMClient::builder()
        .stores(in_memory_empty_im_provider())
        .build()
        .expect("build client");
    let apis = {
        let inner = client.inner.read().await;
        IMClient::connected_apis_from_inner(&inner).expect("connected apis")
    };
    client.store_connected_apis_snapshot(apis);
    client.store_state_snapshot(SdkState::Ready);

    let _locked = client.inner.write().await;

    client.message().expect("message api from snapshot");
    client
        .message_build()
        .expect("message builder from snapshot");
    client
        .conversation()
        .expect("conversation api from snapshot");
    client.media().expect("media api from snapshot");
    client.capability().expect("capability api from snapshot");
    client.presence().expect("presence api from snapshot");
    client
        .capability_registry()
        .expect("capability registry from snapshot");
}

#[tokio::test]
async fn update_access_token_replaces_existing_gateway_bearer() {
    let context = Arc::new(HttpRequestContext::new());
    context
        .set_gateway_context(
            "old-gateway-token".to_string(),
            "tenant-a".to_string(),
            "alice".to_string(),
            None,
        )
        .await;
    let client = IMClient::new();
    {
        let mut inner = client.inner.write().await;
        inner.current_user_id = Some("alice".to_string());
        inner.connect_token = Some("old-im-token".to_string());
        inner.http_request_context = Some(context.clone());
    }
    client.store_state_snapshot(SdkState::Ready);

    client
        .update_access_token("new-gateway-token", Some("tenant-b"))
        .await
        .expect("update token");

    let headers = context.build_headers().await;
    assert_eq!(
        headers.get("Authorization").map(String::as_str),
        Some("Bearer new-gateway-token")
    );
    assert_eq!(
        headers.get("x-tenant-id").map(String::as_str),
        Some("tenant-b")
    );
    assert_eq!(headers.get("x-user-id").map(String::as_str), Some("alice"));
}

#[tokio::test]
async fn update_access_token_rejects_prepared_but_disconnected_session() {
    let context = Arc::new(HttpRequestContext::new());
    context
        .set_gateway_context(
            "old-gateway-token".to_string(),
            "tenant-a".to_string(),
            "alice".to_string(),
            None,
        )
        .await;
    let client = IMClient::new();
    {
        let mut inner = client.inner.write().await;
        inner.current_user_id = Some("alice".to_string());
        inner.connect_token = Some("old-im-token".to_string());
        inner.http_request_context = Some(context.clone());
    }

    let err = client
        .update_access_token("new-gateway-token", Some("tenant-b"))
        .await
        .expect_err("prepared but disconnected session must not refresh gateway auth");
    assert_eq!(err.code(), Some(ErrorCode::NotConnected));

    let headers = context.build_headers().await;
    assert_eq!(
        headers.get("Authorization").map(String::as_str),
        Some("Bearer old-gateway-token")
    );
    assert_eq!(
        headers.get("x-tenant-id").map(String::as_str),
        Some("tenant-a")
    );
}

#[tokio::test]
async fn disconnect_clears_shared_http_auth_context() {
    let context = Arc::new(HttpRequestContext::new());
    context.set_auth_context("im-token".to_string(), None).await;
    context
        .set_gateway_context(
            "gateway-token".to_string(),
            "tenant-a".to_string(),
            "alice".to_string(),
            None,
        )
        .await;
    let client = IMClient::new();
    {
        let mut inner = client.inner.write().await;
        inner.current_user_id = Some("alice".to_string());
        inner.connect_token = Some("im-token".to_string());
        inner.http_request_context = Some(context.clone());
    }

    client.disconnect().await.expect("disconnect");

    let headers = context.build_headers().await;
    assert_eq!(headers.get("Authorization"), None);
    assert_eq!(headers.get("x-user-id"), None);
    assert_eq!(headers.get("x-tenant-id").map(String::as_str), Some("0"));
}
