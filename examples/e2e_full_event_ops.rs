//! Full E2E message/event ops helper:
//! send/reply/forward/typing/reaction/edit/pin/unpin/mark/unmark/read/recall
//!
//! Usage:
//! RUST_LOG=info OPS_WAIT_MS=1200 MY_USER_ID=user-bob CHAT_WITH=user-alice SERVER_URL=ws://localhost:60051 \
//! cargo run -p flare-im-core-sdk --example e2e_full_event_ops --features lifecycle-sqlite

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use flare_im_core_sdk::client::{
    IMClient, LoginDbKind, SdkConfigOverlay, dev_data_dir_relative_to_cwd, parse_data_url_to_path,
    sanitize_user_id_for_dir,
};
use flare_im_core_sdk::core::SdkState;
use flare_im_core_sdk::model::conversation::ConversationType;
use flare_im_core_sdk::model::message::MarkType;
use tracing::info;

const DEFAULT_SELF: &str = "user-bob";
const DEFAULT_PEER: &str = "user-alice";

fn resolve_data_url(my_user_id: &str) -> anyhow::Result<String> {
    let base: PathBuf = std::env::var("FLARE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dev_data_dir_relative_to_cwd()
                .join("e2e_full_event_ops")
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
    let wait_ms = std::env::var("OPS_WAIT_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1200);
    let wait = Duration::from_millis(wait_ms);

    info!(
        "full e2e ops: self={} peer={} ws={} wait={}ms",
        my_user_id, chat_with, ws_url, wait_ms
    );

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

    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
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

    let ts = chrono::Utc::now().timestamp_millis();
    let text = format!("e2e-full-{}", ts);
    let base_msg = client
        .message_build()
        .context("message_build api")?
        .create_text(&conversation_id, &text)
        .await
        .context("create_text")?;
    let base_msg_for_quote = base_msg.clone();
    let base_client_msg_id = base_msg.client_msg_id.clone();
    let ack = client
        .message()
        .context("message api")?
        .send(base_msg)
        .await
        .context("send base text")?;
    info!(
        "sent base text: client_msg_id={} server_msg_id={} seq={}",
        ack.client_msg_id, ack.server_msg_id, ack.seq
    );

    tokio::time::sleep(wait).await;
    let quote_msg = client
        .message_build()
        .context("message_build api")?
        .create_quote(
            &conversation_id,
            &base_client_msg_id,
            "quote-reply",
            Some(&base_msg_for_quote.sender_id),
            Some("quoted"),
            base_msg_for_quote
                .content
                .as_ref()
                .map(|elem| {
                    flare_im_core_sdk::model::content_builder::BuiltContent::new(
                        flare_proto::common::MessageType::try_from(base_msg_for_quote.message_type)
                            .unwrap_or(flare_proto::common::MessageType::Unspecified),
                        flare_im_core_sdk::model::message_elem::elem_to_message_content(elem),
                    )
                }),
        )
        .await
        .context("create_quote")?;
    let quote_ack = client
        .message()
        .context("message api")?
        .send(quote_msg)
        .await
        .context("send quote")?;
    info!(
        "sent quote reply: client_msg_id={} server_msg_id={}",
        quote_ack.client_msg_id, quote_ack.server_msg_id
    );

    tokio::time::sleep(wait).await;
    let forward_msg = client
        .message_build()
        .context("message_build api")?
        .create_forward(
            &conversation_id,
            true,
            "e2e 合并转发",
            vec![base_msg_for_quote.clone()],
        )
        .await
        .context("create_forward")?;
    let forward_ack = client
        .message()
        .context("message api")?
        .send(forward_msg)
        .await
        .context("send forward")?;
    info!(
        "sent forward: client_msg_id={} server_msg_id={}",
        forward_ack.client_msg_id, forward_ack.server_msg_id
    );

    tokio::time::sleep(wait).await;
    client
        .message()
        .context("message api")?
        .typing(&conversation_id, true)
        .await
        .context("typing true")?;
    info!("typing=true sent");

    tokio::time::sleep(wait).await;
    client
        .message()
        .context("message api")?
        .typing(&conversation_id, false)
        .await
        .context("typing false")?;
    info!("typing=false sent");

    tokio::time::sleep(wait).await;
    client
        .message()
        .context("message api")?
        .add_reaction(&base_client_msg_id, "👍")
        .await
        .context("add_reaction")?;
    info!("reaction added");

    tokio::time::sleep(wait).await;
    let edited_text = format!("{}-edited", text);
    client
        .message()
        .context("message api")?
        .edit_text_by_message_id(&base_client_msg_id, &edited_text)
        .await
        .context("edit_text_by_message_id")?;
    info!("edited");

    tokio::time::sleep(wait).await;
    client
        .message()
        .context("message api")?
        .pin_by_message_id(&base_client_msg_id)
        .await
        .context("pin_by_message_id")?;
    info!("pinned");

    tokio::time::sleep(wait).await;
    client
        .message()
        .context("message api")?
        .unpin_by_message_id(&base_client_msg_id)
        .await
        .context("unpin_by_message_id")?;
    info!("unpinned");

    tokio::time::sleep(wait).await;
    client
        .message()
        .context("message api")?
        .mark_by_message_id(&base_client_msg_id, MarkType::Todo, "#FFA500")
        .await
        .context("mark_by_message_id")?;
    info!("marked");

    tokio::time::sleep(wait).await;
    client
        .message()
        .context("message api")?
        .unmark_by_message_id(&base_client_msg_id, MarkType::Todo)
        .await
        .context("unmark_by_message_id")?;
    info!("unmarked");

    tokio::time::sleep(wait).await;
    client
        .message()
        .context("message api")?
        .mark_read_with_ids(&conversation_id, vec![ack.server_msg_id.clone()], ack.seq)
        .await
        .context("mark_read_with_ids")?;
    info!("mark_read_with_ids sent");

    tokio::time::sleep(wait).await;
    client
        .message()
        .context("message api")?
        .remove_reaction(&base_client_msg_id, "👍")
        .await
        .context("remove_reaction")?;
    info!("reaction removed");

    tokio::time::sleep(wait).await;
    client
        .message()
        .context("message api")?
        .recall(&base_client_msg_id)
        .await
        .context("recall")?;
    info!("recalled");

    tokio::time::sleep(Duration::from_secs(2)).await;
    client.logout().await.context("logout")?;
    info!("full e2e ops done");
    Ok(())
}
