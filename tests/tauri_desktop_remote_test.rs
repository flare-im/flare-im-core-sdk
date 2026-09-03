//! Tauri 桌面端联调：走**与 app 完全相同**的入口——`IMClient::login` +
//! `LoginDbKind::Sqlite`（`sdk_login` 命令就是这两行的透传），连真实网关，
//! 断言登录 → 会话同步 → **实时推送到达**。
//!
//! 为什么不驱动窗口：Tauri 是原生 macOS 窗口，没有可靠的自动化输入通道，
//! 而 256 字符的接入 token 手敲进去在 Android 上实测会被截断。直接驱动会话层
//! 验证的是同一套栈（核心 + SQLite 仓储 + 传输），而且可复跑、能进 CI。
//!
//! 默认跳过。要跑就给环境变量——**别把 token 写进仓库**：
//!   FLARE_E2E_WS_URL=ws://<host>/ws \
//!   FLARE_E2E_TOKEN="$(ssh <server> mint_token.py <user>)" \
//!   FLARE_E2E_USER=<user> \
//!   FLARE_E2E_TAG=<发送端写进消息正文的标记> \
//!   cargo test --features lifecycle-sqlite --test tauri_desktop_remote_test -- --nocapture

use std::env;
use std::time::{Duration, Instant};

use flare_im_core_sdk::client::IMClient;
use flare_im_core_sdk::client::lifecycle::{LoginDbKind, SdkConfigOverlay};
use flare_im_core_sdk::model::StartupHomeSyncRequest;
use flare_im_core_sdk::prelude::MessageEvent;
use flare_im_core_sdk::SdkEvent;

fn env_var(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.is_empty())
}

// 必须多线程：核心的 `run_client_async` 用 `block_in_place` 从同步上下文跨进
// 异步，而它在 current-thread runtime 上直接 panic（"can call blocking only when
// running on the multi-threaded runtime"）。Tauri 跑的就是多线程 runtime，
// 单线程跑这条用例等于在测一个真实客户端不会进入的形态。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn desktop_stack_logs_in_and_receives_realtime_push() {
    let (Some(ws_url), Some(token), Some(user_id)) = (
        env_var("FLARE_E2E_WS_URL"),
        env_var("FLARE_E2E_TOKEN"),
        env_var("FLARE_E2E_USER"),
    ) else {
        // 没给环境变量就跳过，保持默认 `cargo test` 不依赖外部环境。
        return;
    };

    // 装上 subscriber，否则核心的 tracing 一行都不会输出——
    // 我曾据此误判"事件根本没到"。
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_test_writer()
        .try_init();

    let data_root = std::env::temp_dir().join(format!(
        "flare-tauri-e2e-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&data_root).expect("create data root");

    let client = IMClient::new();
    client
        .init(
            None,
            Some(SdkConfigOverlay {
                data_url: Some(format!("file://{}", data_root.display())),
                ws_url: Some(ws_url),
                tenant_id: Some("0".to_string()),
                ..Default::default()
            }),
        )
        .await
        .expect("init");

    // 订阅在 login 的 before_connect 闭包里建立——和 sdk_login 把事件桥接到 webview
    // 的时机完全一致：晚于此就会漏掉首批推送。
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SdkEvent>();
    client
        .login(&user_id, Some(&token), LoginDbKind::Sqlite, move |bus, _| {
            let mut raw = bus.subscribe_shared_raw();
            tokio::spawn(async move {
                while let Ok(event) = raw.recv().await {
                    if tx.send(event.cloned_event()).is_err() {
                        break;
                    }
                }
            });
        })
        .await
        .expect("login against the real gateway must succeed");

    assert_eq!(
        client.current_user_id().await,
        Some(user_id.clone()),
        "登录后当前用户应当是 {user_id}"
    );

    let conversations = client
        .conversation_async()
        .await
        .expect("conversation api must be assembled after login");

    // 冷启出图走的就是这一条（app 首屏同款），首次登录本地库是空的，
    // 列表要等同步落库，所以按截止时间轮询而不是登录后立刻断言。
    client
        .bootstrap_startup_home(StartupHomeSyncRequest {
            conversation_limit: 50,
            start_background_convergence: true,
            ..Default::default()
        })
        .await
        .expect("冷启首屏同步必须成功");

    let list_deadline = Instant::now() + Duration::from_secs(60);
    let mut summaries = Vec::new();
    while Instant::now() < list_deadline {
        summaries = conversations.list().await.expect("会话列表必须能拉起来");
        if !summaries.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(
        !summaries.is_empty(),
        "该账号在生产上有会话，60 秒内列表不该还是空的"
    );
    println!("CONVERSATIONS={}", summaries.len());

    // 没给 tag 就只验到登录 + 会话列表：实时推送需要另一端配合发送。
    let Some(tag) = env_var("FLARE_E2E_TAG") else {
        return;
    };

    println!("LISTENER_READY");
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut hit = false;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Ok(Some(event)) = tokio::time::timeout(remaining, rx.recv()).await else {
            break;
        };
        // 只认批量：批量是规范路径，逐条回调对聊天消息不触发。
        let SdkEvent::Message(MessageEvent::ReceivedBatch { messages }) = event else {
            continue;
        };
        if messages
            .iter()
            .any(|message| format!("{message:?}").contains(&tag))
        {
            hit = true;
            println!("RECEIVED {tag}");
            break;
        }
    }
    assert!(hit, "240 秒内没有收到带标记 [{tag}] 的实时推送");
}
