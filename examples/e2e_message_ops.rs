//! E2E message ops helper:
//! send -> add reaction -> edit -> remove reaction -> recall
//!
//! Usage (from repo root):
//! RUST_LOG=info MY_USER_ID=user-bob CHAT_WITH=user-alice SERVER_URL=ws://localhost:60051 \
//! cargo run -p flare-im-core-sdk --example e2e_message_ops --features lifecycle-sqlite

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use flare_im_core_sdk::client::{
    dev_data_dir_relative_to_cwd, parse_data_url_to_path, sanitize_user_id_for_dir, LoginDbKind,
    SdkConfigOverlay, IMClient,
};
use flare_im_core_sdk::core::SdkState;
use flare_im_core_sdk::model::conversation::ConversationType;
use tracing::info;

const DEFAULT_SELF: &str = "user-bob";
const DEFAULT_PEER: &str = "user-alice";

fn resolve_data_url(my_user_id: &str) -> anyhow::Result<String> {
    let base: PathBuf = std::env::var("FLARE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dev_data_dir_relative_to_cwd()
                .join("e2e_message_ops")
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(true)
        .try_init()
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;

    let ws_url = std::env::var("SERVER_URL").unwrap_or_else(|_| "ws://localhost:60051".to_string());
    let my_user_id = std::env::var("MY_USER_ID").unwrap_or_else(|_| DEFAULT_SELF.to_string());
    let chat_with = std::env::var("CHAT_WITH").unwrap_or_else(|_| DEFAULT_PEER.to_string());
    let data_url = resolve_data_url(&my_user_id)?;

    info!("e2e ops: self={} peer={} ws={}", my_user_id, chat_with, ws_url);

    let client = IMClient::new();
    client
        .init(
            None,
            Some(SdkConfigOverlay {
                data_url: Some(data_url),
                ws_url: Some(ws_url),
                ..Default::default()
            }),
        )
        .await
        .context("client.init")?;

    let token = IMClient::generate_test_token("", "", &my_user_id, None)
        .context("generate_test_token")?;
    client
        .login(&my_user_id, Some(&token), LoginDbKind::Sqlite, |_, _| {})
        .await
        .context("client.login")?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while client.state() != SdkState::Ready {
        if tokio::time::Instant::now() > deadline {
            anyhow::bail!("wait Ready timeout");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    info!("connected and ready");

    let conversation_id = client
        .conversation()
        .context("conversation api")?
        .get_one(&chat_with, &ConversationType::Single)
        .await
        .context("get_one conversation")?
        .conversation_id()
        .to_string();
    info!("conversation_id={}", conversation_id);
    let wait_ms = std::env::var("OPS_WAIT_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(600);
    let wait = Duration::from_millis(wait_ms);
    info!("ops wait per step: {}ms", wait_ms);

    let ts = chrono::Utc::now().timestamp_millis();
    let text = format!("e2e-ops-{}", ts);
    let msg = client
        .message_build()
        .context("message_build api")?
        .create_text(&conversation_id, &text)
        .await
        .context("create_text")?;
    let client_msg_id = msg.client_msg_id.clone();
    let ack = client
        .message()
        .context("message api")?
        .send(msg)
        .await
        .context("send text")?;
    info!(
        "sent: client_msg_id={} server_msg_id={} seq={} success={}",
        ack.client_msg_id, ack.server_msg_id, ack.seq, ack.success
    );

    tokio::time::sleep(wait).await;
    client
        .message()
        .context("message api")?
        .add_reaction(&client_msg_id, "👍")
        .await
        .context("add_reaction")?;
    info!("reaction added 👍 on {}", client_msg_id);

    tokio::time::sleep(wait).await;
    let edited_text = format!("{}-edited", text);
    client
        .message()
        .context("message api")?
        .edit_text_by_message_id(&client_msg_id, &edited_text)
        .await
        .context("edit_text_by_message_id")?;
    info!("edited message {}", client_msg_id);

    tokio::time::sleep(wait).await;
    client
        .message()
        .context("message api")?
        .remove_reaction(&client_msg_id, "👍")
        .await
        .context("remove_reaction")?;
    info!("reaction removed 👍 on {}", client_msg_id);

    tokio::time::sleep(wait).await;
    client
        .message()
        .context("message api")?
        .recall(&client_msg_id)
        .await
        .context("recall")?;
    info!("recalled message {}", client_msg_id);

    tokio::time::sleep(Duration::from_secs(2)).await;
    client.logout().await.context("logout")?;
    info!("e2e ops done");
    Ok(())
}
