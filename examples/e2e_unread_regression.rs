//! Unread regression checker:
//! 1) create fresh user pair
//! 2) sender sends N messages to receiver
//! 3) receiver relogin sees unread == N
//! 4) receiver mark read to max_seq
//! 5) receiver relogin sees unread == 0
//! 6) generate unread again, then receiver mark read with read_seq=0
//! 7) receiver relogin still sees unread == 0 (server-side read_seq=0 fallback)
//!
//! Usage:
//! RUST_LOG=info SERVER_URL=ws://localhost:60051 \
//! cargo run -p flare-im-core-sdk --example e2e_unread_regression --features lifecycle-sqlite

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, bail};
use flare_im_core_sdk::client::{
    IMClient, LoginDbKind, SdkConfigOverlay, dev_data_dir_relative_to_cwd, parse_data_url_to_path,
    sanitize_user_id_for_dir,
};
use flare_im_core_sdk::core::SdkState;
use flare_im_core_sdk::model::Conversation;
use flare_im_core_sdk::model::conversation::ConversationType;
use tracing::info;

const DEFAULT_SENDER: &str = "12345";
const DEFAULT_RECEIVER: &str = "123456";

fn resolve_data_url(run_id: &str, user_id: &str) -> anyhow::Result<String> {
    let base: PathBuf = dev_data_dir_relative_to_cwd()
        .join("e2e_unread_regression")
        .join(run_id)
        .join(sanitize_user_id_for_dir(user_id));
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

async fn login_client(user_id: &str, ws_url: &str, run_id: &str) -> anyhow::Result<IMClient> {
    let data_url = resolve_data_url(run_id, user_id)?;
    let client = IMClient::new();
    client
        .init(
            None,
            Some(SdkConfigOverlay {
                data_url: Some(data_url),
                ws_url: Some(ws_url.to_string()),
                ..Default::default()
            }),
        )
        .await
        .with_context(|| format!("init {}", user_id))?;

    let token = IMClient::generate_test_token("", "", user_id, None)
        .with_context(|| format!("token {}", user_id))?;

    client
        .login(user_id, Some(&token), LoginDbKind::Sqlite, |_, _| {})
        .await
        .with_context(|| format!("login {}", user_id))?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while client.state() != SdkState::Ready {
        if tokio::time::Instant::now() > deadline {
            bail!("wait ready timeout for {}", user_id);
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
    }

    Ok(client)
}

async fn wait_messages_acked(
    client: &IMClient,
    client_msg_ids: &[String],
    timeout: Duration,
) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        let mut all_acked = true;
        for client_msg_id in client_msg_ids {
            let msg = client
                .message()
                .context("sender message api(wait ack)")?
                .get_raw(client_msg_id)
                .await
                .with_context(|| format!("get_raw({})", client_msg_id))?;
            let Some(msg) = msg else {
                all_acked = false;
                continue;
            };
            let acked = msg.status >= flare_proto::common::MessageStatus::Sent as i32
                && !msg.server_id.trim().is_empty()
                && msg.server_id != msg.client_msg_id;
            if !acked {
                all_acked = false;
            }
        }
        if all_acked {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
    bail!(
        "wait send ack timeout: not all messages got server_id within {:?}",
        timeout
    )
}

fn find_conversation<'a>(
    list: &'a [Conversation],
    conversation_id: &str,
) -> anyhow::Result<&'a Conversation> {
    list.iter()
        .find(|c| c.conversation_id() == conversation_id)
        .ok_or_else(|| anyhow::anyhow!("conversation not found: {}", conversation_id))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(true)
        .try_init()
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;

    let ws_url = std::env::var("SERVER_URL").unwrap_or_else(|_| "ws://localhost:60051".to_string());
    let n_msgs: usize = std::env::var("N_MSGS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);
    let n_msgs_zero: usize = std::env::var("N_MSGS_ZERO")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    let now = chrono::Utc::now().timestamp_millis();

    let sender = std::env::var("SENDER_ID").unwrap_or_else(|_| DEFAULT_SENDER.to_string());
    let receiver = std::env::var("RECEIVER_ID").unwrap_or_else(|_| DEFAULT_RECEIVER.to_string());
    let run_id = std::env::var("RUN_ID").unwrap_or_else(|_| format!("run_{}", now));

    info!(
        "unread regression start ws={} sender={} receiver={} run_id={} n_msgs={} n_msgs_zero={}",
        ws_url, sender, receiver, run_id, n_msgs, n_msgs_zero
    );

    // Step 1: sender login and create/get conversation.
    let sender_client = login_client(&sender, &ws_url, &run_id).await?;
    let conversation_id = sender_client
        .conversation()
        .context("sender conversation api")?
        .get_one(&receiver, &ConversationType::Single)
        .await
        .context("sender get_one")?
        .conversation_id()
        .to_string();

    info!("conversation_id={}", conversation_id);

    // Step 2: receiver login once, capture baseline unread for this conversation, then logout.
    let receiver_client = login_client(&receiver, &ws_url, &run_id).await?;
    let _ = receiver_client
        .conversation()
        .context("receiver conversation api")?
        .get_one(&sender, &ConversationType::Single)
        .await
        .context("receiver get_one")?;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let baseline_list = receiver_client
        .conversation()
        .context("receiver conversation api baseline")?
        .list()
        .await
        .context("receiver list baseline")?;
    let baseline_unread = find_conversation(&baseline_list, &conversation_id)
        .map(|c| c.unread_count() as usize)
        .unwrap_or(0);
    info!("receiver baseline unread: {}", baseline_unread);
    receiver_client
        .logout()
        .await
        .context("receiver first logout")?;

    // Step 3: sender sends N messages while receiver is offline.
    let mut sent_client_ids = Vec::with_capacity(n_msgs);
    for i in 0..n_msgs {
        let body = format!("unread-regression-{}-{}", now, i + 1);
        let msg = sender_client
            .message_build()
            .context("sender message_build")?
            .create_text(&conversation_id, &body, false)
            .await
            .with_context(|| format!("create_text #{}", i + 1))?;
        sent_client_ids.push(msg.client_msg_id.clone());
        let ack = sender_client
            .message()
            .context("sender message api")?
            .send(msg)
            .await
            .with_context(|| format!("send #{}", i + 1))?;
        if !ack.success {
            bail!("send #{} failed: {:?}", i + 1, ack.error_message);
        }
    }
    wait_messages_acked(&sender_client, &sent_client_ids, Duration::from_secs(12)).await?;
    info!("sender sent {} messages", n_msgs);
    sender_client.logout().await.context("sender logout")?;

    tokio::time::sleep(Duration::from_millis(800)).await;

    // Step 4: receiver relogin and verify unread delta == N.
    let receiver_client = login_client(&receiver, &ws_url, &run_id).await?;
    tokio::time::sleep(Duration::from_millis(1200)).await;

    let list = receiver_client
        .conversation()
        .context("receiver conversation api(2)")?
        .list()
        .await
        .context("receiver list(2)")?;
    let conv = find_conversation(&list, &conversation_id)?;
    let unread = conv.unread_count() as usize;
    let delta = unread.saturating_sub(baseline_unread);
    info!(
        "receiver unread after relogin: {} (baseline={}, delta={})",
        unread, baseline_unread, delta
    );
    if delta != n_msgs {
        bail!(
            "unread delta mismatch after relogin: expected +{}, got +{} (baseline={}, now={}, conversation_id={})",
            n_msgs,
            delta,
            baseline_unread,
            unread,
            conversation_id
        );
    }

    // Step 5: mark read to max seq and relogin verify unread == 0.
    let read_seq = conv.max_seq();
    receiver_client
        .mark_session_read(&conversation_id, read_seq)
        .await
        .context("mark_session_read")?;
    receiver_client
        .logout()
        .await
        .context("receiver second logout")?;

    tokio::time::sleep(Duration::from_millis(800)).await;

    let receiver_client = login_client(&receiver, &ws_url, &run_id).await?;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let list = receiver_client
        .conversation()
        .context("receiver conversation api(3)")?
        .list()
        .await
        .context("receiver list(3)")?;
    let conv = find_conversation(&list, &conversation_id)?;
    let unread_after_mark = conv.unread_count();
    info!("receiver unread after mark+relogin: {}", unread_after_mark);
    if unread_after_mark != 0 {
        bail!(
            "unread mismatch after mark read + relogin: expected 0, got {} (conversation_id={})",
            unread_after_mark,
            conversation_id
        );
    }

    // Step 6: receiver offline again; sender sends M messages to re-create unread.
    receiver_client
        .logout()
        .await
        .context("receiver third logout before read_seq=0 phase")?;
    tokio::time::sleep(Duration::from_millis(800)).await;

    let sender_client = login_client(&sender, &ws_url, &run_id).await?;
    let mut sent_client_ids_zero = Vec::with_capacity(n_msgs_zero);
    for i in 0..n_msgs_zero {
        let body = format!("unread-regression-zero-{}-{}", now, i + 1);
        let msg = sender_client
            .message_build()
            .context("sender message_build zero phase")?
            .create_text(&conversation_id, &body, false)
            .await
            .with_context(|| format!("create_text zero phase #{}", i + 1))?;
        sent_client_ids_zero.push(msg.client_msg_id.clone());
        let ack = sender_client
            .message()
            .context("sender message api zero phase")?
            .send(msg)
            .await
            .with_context(|| format!("send zero phase #{}", i + 1))?;
        if !ack.success {
            bail!("send zero phase #{} failed: {:?}", i + 1, ack.error_message);
        }
    }
    wait_messages_acked(
        &sender_client,
        &sent_client_ids_zero,
        Duration::from_secs(12),
    )
    .await?;
    sender_client
        .logout()
        .await
        .context("sender second logout")?;

    tokio::time::sleep(Duration::from_millis(800)).await;

    // Step 7: receiver relogin sees unread increased, then mark read with read_seq=0.
    let receiver_client = login_client(&receiver, &ws_url, &run_id).await?;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let list = receiver_client
        .conversation()
        .context("receiver conversation api(4)")?
        .list()
        .await
        .context("receiver list(4)")?;
    let conv = find_conversation(&list, &conversation_id)?;
    let unread_before_zero_mark = conv.unread_count() as usize;
    if unread_before_zero_mark < n_msgs_zero {
        bail!(
            "unread before read_seq=0 mark is smaller than expected: expected at least {}, got {}",
            n_msgs_zero,
            unread_before_zero_mark
        );
    }
    receiver_client
        .mark_session_read(&conversation_id, 0)
        .await
        .context("mark_session_read_zero")?;
    receiver_client
        .logout()
        .await
        .context("receiver fourth logout after read_seq=0 mark")?;

    tokio::time::sleep(Duration::from_millis(800)).await;

    let receiver_client = login_client(&receiver, &ws_url, &run_id).await?;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let list = receiver_client
        .conversation()
        .context("receiver conversation api(5)")?
        .list()
        .await
        .context("receiver list(5)")?;
    let conv = find_conversation(&list, &conversation_id)?;
    let unread_after_zero_mark = conv.unread_count();
    info!(
        "receiver unread after read_seq=0 mark+relogin: {}",
        unread_after_zero_mark
    );
    if unread_after_zero_mark != 0 {
        bail!(
            "unread mismatch after mark read(read_seq=0) + relogin: expected 0, got {} (conversation_id={})",
            unread_after_zero_mark,
            conversation_id
        );
    }

    receiver_client
        .logout()
        .await
        .context("receiver final logout")?;
    info!("PASS unread regression: all assertions satisfied");
    Ok(())
}
