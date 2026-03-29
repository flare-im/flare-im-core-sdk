//! 生命周期：透传 [IMClient]；事件仅做 SdkEvent → emit，无查库或合并。

use tauri::State;
use tokio::sync::broadcast;
use tracing::info;

use crate::model::SdkInitArgs;
use crate::state::SdkState;
use flare_im_core_sdk::client::{IMClient, LoginDbKind};
use flare_im_core_sdk::event::SdkEvent;

/// 透传 [IMClient::init]；`sdkConfig` / `dataUrl` 由前端与 core-sdk 约定。
#[tauri::command]
pub async fn sdk_init(
    state: State<'_, SdkState>,
    args: SdkInitArgs,
) -> std::result::Result<(), String> {
    state
        .set_config(args.environment, args.sdk_config)
        .await
        .map_err(super::map_sdk_err)
}

/// 透传 [IMClient::login]。`app` 由 Tauri 运行时注入（勿从前端 JSON 传参）。
#[tauri::command]
pub async fn sdk_login(
    state: State<'_, SdkState>,
    app: tauri::AppHandle,
    user_id: String,
    token: String,
) -> std::result::Result<(), String> {
    let client = state.client();
    client
        .login(
            &user_id,
            Some(&token),
            LoginDbKind::Sqlite,
            move |bus, _msg_store| {
                // 必须在 `connect` 之前订阅：否则登录后首轮同步/推送在异步任务尚未 `subscribe` 时已发出，
                // broadcast 新订户收不到历史包，WebView 会漏 `im://message*`（DB 有数据但实时事件丢、竞态下也可能与首屏交错）。
                let rx = bus.subscribe();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    forward_event_rx_to_webview(app, rx).await;
                });
            },
        )
        .await
        .map_err(super::map_sdk_err)?;

    info!(user_id = %user_id, "sdk_login ok");
    Ok(())
}

/// EventBus → `im://*`，独立任务避免阻塞登录路径。
async fn forward_event_rx_to_webview(app: tauri::AppHandle, mut rx: broadcast::Receiver<SdkEvent>) {
    use crate::convert::sdk_event_to_tauri;
    use tauri::Emitter;
    loop {
        let ev = match rx.recv().await {
            Ok(e) => e,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(dropped = n, "tauri event forward lagged, skipped events");
                continue;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        };
        if let Some((name, payload)) = sdk_event_to_tauri(&ev) {
            let _ = app.emit(&name, payload);
        }
    }
}

#[tauri::command]
pub async fn sdk_logout(state: State<'_, SdkState>) -> std::result::Result<(), String> {
    state.logout().await.map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_is_connected(state: State<'_, SdkState>) -> std::result::Result<bool, String> {
    Ok(state.client().is_connected().await)
}

#[tauri::command]
pub async fn sdk_current_user_id(
    state: State<'_, SdkState>,
) -> std::result::Result<Option<String>, String> {
    Ok(state.client().current_user_id().await)
}

#[tauri::command]
pub async fn sdk_generate_test_token(
    secret: String,
    issuer: String,
    user_id: String,
    tenant_id: Option<String>,
) -> std::result::Result<String, String> {
    IMClient::generate_test_token(
        secret.as_str(),
        issuer.as_str(),
        &user_id,
        tenant_id.as_deref(),
    )
    .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_engine_state(state: State<'_, SdkState>) -> std::result::Result<String, String> {
    let c = state.client();
    Ok(format!("{:?}", c.state()))
}

#[tauri::command]
pub async fn sdk_sync_conversation(
    state: State<'_, SdkState>,
    conversation_id: String,
) -> std::result::Result<(), String> {
    state
        .client()
        .sync_conversation(&conversation_id)
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_sync_messages(
    state: State<'_, SdkState>,
    conversation_id: String,
    last_seq: u64,
    limit: i32,
) -> std::result::Result<(), String> {
    state
        .client()
        .sync_messages(&conversation_id, last_seq, limit)
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_mark_session_read(
    state: State<'_, SdkState>,
    conversation_id: String,
    read_seq: u64,
) -> std::result::Result<(), String> {
    state
        .client()
        .mark_session_read(&conversation_id, read_seq)
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_set_conversation_input_state(
    state: State<'_, SdkState>,
    conversation_id: String,
    is_typing: bool,
) -> std::result::Result<(), String> {
    state
        .client()
        .set_conversation_input_state(&conversation_id, is_typing)
        .await
        .map_err(super::map_sdk_err)
}
