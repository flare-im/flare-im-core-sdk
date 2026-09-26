use std::sync::Arc;
use std::time::Duration;

use super::session_watchers::recovered_connection_state;
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

#[test]
fn stale_reconnect_only_restores_a_stable_connected_engine_state() {
    assert_eq!(
        recovered_connection_state(SdkState::Ready, true),
        Some(SdkState::Ready)
    );
    assert_eq!(
        recovered_connection_state(SdkState::Connected, true),
        Some(SdkState::Connected)
    );
    assert_eq!(
        recovered_connection_state(SdkState::Reconnecting, true),
        None,
        "an in-flight reconnect must publish its own completion"
    );
    assert_eq!(
        recovered_connection_state(SdkState::Ready, false),
        None,
        "an unavailable transport must never clear the reconnect notice"
    );
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
async fn session_active_sync_is_independent_of_transport_state() {
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
    assert!(client.session_active_sync());

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

/// 契约：设备标识必须由**同一处**决定，供登录签 token 与 CONNECT 上报共用。
/// 网关在两侧都非空且不等时直接拒连，此前两处各自取值无对齐机制，只能给登录传空绕过。
#[tokio::test]
async fn bind_device_id_writes_then_reads_back_the_same_value() {
    let client = IMClient::new();

    // 未配置：只读返回 None（连接侧届时回退到进程级临时标识）
    assert_eq!(client.bind_device_id(None).await, None);

    // 写入后回显，且后续只读拿到同一个值 —— 登录与连接因此同源
    assert_eq!(
        client
            .bind_device_id(Some("device-alpha".to_string()))
            .await,
        Some("device-alpha".to_string())
    );
    assert_eq!(
        client.bind_device_id(None).await,
        Some("device-alpha".to_string()),
        "a later read must not invent a different device id"
    );

    // 空白输入按未提供处理：不得清空已绑定值
    assert_eq!(
        client.bind_device_id(Some("   ".to_string())).await,
        Some("device-alpha".to_string()),
        "blank input is a read, not a reset"
    );
}

/// `init` 已配置 device_id 时，只读路径要能取到它（社交登录据此复用同一个值）。
#[tokio::test]
async fn bind_device_id_reads_value_configured_at_init() {
    let client = IMClient::new();
    client
        .init(
            None,
            Some(SdkConfigOverlay {
                device_id: Some("device-from-init".to_string()),
                ..Default::default()
            }),
        )
        .await
        .expect("init must succeed");

    assert_eq!(
        client.bind_device_id(None).await,
        Some("device-from-init".to_string())
    );
}

#[tokio::test]
async fn prepared_session_searches_offline_and_invalidates_old_user() {
    use crate::client::lifecycle::LoginDbKind;
    let client = IMClient::new();
    let root = std::env::temp_dir().join(format!("flare-offline-session-{}", std::process::id()));
    client
        .init(
            None,
            Some(SdkConfigOverlay {
                data_url: Some(format!("file://{}", root.display())),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    client
        .prepare(
            "alice",
            LoginDbKind::IndexedDb(in_memory_empty_im_provider()),
        )
        .await
        .unwrap();
    let alice_generation = client.session_generation_snapshot();
    assert_eq!(client.state(), SdkState::Disconnected);
    assert!(client.session_active_sync());
    let alice_api = client.message().unwrap();
    assert!(
        alice_api
            .search_in_conversation("c1", "hello", 10)
            .await
            .unwrap()
            .is_empty()
    );
    // A failed connection must not revoke local API access.
    client.clear_session_snapshot();
    assert!(client.connected_apis().await.is_ok());
    client
        .prepare("bob", LoginDbKind::IndexedDb(in_memory_empty_im_provider()))
        .await
        .unwrap();
    assert!(client.session_generation_snapshot() > alice_generation);
    assert_eq!(client.current_user_id().await.as_deref(), Some("bob"));
    assert!(alice_api.search("hello", 10).await.is_err());
    let bob_api = client.message().unwrap();
    client.logout().await.unwrap();
    assert!(!client.session_active_sync());
    assert!(client.message().is_err());
    assert!(bob_api.search("hello", 10).await.is_err());
}

/// 记下被调了几次的宿主续期回调。
struct CountingRefresher {
    token: Option<String>,
    calls: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl crate::client::token_provider::ConnectTokenRefresher for CountingRefresher {
    async fn refresh_connect_token(&self) -> Option<String> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.token.clone()
    }
}

fn counting_refresher(token: Option<&str>) -> Arc<CountingRefresher> {
    Arc::new(CountingRefresher {
        token: token.map(str::to_string),
        calls: std::sync::atomic::AtomicUsize::new(0),
    })
}

async fn host_managed_client(generation: u64, refresher: Arc<CountingRefresher>) -> IMClient {
    let client = IMClient::new();
    {
        let mut g = client.inner.write().await;
        g.session_generation = generation;
        g.connect_token = Some("expired".to_string());
        g.connect_token_refresher = Some(refresher);
    }
    client
}

#[tokio::test]
async fn a_rejected_reconnect_takes_a_fresh_token_from_the_host() {
    let refresher = counting_refresher(Some("fresh"));
    let client = host_managed_client(3, refresher.clone()).await;

    assert_eq!(
        client.refresh_connect_token_via_host(3).await.as_deref(),
        Some("fresh")
    );
    assert_eq!(
        client.inner.read().await.connect_token.as_deref(),
        Some("fresh"),
        "the next reconnect attempt must read the new token"
    );
    assert_eq!(refresher.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn the_same_token_handed_back_is_not_a_swap() {
    // 宿主刷新没成功（冷却中、刷新令牌已用过）时原样返回旧令牌：不能当成换到了，
    // 否则重连会从头计退避、对着网关快速空转。
    for unchanged in [Some("expired"), Some("  "), None] {
        let refresher = counting_refresher(unchanged);
        let client = host_managed_client(3, refresher).await;
        assert_eq!(client.refresh_connect_token_via_host(3).await, None);
        assert_eq!(
            client.inner.read().await.connect_token.as_deref(),
            Some("expired")
        );
    }
}

#[tokio::test]
async fn a_superseded_session_does_not_ask_the_host() {
    let refresher = counting_refresher(Some("fresh"));
    let client = host_managed_client(4, refresher.clone()).await;

    assert_eq!(client.refresh_connect_token_via_host(3).await, None);
    assert_eq!(refresher.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(
        client.inner.read().await.connect_token.as_deref(),
        Some("expired")
    );
}

#[tokio::test]
async fn sdk_managed_tokens_are_left_to_the_gateway_issuer() {
    let refresher = counting_refresher(Some("fresh"));
    let client = host_managed_client(3, refresher.clone()).await;
    client.inner.write().await.sdk_config = Some(SdkConfigOverlay {
        auth: Some(crate::client::config::SdkAuthConfig {
            token_endpoint: Some("http://gateway/api".to_string()),
            refresh_lead_secs: None,
        }),
        ..Default::default()
    });

    assert_eq!(client.refresh_connect_token_via_host(3).await, None);
    assert_eq!(refresher.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn the_host_refresher_survives_the_login_rebuild() {
    use crate::client::lifecycle::LoginDbKind;
    let client = IMClient::new();
    let root = std::env::temp_dir().join(format!("flare-refresher-{}", std::process::id()));
    client
        .init(
            None,
            Some(SdkConfigOverlay {
                data_url: Some(format!("file://{}", root.display())),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    client
        .set_connect_token_refresher(Some(counting_refresher(Some("fresh"))))
        .await;
    client
        .prepare(
            "alice",
            LoginDbKind::IndexedDb(in_memory_empty_im_provider()),
        )
        .await
        .unwrap();
    assert!(client.inner.read().await.connect_token_refresher.is_some());
    client.logout().await.unwrap();
}

/// 模拟宿主的另一条推送路径：刷新成功后先把新令牌写进核心，再把它返回。
struct PushingRefresher {
    client: std::sync::Mutex<Option<IMClient>>,
}

#[async_trait::async_trait]
impl crate::client::token_provider::ConnectTokenRefresher for PushingRefresher {
    async fn refresh_connect_token(&self) -> Option<String> {
        let client = self.client.lock().unwrap().clone()?;
        // 没配 HTTP 上下文时同步那一步会报错，但令牌已先写入，正是要模拟的情形。
        let _ = client.update_access_token("fresh", None).await;
        Some("fresh".to_string())
    }
}

#[tokio::test]
async fn a_token_pushed_in_by_the_host_first_still_counts_as_a_swap() {
    let refresher = Arc::new(PushingRefresher {
        client: std::sync::Mutex::new(None),
    });
    let client = IMClient::new();
    {
        let mut g = client.inner.write().await;
        g.session_generation = 3;
        g.connect_token = Some("expired".to_string());
        g.connect_token_refresher = Some(refresher.clone());
    }
    *refresher.client.lock().unwrap() = Some(client.clone());

    assert_eq!(
        client.refresh_connect_token_via_host(3).await.as_deref(),
        Some("fresh")
    );
    assert_eq!(
        client.inner.read().await.connect_token.as_deref(),
        Some("fresh")
    );
}

#[tokio::test]
async fn the_build_time_http_url_survives_the_login_rebuild() {
    use crate::client::config::SdkConfig;
    use crate::client::lifecycle::LoginDbKind;
    let mut config = SdkConfig::new("wss://host.example/im-ws");
    config.http_url = Some("https://host.example/api".to_string());
    let client = IMClient::builder()
        .config(config)
        .stores(in_memory_empty_im_provider())
        .build()
        .unwrap();
    let root = std::env::temp_dir().join(format!("flare-http-url-{}", std::process::id()));
    client
        .init(
            None,
            Some(SdkConfigOverlay {
                data_url: Some(format!("file://{}", root.display())),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    client
        .prepare(
            "alice",
            LoginDbKind::IndexedDb(in_memory_empty_im_provider()),
        )
        .await
        .unwrap();

    let http_url = client
        .inner
        .read()
        .await
        .configured_config
        .as_ref()
        .and_then(|config| config.http_url.clone());
    assert_eq!(http_url.as_deref(), Some("https://host.example/api"));
    client.logout().await.unwrap();
}

#[test]
fn an_overlay_still_overrides_the_build_time_config() {
    use crate::client::config::SdkConfig;
    let mut base = SdkConfig::new("wss://host.example/im-ws");
    base.http_url = Some("https://host.example/api".to_string());
    base.tenant_id = Some("t-base".to_string());

    let merged = crate::client::lifecycle::merge_sdk_config_onto(
        base,
        "wss://other.example/im-ws",
        Some(&SdkConfigOverlay {
            http_url: Some("https://other.example/api".to_string()),
            ..Default::default()
        }),
    );
    assert_eq!(merged.ws_url.as_deref(), Some("wss://other.example/im-ws"));
    assert_eq!(
        merged.http_url.as_deref(),
        Some("https://other.example/api")
    );
    assert_eq!(merged.tenant_id.as_deref(), Some("t-base"));
}
