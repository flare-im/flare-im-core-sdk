//! Observe full event set for E2E verification.
//!
//! Usage:
//! RUST_LOG=info MY_USER_ID=user-alice CHAT_WITH=user-bob SERVER_URL=ws://localhost:60051 \
//! cargo run -p flare-im-core-sdk --example e2e_full_event_observer --features lifecycle-sqlite

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::Context;
use flare_im_core_sdk::client::{
    IMClient, LoginDbKind, SdkConfigOverlay, dev_data_dir_relative_to_cwd, parse_data_url_to_path,
    sanitize_user_id_for_dir,
};
use flare_im_core_sdk::core::SdkState;
use flare_im_core_sdk::event::{MessageEvent, SdkEvent};
use flare_im_core_sdk::model::conversation::ConversationType;
use tracing::info;

const DEFAULT_SELF: &str = "user-alice";
const DEFAULT_PEER: &str = "user-bob";

fn resolve_data_url(my_user_id: &str) -> anyhow::Result<String> {
    let base: PathBuf = std::env::var("FLARE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dev_data_dir_relative_to_cwd()
                .join("e2e_full_event_observer")
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

    info!(
        "full observer start: self={} peer={} ws={}",
        my_user_id, chat_with, ws_url
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
    info!("observer connected and ready");

    let conversation_id = client
        .conversation()
        .context("conversation api")?
        .get_one(&chat_with, &ConversationType::Single)
        .await
        .context("get_one conversation")?
        .conversation_id()
        .to_string();
    info!("observer conversation_id={}", conversation_id);

    let seen_edited = Arc::new(AtomicBool::new(false));
    let seen_recalled = Arc::new(AtomicBool::new(false));
    let seen_reaction = Arc::new(AtomicBool::new(false));
    let seen_typing = Arc::new(AtomicBool::new(false));
    let seen_read = Arc::new(AtomicBool::new(false));
    let seen_pin = Arc::new(AtomicBool::new(false));
    let seen_unpin = Arc::new(AtomicBool::new(false));
    let seen_mark = Arc::new(AtomicBool::new(false));
    let seen_unmark = Arc::new(AtomicBool::new(false));
    let received_count = Arc::new(AtomicUsize::new(0));

    let e1 = Arc::clone(&seen_edited);
    let e2 = Arc::clone(&seen_recalled);
    let e3 = Arc::clone(&seen_reaction);
    let e4 = Arc::clone(&seen_typing);
    let e5 = Arc::clone(&seen_read);
    let e6 = Arc::clone(&seen_pin);
    let e7 = Arc::clone(&seen_unpin);
    let e8 = Arc::clone(&seen_mark);
    let e9 = Arc::clone(&seen_unmark);
    let recv = Arc::clone(&received_count);
    client
        .on_any(move |ev| {
            if let SdkEvent::Message(me) = ev.as_ref() {
                match me {
                    MessageEvent::Received { .. } => {
                        recv.fetch_add(1, Ordering::Relaxed);
                    }
                    MessageEvent::ReceivedBatch { messages } => {
                        recv.fetch_add(messages.len(), Ordering::Relaxed);
                    }
                    MessageEvent::Edited { .. } => {
                        e1.store(true, Ordering::Relaxed);
                        info!("OBS Edited");
                    }
                    MessageEvent::Recalled { .. } => {
                        e2.store(true, Ordering::Relaxed);
                        info!("OBS Recalled");
                    }
                    MessageEvent::ReactionChanged { .. } => {
                        e3.store(true, Ordering::Relaxed);
                        info!("OBS ReactionChanged");
                    }
                    MessageEvent::Typing { .. } => {
                        e4.store(true, Ordering::Relaxed);
                        info!("OBS Typing");
                    }
                    MessageEvent::ReadReceipt { .. } => {
                        e5.store(true, Ordering::Relaxed);
                        info!("OBS ReadReceipt");
                    }
                    MessageEvent::Pinned { .. } => {
                        e6.store(true, Ordering::Relaxed);
                        info!("OBS Pinned");
                    }
                    MessageEvent::Unpinned { .. } => {
                        e7.store(true, Ordering::Relaxed);
                        info!("OBS Unpinned");
                    }
                    MessageEvent::Marked { .. } => {
                        e8.store(true, Ordering::Relaxed);
                        info!("OBS Marked");
                    }
                    MessageEvent::Unmarked { .. } => {
                        e9.store(true, Ordering::Relaxed);
                        info!("OBS Unmarked");
                    }
                    _ => {}
                }
            }
        })
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    info!("observer waiting for full event set up to 120s ...");
    let start = tokio::time::Instant::now();
    loop {
        if seen_edited.load(Ordering::Relaxed)
            && seen_recalled.load(Ordering::Relaxed)
            && seen_reaction.load(Ordering::Relaxed)
            && seen_typing.load(Ordering::Relaxed)
            && seen_read.load(Ordering::Relaxed)
            && seen_pin.load(Ordering::Relaxed)
            && seen_unpin.load(Ordering::Relaxed)
            && seen_mark.load(Ordering::Relaxed)
            && seen_unmark.load(Ordering::Relaxed)
            && received_count.load(Ordering::Relaxed) >= 3
        {
            info!(
                "observer result: PASS recv={} edited/recalled/reaction/typing/read/pin/unpin/mark/unmark all seen",
                received_count.load(Ordering::Relaxed)
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(120) {
            info!(
                "observer result: TIMEOUT recv={} edited={} recalled={} reaction={} typing={} read={} pin={} unpin={} mark={} unmark={}",
                received_count.load(Ordering::Relaxed),
                seen_edited.load(Ordering::Relaxed),
                seen_recalled.load(Ordering::Relaxed),
                seen_reaction.load(Ordering::Relaxed),
                seen_typing.load(Ordering::Relaxed),
                seen_read.load(Ordering::Relaxed),
                seen_pin.load(Ordering::Relaxed),
                seen_unpin.load(Ordering::Relaxed),
                seen_mark.load(Ordering::Relaxed),
                seen_unmark.load(Ordering::Relaxed),
            );
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    client.logout().await.context("logout")?;
    info!("observer done");
    Ok(())
}

