//! 生命周期：透传 [IMClient]；事件仅做 SdkEvent → emit，无查库或合并。

use tauri::State;
use tokio::sync::broadcast;
use tracing::info;

use crate::model::RtcIceConfigSnapshotPayload;
use crate::model::SdkInitArgs;
use crate::state::SdkState;
use flare_im_core_sdk::client::{IMClient, LoginDbKind};
use flare_im_core_sdk::event::SdkEvent;

fn env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(v) => {
            let s = v.trim().to_ascii_lowercase();
            !(s == "0" || s == "false" || s == "off" || s == "no" || s.is_empty())
        }
        Err(_) => default,
    }
}

fn env_string(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn parse_csv_urls(raw: Option<String>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn parse_env_file_vars(path: &std::path::Path) -> std::collections::HashMap<String, String> {
    let mut vars = std::collections::HashMap::new();
    let Ok(content) = std::fs::read_to_string(path) else {
        return vars;
    };
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((k, v)) = trimmed.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let val = v.trim().trim_matches('"').trim_matches('\'');
        if key.is_empty() || val.is_empty() {
            continue;
        }
        vars.insert(key.to_string(), val.to_string());
    }
    vars
}

fn try_load_default_rtc_env_file() -> std::collections::HashMap<String, String> {
    let rel =
        std::path::Path::new("flare-plugin/flare-strom-sfu/docs/coturn/strom_grpc_core_proxy.env");
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    let mut push_from_base = |base: &std::path::Path| {
        candidates.push(base.join(rel));
    };
    if let Ok(cwd) = std::env::current_dir() {
        push_from_base(&cwd);
        for anc in cwd.ancestors() {
            push_from_base(anc);
        }
    }
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    push_from_base(&manifest_dir);
    for anc in manifest_dir.ancestors() {
        push_from_base(anc);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            push_from_base(exe_dir);
            for anc in exe_dir.ancestors() {
                push_from_base(anc);
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    for p in candidates {
        let vars = parse_env_file_vars(&p);
        if !vars.is_empty() {
            return vars;
        }
    }
    std::collections::HashMap::new()
}

fn var_with_file_fallback(
    env_name: &str,
    file_vars: &std::collections::HashMap<String, String>,
) -> Option<String> {
    env_string(env_name).or_else(|| file_vars.get(env_name).cloned())
}

fn bool_with_file_fallback(
    env_name: &str,
    file_vars: &std::collections::HashMap<String, String>,
    default: bool,
) -> bool {
    if std::env::var(env_name).is_ok() {
        return env_bool(env_name, default);
    }
    if let Some(v) = file_vars.get(env_name) {
        let s = v.trim().to_ascii_lowercase();
        return !(s == "0" || s == "false" || s == "off" || s == "no" || s.is_empty());
    }
    default
}

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
    let apis = client
        .login(
            &user_id,
            Some(&token),
            LoginDbKind::Sqlite,
            move |bus, _msg_store| {
                let rx = bus.subscribe();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    forward_event_rx_to_webview(app, rx).await;
                });
            },
        )
        .await
        .map_err(super::map_sdk_err)?;
    state.install_session(apis).await;

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
pub async fn sdk_rtc_ice_config_snapshot()
-> std::result::Result<Option<RtcIceConfigSnapshotPayload>, String> {
    let file_vars = try_load_default_rtc_env_file();
    let source = if file_vars.is_empty() {
        "env".to_string()
    } else {
        "env_or_default_file".to_string()
    };

    let stun_enabled = bool_with_file_fallback("RTC_ICE_STUN_ENABLED", &file_vars, true);
    let turn_enabled = bool_with_file_fallback("RTC_ICE_TURN_ENABLED", &file_vars, false);
    let stun_urls = parse_csv_urls(var_with_file_fallback("RTC_ICE_STUN_URLS", &file_vars));
    let turn_urls = parse_csv_urls(var_with_file_fallback("RTC_ICE_TURN_URLS", &file_vars));
    let turn_username = var_with_file_fallback("RTC_ICE_TURN_USERNAME", &file_vars);
    let turn_credential = var_with_file_fallback("RTC_ICE_TURN_CREDENTIAL", &file_vars);
    let default_ice_tf = var_with_file_fallback("RTC_ICE_DEFAULT_TRANSPORT", &file_vars)
        .unwrap_or_else(|| "all".to_string());

    let mut rows: Vec<serde_json::Value> = Vec::new();
    if stun_enabled {
        for u in stun_urls {
            rows.push(serde_json::json!({ "urls": u }));
        }
    }
    if turn_enabled {
        for u in turn_urls {
            rows.push(serde_json::json!({
                "urls": u,
                "username": turn_username.clone().unwrap_or_default(),
                "credential": turn_credential.clone().unwrap_or_default(),
            }));
        }
    }

    if rows.is_empty() {
        return Ok(None);
    }
    Ok(Some(RtcIceConfigSnapshotPayload {
        source,
        turn_enabled,
        default_ice_tf,
        ice_servers: serde_json::Value::Array(rows),
    }))
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
