//! 服务端停机后，SDK 在短暂的随机等待后重连，而不是按错误的退避节奏。
//!
//! 本机回环（只绑 127.0.0.1）起一个真实的 flare-core WebSocket 服务端。服务端停机时先给在线连接
//! 下发 CLOSE(SERVER_SHUTDOWN)，flare-core 客户端以这个原因断开；SDK 的重连循环据此把第一次重连前的
//! 等待换成 1–5 s 的随机一段。这里把重连间隔配成 30 s：按退避，第一次重连要到 24 s 之后。

#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use flare_core::common::config_types::TransportProtocol;
use flare_core::common::device::DeviceInfo;
use flare_core::common::shutdown::{SERVER_SHUTDOWN_RECONNECT_MIN, is_server_shutdown_reason};
use flare_core::server::{
    AuthResult, Authenticator, FlareServer, FlareServerBuilder, ServerEventHandler,
};
use flare_im_core_sdk::prelude::*;

const USER_ID: &str = "alice";

/// 一律放行，记下放行次数。
#[derive(Default)]
struct AdmitAll {
    admitted: AtomicUsize,
}

#[async_trait]
impl Authenticator for AdmitAll {
    async fn authenticate(
        &self,
        _token: &str,
        _connection_id: &str,
        _device_info: Option<&DeviceInfo>,
        _metadata: Option<&HashMap<String, Vec<u8>>>,
    ) -> flare_core::common::error::Result<AuthResult> {
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

async fn start_server(port: u16, gate: Arc<AdmitAll>) -> FlareServer {
    let address = format!("127.0.0.1:{port}");
    let server = FlareServerBuilder::new(address.clone(), Arc::new(NoopHandler))
        .with_protocols(vec![TransportProtocol::WebSocket])
        .with_protocol_address(TransportProtocol::WebSocket, address)
        .enable_auth()
        .with_authenticator(gate)
        .build()
        .expect("build WebSocket server");
    server.start().await.expect("start WebSocket server");
    server
}

/// 本用例的 SDK 数据根：在 Cargo 给集成测试的临时目录下，用例结束时删掉。
fn data_root(case: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("flare-im-core-sdk-server-shutdown")
        .join(format!("{case}-{}", std::process::id()))
}

/// 重连间隔 30 s：按退避，第一次重连在 24–30 s 之后。
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
                reconnect_interval_secs: Some(30),
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

#[derive(Default)]
struct ConnectionEvents {
    max_reconnect_attempt: AtomicUsize,
    kicked_off: std::sync::Mutex<Vec<String>>,
    disconnected: std::sync::Mutex<Vec<String>>,
}

impl ConnectionEvents {
    fn record(client: &IMClient) -> (Arc<Self>, Subscription) {
        let events = Arc::new(Self::default());
        let sink = events.clone();
        let subscription = client
            .on_event_kind(SdkEventKind::Connection, move |event| {
                match event.as_ref() {
                    SdkEvent::Connection(ConnectionEvent::Reconnecting { attempt }) => {
                        sink.max_reconnect_attempt
                            .fetch_max(*attempt as usize, Ordering::SeqCst);
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
}

/// 停机：SDK 看到的断开原因带 SERVER_SHUTDOWN；同一端口上的新实例起来后，SDK 在 1–5 s 的随机
/// 等待后重连回到 Ready，会话不终止，只用了一次重连尝试——而不是等 30 s 间隔的退避。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn server_shutdown_reconnects_after_a_short_pause_instead_of_the_backoff() {
    let gate = Arc::new(AdmitAll::default());
    let port = free_tcp_port();
    let server = start_server(port, gate.clone()).await;
    let root = data_root("shutdown");
    let client = client(&format!("ws://127.0.0.1:{port}"), &root).await;
    client
        .connect(USER_ID, Some("token"))
        .await
        .expect("initial connect");
    wait_until("Ready after connect", Duration::from_secs(10), || async {
        client.state() == SdkState::Ready
    })
    .await;
    let (events, _subscription) = ConnectionEvents::record(&client);

    let stopped_at = Instant::now();
    server.stop().await.expect("stop server");
    wait_until("SDK sees the shutdown", Duration::from_secs(3), || async {
        !events.disconnected.lock().unwrap().is_empty()
    })
    .await;
    let reasons = events.disconnected.lock().unwrap().clone();
    assert!(
        reasons
            .iter()
            .any(|reason| is_server_shutdown_reason(reason)),
        "the SDK must see the typed shutdown reason: {reasons:?}"
    );

    // 新实例在同一端口上起来（停机已释放监听端口）
    let restarted = start_server(port, gate.clone()).await;
    wait_until(
        "Ready again after the shutdown",
        Duration::from_secs(12),
        || async { gate.admitted.load(Ordering::SeqCst) >= 2 && client.state() == SdkState::Ready },
    )
    .await;
    let recovered_in = stopped_at.elapsed();
    assert!(
        recovered_in >= SERVER_SHUTDOWN_RECONNECT_MIN,
        "the SDK must pause before reconnecting after a shutdown: {recovered_in:?}"
    );
    assert_eq!(
        events.max_reconnect_attempt.load(Ordering::SeqCst),
        1,
        "one reconnect attempt after the pause is enough"
    );
    assert!(
        events.kicked_off.lock().unwrap().is_empty(),
        "a server shutdown does not end the session"
    );
    assert_eq!(client.current_user_id().await.as_deref(), Some(USER_ID));

    let _ = client.disconnect().await;
    restarted.stop().await.expect("stop restarted server");
    let _ = std::fs::remove_dir_all(&root);
}
