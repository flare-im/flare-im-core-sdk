//! 一对一聊天示例：**仅通过** [`flare_im_core_sdk::client`] 对外能力完成（[`IMClient`]、[`LoginDbKind`]、
//! [`SdkConfigOverlay`]、路径工具、[`IMClient::on_*`] 订阅、[`MessageApi`] / [`ConversationApi`] / [`MessageBuildApi`]）。
//!
//! 需启用 **`lifecycle-sqlite`**。先启动 IM 服务端（默认 `ws://localhost:60051`），再开 **两个终端** 分别执行：
//!
//! ## 启动命令（复制即用）
//!
//! **用户 Alice（与 Bob 聊）**
//! ```text
//! RUST_LOG=info MY_USER_ID=user-alice CHAT_WITH=user-bob SERVER_URL=ws://localhost:60051 cargo run -p flare-im-core-sdk --example two_clients_chat --features lifecycle-sqlite
//! ```
//!
//! **用户 Bob（与 Alice 聊）**
//! ```text
//! RUST_LOG=info MY_USER_ID=user-bob CHAT_WITH=user-alice SERVER_URL=ws://localhost:60051 cargo run -p flare-im-core-sdk --example two_clients_chat --features lifecycle-sqlite
//! ```
//!
//! 在仓库根目录（`flare-im`）执行；若当前目录已是 `flare-im-core-sdk`，可改为：
//! `cargo run --example two_clients_chat --features lifecycle-sqlite`（环境变量同上）。
//!
//! ## 双终端（与上等价，可换行书写）
//!
//! **终端 1 — Alice**
//! ```bash
//! RUST_LOG=info MY_USER_ID=user-alice CHAT_WITH=user-bob SERVER_URL=ws://localhost:60051 cargo run -p flare-im-core-sdk --example two_clients_chat --features lifecycle-sqlite
//! ```
//!
//! **终端 2 — Bob**
//! ```bash
//! RUST_LOG=info MY_USER_ID=user-bob CHAT_WITH=user-alice SERVER_URL=ws://localhost:60051 \
//!   cargo run -p flare-im-core-sdk --example two_clients_chat --features lifecycle-sqlite
//! ```
//!
//! 未设置 `MY_USER_ID` / `CHAT_WITH` 时默认 **`user-alice`** / **`user-bob`**。可选 `FLARE_DATA_DIR`；
//! 否则数据目录为 `temp-data/two_clients_chat/<sanitized_user>/`（与 [`dev_data_dir_relative_to_cwd`]、[`sanitize_user_id_for_dir`] 一致）。
//!
//! **订阅**：在 [`IMClient::login`].await **之后立刻**调用 [`IMClient::on_sync_finished`] / [`IMClient::on_message`] 等；
//! 若你的环境里首轮同步极快，仍可选用 `login` 的 `before_connect` 里对传入的 `EventBus` 注册（与 `on_*` 等价）。

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use flare_im_core_sdk::client::{
    IMClient, LoginDbKind, SdkConfigOverlay, dev_data_dir_relative_to_cwd, parse_data_url_to_path,
    sanitize_user_id_for_dir,
};
use flare_im_core_sdk::core::SdkState;
use flare_im_core_sdk::event::{SdkEvent, SyncPhase};
use flare_im_core_sdk::model::conversation::ConversationType;
use flare_im_core_sdk::prelude::*;
use tokio::io::{self, AsyncBufReadExt};
use tracing::{error, info, warn};

const DEFAULT_SELF: &str = "user-alice";
const DEFAULT_PEER: &str = "user-bob";

fn text_preview_from_message(msg: &IMMessage) -> String {
    decode_content_bytes(&msg.content_bytes)
        .map(|d| d.text_preview().to_string())
        .unwrap_or_else(|_| format!("(decode err server_id={})", msg.server_id))
}

/// 构造 `dataUrl` 并校验与 [`parse_data_url_to_path`] 一致。
fn resolve_data_url(my_user_id: &str) -> anyhow::Result<String> {
    let base: PathBuf = std::env::var("FLARE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dev_data_dir_relative_to_cwd()
                .join("two_clients_chat")
                .join(sanitize_user_id_for_dir(my_user_id))
        });
    std::fs::create_dir_all(&base).context("create data dir")?;
    let raw = base.to_string_lossy().replace('\\', "/");
    let url = if raw.starts_with("file://") {
        raw
    } else {
        format!("file://{}", raw)
    };
    let _ = parse_data_url_to_path(&url).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(url)
}

/// 全部使用 [`IMClient`] 提供的 `on_*`（与直接调 `EventBus` 等价，便于与 App 集成方式对齐）。
fn register_subscriptions(
    client: &IMClient,
    sync_done_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    sync_event_count: Arc<AtomicUsize>,
) -> anyhow::Result<()> {
    client
        .on_sync_state_changed(|state| info!("[sync state] {:?}", state))
        .map_err(|e| anyhow::anyhow!(e))?;
    client
        .on_sync_started(|| info!("[sync] started"))
        .map_err(|e| anyhow::anyhow!(e))?;

    let sync_done_cb = Arc::clone(&sync_done_tx);
    client
        .on_sync_finished(move |phase| {
            info!("[sync] finished phase={:?}", phase);
            if matches!(phase, SyncPhase::Background) {
                if let Ok(mut g) = sync_done_cb.lock() {
                    if let Some(tx) = g.take() {
                        let _ = tx.send(());
                    }
                }
            }
        })
        .map_err(|e| anyhow::anyhow!(e))?;

    client
        .on_message(|msg| info!("[recv] {}", text_preview_from_message(msg)))
        .map_err(|e| anyhow::anyhow!(e))?;
    client
        .on_message_batch(|messages| {
            for msg in messages {
                info!("[recv] {}", text_preview_from_message(msg));
            }
        })
        .map_err(|e| anyhow::anyhow!(e))?;

    let cnt = Arc::clone(&sync_event_count);
    client
        .on_any(move |e| {
            if matches!(e.as_ref(), SdkEvent::Sync(_)) {
                cnt.fetch_add(1, Ordering::Relaxed);
            }
        })
        .map_err(|e| anyhow::anyhow!(e))?;

    client
        .on_server_error(|code, message| {
            warn!("server error code={} message={}", code, message);
        })
        .map_err(|e| anyhow::anyhow!(e))?;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(true)
        .try_init()
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;

    info!("Flare IM SDK 一对一聊天（仅用 client 对外 API）");
    info!("================================================");

    let ws_url = std::env::var("SERVER_URL").unwrap_or_else(|_| "ws://localhost:60051".to_string());
    let my_user_id = std::env::var("MY_USER_ID").unwrap_or_else(|_| DEFAULT_SELF.to_string());
    let chat_with = std::env::var("CHAT_WITH").unwrap_or_else(|_| DEFAULT_PEER.to_string());

    info!("server: {}", ws_url);
    info!("current user (MY_USER_ID): {}", my_user_id);
    info!("peer (CHAT_WITH): {}", chat_with);

    let data_url = resolve_data_url(&my_user_id)?;
    info!("dataUrl: {}", data_url);

    let client = IMClient::new();
    client
        .init(
            None,
            Some(SdkConfigOverlay {
                data_url: Some(data_url),
                ws_url: Some(ws_url.clone()),
                ..Default::default()
            }),
        )
        .await
        .context("client.init")?;

    let token =
        IMClient::generate_test_token("", "", &my_user_id, None).context("generate_test_token")?;

    let (sync_done_tx, sync_done_rx) = tokio::sync::oneshot::channel();
    let sync_done_tx = Arc::new(Mutex::new(Some(sync_done_tx)));
    let sync_event_count = Arc::new(AtomicUsize::new(0));

    info!("connecting...");
    client
        .login(&my_user_id, Some(&token), LoginDbKind::Sqlite, |_, _| {})
        .await
        .context("client.login")?;

    register_subscriptions(
        &client,
        Arc::clone(&sync_done_tx),
        Arc::clone(&sync_event_count),
    )
    .context("register_subscriptions")?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while client.state() != SdkState::Ready {
        if tokio::time::Instant::now() > deadline {
            anyhow::bail!("wait Ready timeout");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    info!("ready");

    match tokio::time::timeout(Duration::from_secs(20), sync_done_rx).await {
        Ok(Ok(())) => {
            let convs = client
                .conversation()
                .context("conversation")?
                .list()
                .await
                .context("conversation.list")?;
            info!("sync done, conversations: {}", convs.len());
            info!(
                "sync events seen: {}",
                sync_event_count.load(Ordering::Relaxed)
            );
        }
        Ok(Err(_)) => warn!("sync done channel closed"),
        Err(_) => {
            warn!("sync wait timeout (20s), continue");
            info!(
                "sync events seen: {}",
                sync_event_count.load(Ordering::Relaxed)
            );
        }
    }

    let conversation_id = client
        .conversation()
        .context("conversation")?
        .get_one(&chat_with, &ConversationType::Single)
        .await
        .context("get_one")?
        .conversation_id()
        .to_string();

    info!("conversation_id: {}", conversation_id);

    let msgs = client
        .message()
        .context("message")?
        .list(&conversation_id, u64::MAX, 50)
        .await
        .context("message.list")?;
    info!("synced messages: {}", msgs.len());
    for m in msgs.iter() {
        info!(
            "  [seq={}] sender={} text={}",
            m.seq,
            m.sender_id(),
            text_preview_from_message(m)
        );
    }

    info!("");
    info!("输入文字回车发送；/list /history /read /quit");
    info!("");

    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(32);

    let input_handle = {
        let tx = tx.clone();
        let conv_id = conversation_id.clone();
        let client_bg = client.clone();
        tokio::spawn(async move {
            let stdin = io::stdin();
            let mut reader = io::BufReader::new(stdin);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        let input = line.trim().to_string();
                        if input.is_empty() {
                            continue;
                        }
                        if input == "/quit" || input == "/exit" {
                            let _ = tx.send(input).await;
                            break;
                        }
                        if input == "/list" {
                            match client_bg.conversation() {
                                Ok(api) => match api.list().await {
                                    Ok(list) => {
                                        info!("conversations ({}):", list.len());
                                        for c in list.iter().take(10) {
                                            info!(
                                                "  {} unread={}",
                                                c.conversation_id(),
                                                c.unread_count()
                                            );
                                        }
                                    }
                                    Err(e) => warn!("list: {}", e),
                                },
                                Err(e) => warn!("conversation: {}", e),
                            }
                            continue;
                        }
                        if input == "/history" {
                            let cid = conv_id.clone();
                            let c = client_bg.clone();
                            match c.message() {
                                Ok(api) => match api.list(&cid, u64::MAX, 20).await {
                                    Ok(msgs) => {
                                        info!("recent {} messages:", msgs.len());
                                        for m in msgs.iter().take(10) {
                                            info!(
                                                "  [{}] {}",
                                                m.sender_id(),
                                                text_preview_from_message(m)
                                            );
                                        }
                                    }
                                    Err(e) => warn!("history: {}", e),
                                },
                                Err(e) => warn!("message: {}", e),
                            }
                            continue;
                        }
                        if input == "/read" {
                            let cid = conv_id.clone();
                            let c = client_bg.clone();
                            match c.mark_session_read(&cid, u64::MAX).await {
                                Ok(()) => info!("marked read (message + conversation + sync ack)"),
                                Err(e) => warn!("mark_session_read: {}", e),
                            }
                            continue;
                        }
                        if let Err(e) = tx.send(input).await {
                            error!("send to main loop: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        error!("stdin: {}", e);
                        break;
                    }
                }
            }
        })
    };

    while let Some(text) = rx.recv().await {
        if text == "/quit" || text == "/exit" {
            break;
        }

        let conversation_id = conversation_id.clone();
        let c = client.clone();
        let send_result = async move {
            let msg = c
                .message_build()
                .map_err(|e| e.to_string())?
                .create_text(&conversation_id, &text)
                .await
                .map_err(|e| e.to_string())?;
            let mut tried = 0u32;
            let mut delay = Duration::from_millis(500);
            loop {
                match c
                    .message()
                    .map_err(|e| e.to_string())?
                    .send(msg.clone())
                    .await
                {
                    Ok(ack) => {
                        if ack.success {
                            info!("sent seq={}", ack.seq);
                        } else {
                            warn!("send not success server_msg_id={}", ack.server_msg_id);
                        }
                        return Ok::<(), String>(());
                    }
                    Err(e) => {
                        tried += 1;
                        if tried >= 5 {
                            return Err(format!("send failed after retries: {}", e));
                        }
                        warn!("send retry in {}ms: {}", delay.as_millis(), e);
                        tokio::time::sleep(delay).await;
                        delay = std::cmp::min(delay * 2, Duration::from_secs(2));
                    }
                }
            }
        }
        .await;

        match send_result {
            Ok(()) => {}
            Err(e) => error!("{}", e),
        }
    }

    input_handle.abort();
    client.logout().await.context("logout")?;
    info!("bye");
    Ok(())
}
