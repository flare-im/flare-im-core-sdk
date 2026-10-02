//! 被同平台新设备顶下线：会话终止、上报 KickedOff，不再自动重连；应用重新登录才恢复。
//!
//! 本机回环（只绑 127.0.0.1）起一个真实的 flare-core WebSocket 服务端（平台互斥的设备管理器）。
//! 同一账号的两个 SDK 客户端都以 PC 平台登录（设备 ID 不同），第二台登录时服务端把第一台踢下线。
//!
//! 复现的是 W8-S1 F1：被踢的一端停在「看似 Ready、实际不可用」——不发 KickedOff、状态仍是 Ready，
//! 或被当成对端断开、几秒后由重连监视器连回去（再把新设备顶下线）。

#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use flare_core::common::config_types::TransportProtocol;
use flare_core::common::device::DeviceInfo;
use flare_core::server::{
    AuthResult, Authenticator, DeviceConflictStrategyBuilder, DeviceManager, FlareServer,
    FlareServerBuilder, ServerEventHandler,
};
use flare_im_core_sdk::prelude::*;

const USER_ID: &str = "alice";
const DEVICE_CONFLICT: &str = "DEVICE_CONFLICT";
/// 重连监视器的退避间隔：第一次重连在断开后 0.8–1.2 s 之间。
const RECONNECT_INTERVAL_SECS: u64 = 1;

/// 一律放行，记下放行次数（每次 CONNECT 一次）。
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

async fn start_server(gate: Arc<AdmitAll>) -> (FlareServer, String) {
    let address = format!("127.0.0.1:{}", free_tcp_port());
    let device_manager = Arc::new(DeviceManager::new(
        DeviceConflictStrategyBuilder::new()
            .platform_exclusive()
            .build(),
    ));
    let server = FlareServerBuilder::new(address.clone(), Arc::new(NoopHandler))
        .with_protocols(vec![TransportProtocol::WebSocket])
        .with_protocol_address(TransportProtocol::WebSocket, address.clone())
        .with_device_manager(device_manager)
        .enable_auth()
        .with_authenticator(gate)
        .build()
        .expect("build WebSocket server");
    server.start().await.expect("start WebSocket server");
    (server, format!("ws://{address}"))
}

/// 本用例的 SDK 数据根：在 Cargo 给集成测试的临时目录下。
fn data_root(case: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("flare-im-core-sdk-device-conflict-kick")
        .join(format!("{case}-{}", std::process::id()))
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
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// 应用看得到的连接事件。
#[derive(Default)]
struct Observed {
    kicked_off: Mutex<Vec<String>>,
    disconnected: Mutex<Vec<String>>,
    reconnecting: Mutex<Vec<u32>>,
}

impl Observed {
    fn record(client: &IMClient) -> (Arc<Self>, Subscription) {
        let observed = Arc::new(Self::default());
        let sink = observed.clone();
        let subscription = client
            .on_event_kind(SdkEventKind::Connection, move |event| {
                match event.as_ref() {
                    SdkEvent::Connection(ConnectionEvent::KickedOff { reason }) => {
                        sink.kicked_off.lock().unwrap().push(reason.clone());
                    }
                    SdkEvent::Connection(ConnectionEvent::Disconnected { reason }) => {
                        sink.disconnected.lock().unwrap().push(reason.clone());
                    }
                    SdkEvent::Connection(ConnectionEvent::Reconnecting { attempt }) => {
                        sink.reconnecting.lock().unwrap().push(*attempt);
                    }
                    _ => {}
                }
            })
            .expect("subscribe connection events");
        (observed, subscription)
    }

    fn kicked_off(&self) -> Vec<String> {
        self.kicked_off.lock().unwrap().clone()
    }
}

async fn new_client(ws_url: &str, device_id: &str, data_root: &Path) -> IMClient {
    let mut config = SdkConfig::new(ws_url);
    config.device_id = Some(device_id.to_string());
    let client = IMClient::builder()
        .config(config)
        .stores(in_memory_im_provider())
        .build()
        .expect("build IMClient");
    client
        .init(
            None,
            Some(SdkConfigOverlay {
                data_url: Some(data_root.display().to_string()),
                ws_url: Some(ws_url.to_string()),
                reconnect_interval_secs: Some(RECONNECT_INTERVAL_SECS),
                ..Default::default()
            }),
        )
        .await
        .expect("init SDK config");
    client
}

async fn log_in(client: &IMClient) {
    client
        .connect(USER_ID, Some("token"))
        .await
        .expect("connect");
    wait_until("Ready after connect", Duration::from_secs(10), || async {
        client.state() == SdkState::Ready
    })
    .await;
}

/// 第二台同平台设备登录：第一台收到恰好一次 `KickedOff { reason: "DEVICE_CONFLICT" }`，状态为
/// Disconnected、会话终止（不再有登录用户），监视器不重连——几个退避周期内服务端没有新的 CONNECT，
/// 新设备一直在线。应用重新登录后第一台恢复（同平台规则下这回轮到新设备被顶下线）。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_device_conflict_kick_ends_the_session_without_reconnecting() {
    let gate = Arc::new(AdmitAll::default());
    let (server, ws_url) = start_server(gate.clone()).await;

    let owner_root = data_root("owner");
    let owner = new_client(&ws_url, "owner-device", &owner_root).await;
    log_in(&owner).await;
    let (owner_events, _owner_subscription) = Observed::record(&owner);
    assert_eq!(gate.admitted.load(Ordering::SeqCst), 1);

    let second_root = data_root("second");
    let second = new_client(&ws_url, "second-device", &second_root).await;
    log_in(&second).await;
    let (second_events, _second_subscription) = Observed::record(&second);

    wait_until(
        "owner reports KickedOff",
        Duration::from_secs(5),
        || async { !owner_events.kicked_off().is_empty() },
    )
    .await;
    wait_until(
        "owner session terminated",
        Duration::from_secs(5),
        || async { !owner.is_connected().await && owner.state() == SdkState::Disconnected },
    )
    .await;

    // 几个退避周期（每个 0.8–1.2 s）：被踢的一端不重连，状态不变，新设备一直在线。
    tokio::time::sleep(Duration::from_secs(4 * RECONNECT_INTERVAL_SECS)).await;
    assert_eq!(owner_events.kicked_off(), vec![DEVICE_CONFLICT]);
    assert!(
        owner_events.reconnecting.lock().unwrap().is_empty(),
        "a kicked session must not reconnect: {:?}",
        owner_events.reconnecting.lock().unwrap()
    );
    assert_eq!(
        gate.admitted.load(Ordering::SeqCst),
        2,
        "no CONNECT after the kick"
    );
    assert_eq!(owner.state(), SdkState::Disconnected);
    assert!(!owner.is_connected().await);
    assert_eq!(
        server.connection_count(),
        1,
        "only the new device is online"
    );
    assert_eq!(second.state(), SdkState::Ready);
    assert!(second_events.kicked_off().is_empty());

    // 应用重新登录（会话已终止，引擎随之释放，要走完整登录）：被踢的一端恢复，同平台规则下
    // 新设备被顶下线。
    owner
        .login(
            USER_ID,
            Some("token"),
            LoginDbKind::IndexedDb(in_memory_im_provider()),
            |_, _| {},
        )
        .await
        .expect("log in again");
    wait_until(
        "Ready after logging in again",
        Duration::from_secs(10),
        || async { owner.state() == SdkState::Ready },
    )
    .await;
    wait_until(
        "the new device is kicked in turn",
        Duration::from_secs(5),
        || async { second_events.kicked_off() == vec![DEVICE_CONFLICT.to_string()] },
    )
    .await;
    assert_eq!(gate.admitted.load(Ordering::SeqCst), 3);
    assert_eq!(owner.state(), SdkState::Ready);

    owner.disconnect().await.ok();
    second.disconnect().await.ok();
    server.stop().await.expect("stop server");
    let _ = std::fs::remove_dir_all(&owner_root);
    let _ = std::fs::remove_dir_all(&second_root);
}
