use std::sync::Arc;
use std::time::Duration;

use super::session_watchers::recovered_connection_state;
use super::{
    HeartbeatAppState, IMClient, NetworkChangeEvent, SdkConfigOverlay, SdkState, reconnect_delay,
    reconnect_delay_bounds_secs, should_skip_reconnect_for_disconnect_reason,
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
        let actual = reconnect_delay(5, attempt);
        assert!(
            (Duration::from_secs(min)..=Duration::from_secs(max)).contains(&actual),
            "attempt {attempt} delay {actual:?} outside {min}..={max}s"
        );
    }
}

/// 抖动按毫秒取值：同一窗口里的取值铺开，而不是只落在几个整秒上（以前 5 s 间隔的第一次重连只有
/// 4、5、6 s 三个取值）。暂时性拒绝（SERVICE_UNAVAILABLE）、服务端重启后的重连都走这条退避。
#[test]
fn reconnect_jitter_is_spread_at_millisecond_granularity() {
    let distinct: std::collections::HashSet<Duration> =
        (0..200).map(|_| reconnect_delay(5, 1)).collect();
    assert!(
        distinct.len() > 50,
        "only {} distinct delays in 200 draws",
        distinct.len()
    );
    assert!(
        distinct
            .iter()
            .all(|delay| (Duration::from_secs(4)..=Duration::from_secs(6)).contains(delay))
    );
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

/// 租户不可用的断开不排重连：网关只会再拒一次，由 KickedOff 交给应用。
#[test]
fn tenant_unavailable_disconnect_does_not_schedule_reconnect() {
    assert!(should_skip_reconnect_for_disconnect_reason(
        "TENANT_UNAVAILABLE"
    ));
}

/// 心跳判死是 flare-core 替我们断的，不是「我方主动断开」：必须重连，否则界面一直停在「连接已断开」。
#[test]
fn heartbeat_verdict_disconnects_schedule_reconnect() {
    use flare_core::client::heartbeat::manager::{
        HEARTBEAT_SEND_FAILED_REASON, HEARTBEAT_TIMEOUT_REASON,
    };
    assert!(!should_skip_reconnect_for_disconnect_reason(
        HEARTBEAT_TIMEOUT_REASON
    ));
    assert!(!should_skip_reconnect_for_disconnect_reason(
        HEARTBEAT_SEND_FAILED_REASON
    ));
}

/// 连接以错误终止时 SocketHandler 报的断开：重连监视器要接手（flare-core 客户端不再自己重连）。
#[test]
fn transport_error_disconnects_schedule_reconnect() {
    use crate::infrastructure::transport::socket::socket_handler::TRANSPORT_ERROR_REASON_PREFIX;
    for error in [
        "FlareError { code: ConnectionFailed, message: \"WebSocket protocol error: Encountered invalid opcode: 3\" }",
        "FlareError { code: ProtocolError, message: \"QUIC connection closed by peer with a protocol error: PROTOCOL_VIOLATION\" }",
        "connection lost: timed out",
    ] {
        let reason = format!("{TRANSPORT_ERROR_REASON_PREFIX}{error}");
        assert!(
            !should_skip_reconnect_for_disconnect_reason(&reason),
            "{reason}"
        );
    }
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
async fn update_access_token_refreshes_core_bearer_without_overwriting_bff_bearer() {
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
    context
        .set_auth_context("old-im-token".to_string(), None)
        .await;
    {
        let mut inner = client.inner.write().await;
        inner.current_user_id = Some("alice".to_string());
        inner.connect_token = Some("old-im-token".to_string());
        inner.http_request_context = Some(context.clone());
    }
    client.store_state_snapshot(SdkState::Ready);

    client
        .update_access_token("new-im-token", Some("tenant-a"))
        .await
        .expect("update token");

    let headers = context.build_headers().await;
    assert_eq!(
        headers.get("Authorization").map(String::as_str),
        Some("Bearer old-gateway-token")
    );
    assert_eq!(
        headers.get("x-tenant-id").map(String::as_str),
        Some("tenant-a")
    );
    assert_eq!(headers.get("x-user-id").map(String::as_str), Some("alice"));
    let core_headers = context.build_core_headers().await;
    assert_eq!(
        core_headers.get("Authorization").map(String::as_str),
        Some("Bearer new-im-token")
    );
    assert_eq!(client.access_token().await.as_deref(), Some("new-im-token"));
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
    context
        .set_auth_context("old-im-token".to_string(), None)
        .await;
    {
        let mut inner = client.inner.write().await;
        inner.current_user_id = Some("alice".to_string());
        inner.connect_token = Some("old-im-token".to_string());
        inner.http_request_context = Some(context.clone());
    }

    let err = client
        .update_access_token("new-gateway-token", Some("tenant-b"))
        .await
        .expect_err("prepared but disconnected session must not refresh HTTP auth");
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
    assert_eq!(
        context
            .build_core_headers()
            .await
            .get("Authorization")
            .map(String::as_str),
        Some("Bearer old-im-token")
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

/// 经 builder 配的重连间隔与次数要到 SDK 自己的重连循环，不只到 flare-core 客户端；
/// `init()` 的覆盖项仍然优先，没覆盖的那一项照旧取 builder 的配置。
#[tokio::test]
async fn reconnect_backoff_comes_from_the_builder_config_unless_overridden() {
    use crate::client::config::SdkConfig;
    let mut config = SdkConfig::new("ws://127.0.0.1:9");
    config.reconnect_interval_secs = Some(2);
    config.max_reconnect_attempts = Some(4);
    let client = IMClient::builder()
        .config(config)
        .stores(in_memory_empty_im_provider())
        .build()
        .expect("build client");
    {
        let mut inner = client.inner.write().await;
        inner.current_user_id = Some("alice".to_string());
        inner.connect_token = Some("token".to_string());
    }
    let generation = client.session_generation_snapshot();
    let (_, _, interval_secs, max_attempts) = client
        .reconnect_settings(generation)
        .await
        .expect("reconnect settings");
    assert_eq!((interval_secs, max_attempts), (2, Some(4)));

    client.inner.write().await.sdk_config = Some(SdkConfigOverlay {
        reconnect_interval_secs: Some(1),
        ..Default::default()
    });
    let (_, _, interval_secs, max_attempts) = client
        .reconnect_settings(generation)
        .await
        .expect("reconnect settings");
    assert_eq!((interval_secs, max_attempts), (1, Some(4)));
}

/// 登录路径（`prepare` 按快照重建会话）也带着 builder 的重连配置。
#[tokio::test]
async fn reconnect_backoff_survives_the_login_rebuild() {
    use crate::client::config::SdkConfig;
    use crate::client::lifecycle::LoginDbKind;
    let mut config = SdkConfig::new("ws://127.0.0.1:9");
    config.reconnect_interval_secs = Some(3);
    config.max_reconnect_attempts = Some(2);
    let client = IMClient::builder()
        .config(config)
        .stores(in_memory_empty_im_provider())
        .build()
        .expect("build client");
    client
        .prepare(
            "alice",
            LoginDbKind::IndexedDb(in_memory_empty_im_provider()),
        )
        .await
        .expect("prepare");
    client.inner.write().await.connect_token = Some("token".to_string());

    let generation = client.session_generation_snapshot();
    let (_, _, interval_secs, max_attempts) = client
        .reconnect_settings(generation)
        .await
        .expect("reconnect settings");
    assert_eq!((interval_secs, max_attempts), (3, Some(2)));
    client.logout().await.expect("logout");
}

#[test]
fn reconnect_backoff_defaults_and_floors() {
    use super::session_watchers::reconnect_backoff_settings;
    use crate::client::config::SdkConfig;
    assert_eq!(reconnect_backoff_settings(None, None), (5, None));

    let mut zeroed = SdkConfig::new("ws://127.0.0.1:9");
    zeroed.reconnect_interval_secs = Some(0);
    zeroed.max_reconnect_attempts = Some(0);
    assert_eq!(
        reconnect_backoff_settings(None, Some(&zeroed)),
        (1, Some(1))
    );

    let mut unset = SdkConfig::new("ws://127.0.0.1:9");
    unset.reconnect_interval_secs = None;
    assert_eq!(reconnect_backoff_settings(None, Some(&unset)), (5, None));
}

/// 把当前线程的 tracing 输出收进内存，供断言「会话结束后没有再记重连成功」。
#[derive(Clone, Default)]
struct CapturedLogs(Arc<std::sync::Mutex<Vec<u8>>>);

struct CapturedLogsWriter(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for CapturedLogsWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log buffer").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLogs {
    type Writer = CapturedLogsWriter;

    fn make_writer(&'a self) -> Self::Writer {
        CapturedLogsWriter(self.0.clone())
    }
}

impl CapturedLogs {
    /// 只在当前线程生效：`#[tokio::test]` 的单线程运行时里，SDK 后台任务也在这条线程上跑。
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_writer(self.clone())
                .with_max_level(tracing::Level::DEBUG)
                .with_ansi(false)
                .finish(),
        )
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("log buffer")).into_owned()
    }
}

#[derive(Clone, Copy, Debug)]
enum SessionEnd {
    Logout,
    Disconnect,
    DropClient,
}

/// 重连监视器在退避中时会话结束：监视器应立刻收手，退避到点后不再记「SDK reconnect
/// succeeded」，也不再发起重连。此前它睡满退避，醒来后 `reconnect_current_engine` 因世代
/// 已变直接返回 Ok，于是在已结束的会话上记一条重连成功。
async fn reconnect_monitor_stops_when_the_session_ends(end: SessionEnd) {
    use crate::client::config::SdkConfig;
    use crate::kernel::event::{ConnectionEvent, SdkEvent, SdkEventKind};

    let logs = CapturedLogs::default();
    let _logs = logs.install();

    // 两个地址都指向本机不监听的端口：本用例不会真去连，只要不碰外网。
    let mut config = SdkConfig::new("ws://127.0.0.1:9");
    config.http_url = Some("http://127.0.0.1:9".to_string());
    config.reconnect_interval_secs = Some(1);
    let client = IMClient::builder()
        .config(config)
        .stores(in_memory_empty_im_provider())
        .build()
        .expect("build client");
    let bus = {
        let mut inner = client.inner.write().await;
        inner.current_user_id = Some("alice".to_string());
        inner.connect_token = Some("token".to_string());
        inner.engine.as_ref().expect("engine").bus().clone()
    };
    let generation = client.session_generation_snapshot();
    let mut connection_events = bus.subscribe_filter(SdkEventKind::Connection);
    client.spawn_reconnect_session_watcher(generation, bus.clone());

    bus.publish(SdkEvent::Connection(ConnectionEvent::Disconnected {
        reason: "network lost".to_string(),
    }));
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(SdkEvent::Connection(ConnectionEvent::Reconnecting { attempt: 1 })) =
                connection_events.recv().await
            {
                break;
            }
        }
    })
    .await
    .expect("the monitor announces its first attempt and starts the backoff");

    // 第一次退避是 1–2 s；在退避中结束会话。
    match end {
        SessionEnd::Logout => client.logout().await.expect("logout"),
        SessionEnd::Disconnect => client.disconnect().await.expect("disconnect"),
        SessionEnd::DropClient => drop(client),
    }
    let ended_at = std::time::Instant::now();
    while !logs.text().contains("SDK reconnect monitor stopped") {
        assert!(
            ended_at.elapsed() < Duration::from_millis(500),
            "{end:?}: the reconnect monitor must stop as soon as the session ends\n{}",
            logs.text()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // 等过退避上限：不能再出现重连结果，也不能有第二次尝试。
    tokio::time::sleep(Duration::from_millis(2_500)).await;
    let text = logs.text();
    assert!(
        !text.contains("SDK reconnect succeeded"),
        "{end:?}: a reconnect must not be reported after the session ended\n{text}"
    );
    assert!(
        !text.contains("SDK reconnect failed"),
        "{end:?}: no reconnect may run after the session ended\n{text}"
    );
    while let Ok(event) = connection_events.try_recv() {
        assert!(
            !matches!(
                event,
                SdkEvent::Connection(ConnectionEvent::Reconnecting { .. })
            ),
            "{end:?}: no further reconnect attempt after the session ended"
        );
    }
}

#[tokio::test]
async fn reconnect_monitor_stops_on_logout() {
    reconnect_monitor_stops_when_the_session_ends(SessionEnd::Logout).await;
}

#[tokio::test]
async fn reconnect_monitor_stops_on_disconnect() {
    reconnect_monitor_stops_when_the_session_ends(SessionEnd::Disconnect).await;
}

#[tokio::test]
async fn reconnect_monitor_stops_when_the_client_is_destroyed() {
    reconnect_monitor_stops_when_the_session_ends(SessionEnd::DropClient).await;
}

/// 会话结束（登出、断开、客户端销毁）时，状态快照、终局事件、令牌刷新三个监视器都立刻收手。
/// 此前令牌刷新监视器要睡到下一次刷新（这里约 55 分钟后）才发现会话已结束，状态快照与终局
/// 事件监视器要等到下一条连接事件。
async fn session_watchers_stop_when_the_session_ends(end: SessionEnd) {
    use crate::client::config::{SdkAuthConfig, SdkConfig};
    use crate::shared::util::generate_core_token;

    let logs = CapturedLogs::default();
    let _logs = logs.install();

    // 两个地址都指向本机不监听的端口：本用例不会真去连，只要不碰外网。
    let mut config = SdkConfig::new("ws://127.0.0.1:9");
    config.http_url = Some("http://127.0.0.1:9".to_string());
    let client = IMClient::builder()
        .config(config)
        .stores(in_memory_empty_im_provider())
        .build()
        .expect("build client");
    // SDK 托管令牌，令牌一小时后到期：刷新监视器会睡到到期前 5 分钟。
    let token = generate_core_token(&CoreTokenConfig {
        secret: "a-strong-shared-secret-with-more-than-32-bytes!".into(),
        issuer: "flare-im-core".into(),
        user_id: "alice".into(),
        ttl_secs: 3600,
        device_id: None,
        tenant_id: None,
    })
    .expect("token");
    let bus = {
        let mut inner = client.inner.write().await;
        inner.current_user_id = Some("alice".to_string());
        inner.connect_token = Some(token);
        inner.sdk_config = Some(SdkConfigOverlay {
            auth: Some(SdkAuthConfig {
                token_endpoint: Some("http://127.0.0.1:9".to_string()),
                refresh_lead_secs: None,
            }),
            ..SdkConfigOverlay::default()
        });
        inner.engine.as_ref().expect("engine").bus().clone()
    };
    let generation = client.session_generation_snapshot();
    client.spawn_state_snapshot_watcher(generation, bus.clone());
    client.spawn_terminal_session_watcher(generation, bus.clone());
    client.spawn_token_refresh_watcher(generation);
    // 让三个监视器都进入等待
    tokio::time::sleep(Duration::from_millis(50)).await;

    match end {
        SessionEnd::Logout => client.logout().await.expect("logout"),
        SessionEnd::Disconnect => client.disconnect().await.expect("disconnect"),
        SessionEnd::DropClient => drop(client),
    }
    let ended_at = std::time::Instant::now();
    for watcher in [
        "SDK state snapshot watcher stopped",
        "SDK terminal session watcher stopped",
        "SDK token refresh watcher stopped",
    ] {
        while !logs.text().contains(watcher) {
            assert!(
                ended_at.elapsed() < Duration::from_millis(500),
                "{end:?}: {watcher:?} must happen as soon as the session ends\n{}",
                logs.text()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    // 测试持有总线，它不会因客户端销毁而关闭
    drop(bus);
}

#[tokio::test]
async fn session_watchers_stop_on_logout() {
    session_watchers_stop_when_the_session_ends(SessionEnd::Logout).await;
}

#[tokio::test]
async fn session_watchers_stop_on_disconnect() {
    session_watchers_stop_when_the_session_ends(SessionEnd::Disconnect).await;
}

#[tokio::test]
async fn session_watchers_stop_when_the_client_is_destroyed() {
    session_watchers_stop_when_the_session_ends(SessionEnd::DropClient).await;
}

/// 会话在重连开始前已结束：`reconnect_current_engine` 报明确的错误，而不是 Ok
/// （此前调用方会把它当成重连成功）。
#[tokio::test]
async fn reconnect_after_the_session_ended_is_an_error() {
    use crate::client::config::SdkConfig;

    let client = IMClient::builder()
        .config(SdkConfig::new("ws://127.0.0.1:9"))
        .stores(in_memory_empty_im_provider())
        .build()
        .expect("build client");
    let stale_generation = client.session_generation_snapshot();
    {
        let mut inner = client.inner.write().await;
        inner.session_generation = inner.session_generation.wrapping_add(1);
        client.store_session_generation_snapshot(inner.session_generation);
    }

    let err = client
        .reconnect_current_engine(stale_generation, "alice", "token")
        .await
        .expect_err("a reconnect for an ended session must not report success");
    assert_eq!(err.code(), Some(ErrorCode::NotConnected), "{err}");
    assert!(err.to_string().contains("session ended"), "{err}");
}

/// 断开原因是服务端停机：重连监视器的第一次尝试只等 1–5 s 的随机一段，不按配置的 30 s 退避
/// （按退避要 24–30 s）。地址指向本机不监听的端口，只看重连何时真正开始（重建传输的日志带
/// `trigger=Reconnect`）。
#[tokio::test]
async fn reconnect_monitor_pauses_briefly_after_a_server_shutdown() {
    use crate::client::config::SdkConfig;
    use crate::kernel::event::{ConnectionEvent, SdkEvent};

    let logs = CapturedLogs::default();
    let _logs = logs.install();

    let mut config = SdkConfig::new("ws://127.0.0.1:9");
    config.http_url = Some("http://127.0.0.1:9".to_string());
    config.reconnect_interval_secs = Some(30);
    let client = IMClient::builder()
        .config(config)
        .stores(in_memory_empty_im_provider())
        .build()
        .expect("build client");
    let bus = {
        let mut inner = client.inner.write().await;
        inner.current_user_id = Some("alice".to_string());
        inner.connect_token = Some("token".to_string());
        inner.engine.as_ref().expect("engine").bus().clone()
    };
    let generation = client.session_generation_snapshot();
    client.spawn_reconnect_session_watcher(generation, bus.clone());

    let disconnected_at = std::time::Instant::now();
    bus.publish(SdkEvent::Connection(ConnectionEvent::Disconnected {
        reason: "SERVER_SHUTDOWN: server closed the connection".to_string(),
    }));
    while !logs.text().contains("trigger=Reconnect") {
        assert!(
            disconnected_at.elapsed() < Duration::from_secs(8),
            "the first reconnect after a shutdown must not wait for the 30 s backoff\n{}",
            logs.text()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        disconnected_at.elapsed() >= flare_core::common::shutdown::SERVER_SHUTDOWN_RECONNECT_MIN,
        "the reconnect after a shutdown still pauses first"
    );
    assert!(logs.text().contains("reconnecting after a jittered pause"));
    client.disconnect().await.expect("disconnect");
}
