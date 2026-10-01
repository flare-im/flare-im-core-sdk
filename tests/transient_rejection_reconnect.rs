//! 服务端暂时放不了行时，SDK 退避重连直到放行，而不是终止会话。
//!
//! 网关在严格策略下读不到租户投影（数据库抖动等）时，以 flare-core 的暂时性拒绝
//! （`AuthResult::retryable_failure`）拒绝 CONNECT：关连接前的 ERROR 帧 message 与 metadata
//! `code` 都是 `SERVICE_UNAVAILABLE`。SDK 要把它当成可重试：首次建连报出 `SERVICE_UNAVAILABLE`
//! 由应用重试；会话中的重连循环退避后继续连，直到放行、回到 Ready。只有 `TENANT_UNAVAILABLE`
//! 才终止会话、不再重连。
//!
//! 本机回环（只绑 127.0.0.1）起一个真实的 flare-core WebSocket 服务端，认证器按测试设定
//! 给出暂时性拒绝或租户不可用；SDK 走自己的完整重连循环：断线事件 → 重连监听 → 退避 →
//! 重建传输 → CONNECT → 按 ERROR 帧分类 → 再退避 → 放行。

#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use flare_core::common::config_types::TransportProtocol;
use flare_core::common::device::DeviceInfo;
use flare_core::server::{
    AuthResult, Authenticator, FlareServer, FlareServerBuilder, ServerEventHandler,
};
use flare_im_core_sdk::prelude::*;
use flare_im_core_sdk::{ErrorCode, FlareError};

const USER_ID: &str = "alice";

/// 租户闸门的替身：`outage` 期间（网关读不到投影）每次 CONNECT 都按暂时性拒绝；`suspended`
/// 时按租户不可用拒绝；其余放行。
#[derive(Default)]
struct TenantGate {
    outage: AtomicBool,
    suspended: AtomicBool,
    attempts: AtomicUsize,
    transient_rejections: AtomicUsize,
    admitted: AtomicUsize,
}

impl TenantGate {
    fn attempts(&self) -> usize {
        self.attempts.load(Ordering::SeqCst)
    }

    fn transient_rejections(&self) -> usize {
        self.transient_rejections.load(Ordering::SeqCst)
    }

    fn admitted(&self) -> usize {
        self.admitted.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Authenticator for TenantGate {
    async fn authenticate(
        &self,
        _token: &str,
        _connection_id: &str,
        _device_info: Option<&DeviceInfo>,
        _metadata: Option<&HashMap<String, Vec<u8>>>,
    ) -> flare_core::common::error::Result<AuthResult> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        if self.outage.load(Ordering::SeqCst) {
            self.transient_rejections.fetch_add(1, Ordering::SeqCst);
            return Ok(AuthResult::retryable_failure(
                "tenant runtime projection unavailable".to_string(),
            ));
        }
        if self.suspended.load(Ordering::SeqCst) {
            return Ok(AuthResult::failure("TENANT_UNAVAILABLE".to_string()));
        }
        self.admitted.fetch_add(1, Ordering::SeqCst);
        Ok(AuthResult::success(Some(USER_ID.to_string())).with_tenant_id("tenant-a"))
    }
}

struct NoopHandler;

#[async_trait]
impl ServerEventHandler for NoopHandler {}

fn free_tcp_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind a probe TCP socket")
        .local_addr()
        .expect("probe address")
        .port()
}

async fn start_server(gate: Arc<TenantGate>) -> (FlareServer, String) {
    let address = format!("127.0.0.1:{}", free_tcp_port());
    let server = FlareServerBuilder::new(address.clone(), Arc::new(NoopHandler))
        .with_protocols(vec![TransportProtocol::WebSocket])
        .with_protocol_address(TransportProtocol::WebSocket, address.clone())
        .enable_auth()
        .with_authenticator(gate)
        .build()
        .expect("build WebSocket server");
    server.start().await.expect("start WebSocket server");
    (server, format!("ws://{address}"))
}

/// 本用例的 SDK 数据根：在 Cargo 给集成测试的临时目录下，用例结束时删掉。
fn data_root(case: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("flare-im-core-sdk-transient-rejection")
        .join(format!("{case}-{}", std::process::id()))
}

/// 内存存储的客户端；重连退避间隔取 1 s（默认 5 s 会让用例跑上半分钟）。
async fn client(ws_url: &str, data_root: &Path) -> IMClient {
    let client = IMClient::builder()
        .config(SdkConfig::new(ws_url))
        .stores(in_memory_im_provider())
        .build()
        .expect("build IMClient");
    client
        .init(
            None,
            Some(SdkConfigOverlay {
                data_url: Some(data_root.display().to_string()),
                ws_url: Some(ws_url.to_string()),
                reconnect_interval_secs: Some(1),
                ..Default::default()
            }),
        )
        .await
        .expect("init SDK config");
    client
}

async fn wait_until<F, Fut>(what: &str, deadline: Duration, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let started = Instant::now();
    while !check().await {
        assert!(
            started.elapsed() < deadline,
            "{what} not reached within {deadline:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// 服务端踢掉当前唯一的连接（模拟网关重启、网络切换等任何会触发重连的断线）。
async fn drop_the_connection(server: &FlareServer) {
    let connection_id = server
        .get_server_handle_components()
        .expect("connection manager")
        .list_connections()
        .await
        .into_iter()
        .next()
        .expect("the client connection");
    server
        .disconnect(&connection_id)
        .await
        .expect("server closes the connection");
}

/// SDK 发出的连接事件里与本用例相关的几类：重连循环的第几次尝试、交给应用的 KickedOff、
/// 断开原因。
#[derive(Default)]
struct ConnectionEvents {
    max_reconnect_attempt: AtomicUsize,
    kicked_off: std::sync::Mutex<Vec<String>>,
    disconnected: std::sync::Mutex<Vec<String>>,
}

impl ConnectionEvents {
    /// `on_reconnecting` 在 SDK 宣布第 n 次重连尝试时调用（发生在退避之前、CONNECT 之前）。
    fn record(
        client: &IMClient,
        on_reconnecting: impl Fn(usize) + Send + Sync + 'static,
    ) -> (Arc<Self>, Subscription) {
        let events = Arc::new(Self::default());
        let sink = events.clone();
        let subscription = client
            .on_event_kind(SdkEventKind::Connection, move |event| {
                match event.as_ref() {
                    SdkEvent::Connection(ConnectionEvent::Reconnecting { attempt }) => {
                        let attempt = *attempt as usize;
                        sink.max_reconnect_attempt
                            .fetch_max(attempt, Ordering::SeqCst);
                        on_reconnecting(attempt);
                    }
                    SdkEvent::Connection(ConnectionEvent::KickedOff { reason }) => {
                        sink.kicked_off.lock().unwrap().push(reason.clone());
                    }
                    SdkEvent::Connection(ConnectionEvent::Disconnected { reason }) => {
                        sink.disconnected.lock().unwrap().push(reason.clone());
                    }
                    _ => {}
                }
            })
            .expect("subscribe connection events");
        (events, subscription)
    }

    fn max_reconnect_attempt(&self) -> usize {
        self.max_reconnect_attempt.load(Ordering::SeqCst)
    }

    fn kicked_off(&self) -> Vec<String> {
        self.kicked_off.lock().unwrap().clone()
    }

    fn disconnected(&self) -> Vec<String> {
        self.disconnected.lock().unwrap().clone()
    }
}

/// 投影库暂时读不到：首次建连拿到可重试的 `SERVICE_UNAVAILABLE`（不是鉴权失败、不是租户
/// 不可用、也没被降级成连不上）；会话中断线后，SDK 重连循环的第一次尝试被暂时性拒绝，循环继续
/// 退避，第二次尝试时投影库已恢复、放行，回到 Ready，会话不终止，也没有 KickedOff。
///
/// 投影库在 SDK 宣布第二次重连尝试时恢复，而不是按 CONNECT 计数：断线后 flare-core 客户端
/// 还可能因为同步请求做几次惰性重连，计数会被它们吃掉。这样第一次尝试必定碰上故障，只有
/// 重连循环在暂时性拒绝之后继续，才会有第二次尝试。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_outage_is_retried_until_the_gateway_admits() {
    let gate = Arc::new(TenantGate::default());
    let (server, ws_url) = start_server(gate.clone()).await;
    let root = data_root("outage");
    let client = client(&ws_url, &root).await;

    // 首次建连时投影读不到：错误按服务端给的类别上报，应用据此稍后重试。
    gate.outage.store(true, Ordering::SeqCst);
    let error: FlareError = match client.connect(USER_ID, Some("token")).await {
        Ok(_) => panic!("CONNECT must be rejected while the projection is unreadable"),
        Err(error) => error,
    };
    assert_eq!(error.code(), Some(ErrorCode::ServiceUnavailable), "{error}");
    assert!(error.to_string().contains("SERVICE_UNAVAILABLE"), "{error}");
    assert!(!error.to_string().contains("TENANT_UNAVAILABLE"), "{error}");
    assert!(
        !error
            .to_string()
            .contains("tenant runtime projection unavailable"),
        "server-side text must not reach the client: {error}"
    );
    assert_eq!(gate.transient_rejections(), 1);

    gate.outage.store(false, Ordering::SeqCst);
    client
        .connect(USER_ID, Some("token"))
        .await
        .expect("connects once the projection is readable again");
    wait_until("Ready after connect", Duration::from_secs(10), || async {
        client.state() == SdkState::Ready
    })
    .await;
    assert_eq!(gate.admitted(), 1);
    let recovering_gate = gate.clone();
    let (events, _subscription) = ConnectionEvents::record(&client, move |attempt| {
        if attempt >= 2 {
            recovering_gate.outage.store(false, Ordering::SeqCst);
        }
    });

    // 会话中断线时投影库正读不到。
    gate.outage.store(true, Ordering::SeqCst);
    let rejections_before = gate.transient_rejections();
    drop_the_connection(&server).await;

    wait_until(
        "reconnect admitted after the outage",
        Duration::from_secs(30),
        || async { gate.admitted() >= 2 },
    )
    .await;
    wait_until("Ready after reconnect", Duration::from_secs(10), || async {
        client.state() == SdkState::Ready
    })
    .await;

    assert!(
        gate.transient_rejections() > rejections_before,
        "the first reconnect attempt must have hit the outage"
    );
    assert!(
        events.max_reconnect_attempt() >= 2,
        "the SDK reconnect loop must retry after a transient rejection (max attempt {})",
        events.max_reconnect_attempt()
    );
    assert!(
        events.kicked_off().is_empty(),
        "a transient rejection must not end the session: {:?}",
        events.kicked_off()
    );
    assert_eq!(client.current_user_id().await.as_deref(), Some(USER_ID));

    let _ = client.disconnect().await;
    server.stop().await.expect("stop server");
    let _ = std::fs::remove_dir_all(&root);
}

/// 对照：租户确实不可用（`TENANT_UNAVAILABLE`）时，第一次重连被拒就把 KickedOff 交给应用、
/// 终止会话，不再重连。与上一条用的是同一个回环服务端和重连循环，只差拒绝原因。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tenant_unavailable_ends_the_session_without_retrying() {
    let gate = Arc::new(TenantGate::default());
    let (server, ws_url) = start_server(gate.clone()).await;
    let root = data_root("suspended");
    let client = client(&ws_url, &root).await;

    client
        .connect(USER_ID, Some("token"))
        .await
        .expect("initial connect");
    wait_until("Ready after connect", Duration::from_secs(10), || async {
        client.state() == SdkState::Ready
    })
    .await;
    let (events, _subscription) = ConnectionEvents::record(&client, |_| {});

    gate.suspended.store(true, Ordering::SeqCst);
    drop_the_connection(&server).await;

    wait_until("session terminated", Duration::from_secs(15), || async {
        client.current_user_id().await.is_none()
    })
    .await;
    assert_eq!(events.kicked_off(), ["TENANT_UNAVAILABLE".to_string()]);
    assert_eq!(
        events.max_reconnect_attempt(),
        1,
        "the reconnect loop stops at the first TENANT_UNAVAILABLE"
    );
    // 再等过第二次尝试的退避上限：不能再有 CONNECT
    let attempts_after_rejection = gate.attempts();
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert_eq!(gate.attempts(), attempts_after_rejection);
    assert_eq!(events.max_reconnect_attempt(), 1);

    let _ = client.disconnect().await;
    server.stop().await.expect("stop server");
    let _ = std::fs::remove_dir_all(&root);
}

/// 重连间隔与次数只经 builder 配置、`init()` 不覆盖时，SDK 自己的重连循环也照它退避，到次数
/// 即停手并报「reconnect attempts exhausted」。此前这两项只到 flare-core 客户端，SDK 的循环
/// 仍按默认 5 s、无限次重连，本用例的时限内既到不了第 2 次，也永远不会停。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn builder_reconnect_settings_drive_the_sdk_reconnect_loop() {
    let gate = Arc::new(TenantGate::default());
    let (server, ws_url) = start_server(gate.clone()).await;
    let root = data_root("builder-backoff");
    let mut config = SdkConfig::new(&ws_url);
    config.reconnect_interval_secs = Some(1);
    config.max_reconnect_attempts = Some(2);
    let client = IMClient::builder()
        .config(config)
        .stores(in_memory_im_provider())
        .build()
        .expect("build IMClient");
    client
        .init(
            None,
            Some(SdkConfigOverlay {
                data_url: Some(root.display().to_string()),
                ws_url: Some(ws_url.clone()),
                ..Default::default()
            }),
        )
        .await
        .expect("init SDK config");
    client
        .connect(USER_ID, Some("token"))
        .await
        .expect("initial connect");
    wait_until("Ready after connect", Duration::from_secs(10), || async {
        client.state() == SdkState::Ready
    })
    .await;
    let (events, _subscription) = ConnectionEvents::record(&client, |_| {});

    // 投影库一直读不到：每次重连都被暂时性拒绝，循环只能靠次数上限停下。
    gate.outage.store(true, Ordering::SeqCst);
    drop_the_connection(&server).await;

    // 1 s 间隔下两次退避合计 3–5 s；默认 5 s 时第 2 次尝试要到 12 s 之后。
    wait_until(
        "reconnect attempts exhausted",
        Duration::from_secs(12),
        || async {
            events
                .disconnected()
                .iter()
                .any(|reason| reason.contains("reconnect attempts exhausted"))
        },
    )
    .await;
    assert_eq!(events.max_reconnect_attempt(), 2);
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        events.max_reconnect_attempt(),
        2,
        "the reconnect loop stops at the configured attempt limit"
    );
    assert!(events.kicked_off().is_empty(), "{:?}", events.kicked_off());

    let _ = client.disconnect().await;
    server.stop().await.expect("stop server");
    let _ = std::fs::remove_dir_all(&root);
}
