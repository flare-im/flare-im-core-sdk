//! 生命周期：透传 [IMClient]；事件仅做 SdkEvent → emit，无查库或合并。

use tauri::{Emitter, State};
use tokio::sync::oneshot;
use tracing::{info, warn};

use crate::model::RtcIceConfigSnapshotPayload;
use crate::state::SdkState;
use flare_im_core_sdk::client::{LoginDbKind, SdkConfigOverlay};
use flare_im_core_sdk::event::{SdkEvent, SharedEventReceiver};
use flare_im_core_sdk_bindings_runtime::{SessionTaskSlot, platform_event_bridge_resync_marker};

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
    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        push_from_base(exe_dir);
        for anc in exe_dir.ancestors() {
            push_from_base(anc);
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

#[tauri::command(rename_all = "camelCase")]
pub async fn sdk_ffi_contract_version() -> std::result::Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "version": crate::BINDING_CONTRACT_VERSION
    }))
}

/// 透传 [IMClient::init]；`sdkConfig` / `dataUrl` 由前端与 core-sdk 约定。
#[tauri::command(rename_all = "camelCase")]
pub async fn sdk_init(
    state: State<'_, SdkState>,
    environment: Option<String>,
    sdk_config: Option<SdkConfigOverlay>,
) -> std::result::Result<(), String> {
    state
        .set_config(environment, sdk_config)
        .await
        .map_err(super::map_sdk_err)
}

/// 透传 [IMClient::login]。`app` 由 Tauri 运行时注入（勿从前端 JSON 传参）。
#[tauri::command(rename_all = "camelCase")]
pub async fn sdk_login(
    state: State<'_, SdkState>,
    app: tauri::AppHandle,
    user_id: String,
    token: String,
) -> std::result::Result<(), String> {
    let client = state.client();
    let event_bridge = state.event_bridge();
    event_bridge.clear();
    let event_bridge_for_login = event_bridge.clone();
    let login_result = client
        .login(
            &user_id,
            Some(&token),
            LoginDbKind::Sqlite,
            move |bus, _msg_store| {
                let rx = bus.subscribe_shared_raw();
                let app = app.clone();
                spawn_event_bridge(app, rx, event_bridge_for_login.clone());
            },
        )
        .await;
    let apis = match login_result {
        Ok(apis) => apis,
        Err(err) => {
            event_bridge.clear();
            return Err(super::map_sdk_err(err));
        }
    };
    state.install_session(apis).await;

    info!(user_id = %user_id, "sdk_login ok");
    Ok(())
}

/// 透传 [IMClient::prepare]：开库 + 建引擎（不连网），把开库 / 迁移移出登录关键路径。
///
/// 与 [`sdk_connect`] 配合实现「初始化前置、登录只做网络」。
#[tauri::command(rename_all = "camelCase")]
pub async fn sdk_prepare(
    state: State<'_, SdkState>,
    app: tauri::AppHandle,
    user_id: String,
) -> std::result::Result<(), String> {
    let client = state.client();
    let event_bridge = state.event_bridge();
    event_bridge.clear();
    client
        .prepare(&user_id, LoginDbKind::Sqlite)
        .await
        .map_err(super::map_sdk_err)?;
    // 预热后订阅事件总线 → webview（等价 login 闭包在 connect 前所做）。
    let bus = client.bus().await.map_err(super::map_sdk_err)?;
    let rx = bus.subscribe_shared_raw();
    spawn_event_bridge(app, rx, event_bridge);
    info!(user_id = %user_id, "sdk_prepare ok");
    Ok(())
}

/// 透传 [IMClient::connect]：连接已预热引擎 + 首次同步（登录的网络半段）。
#[tauri::command(rename_all = "camelCase")]
pub async fn sdk_connect(
    state: State<'_, SdkState>,
    user_id: String,
    token: String,
) -> std::result::Result<(), String> {
    let client = state.client();
    let apis = client
        .connect(&user_id, Some(&token))
        .await
        .map_err(super::map_sdk_err)?;
    state.install_session(apis).await;
    info!(user_id = %user_id, "sdk_connect ok");
    Ok(())
}

/// EventBus → `im://*`，独立任务避免阻塞登录路径。
fn spawn_event_bridge(app: tauri::AppHandle, rx: SharedEventReceiver, bridge: SessionTaskSlot) {
    let (cancel_tx, cancel_rx) = oneshot::channel();
    bridge.replace(move || {
        let _ = cancel_tx.send(());
    });
    tauri::async_runtime::spawn(async move {
        forward_event_rx_to_webview(app, rx, cancel_rx).await;
    });
}

async fn forward_event_rx_to_webview(
    app: tauri::AppHandle,
    mut rx: SharedEventReceiver,
    mut cancel_rx: oneshot::Receiver<()>,
) {
    let mut missed_events = 0u64;
    loop {
        tokio::select! {
            _ = &mut cancel_rx => break,
            result = rx.recv() => {
                let ev = match result {
                    Ok(e) => e,
                    Err(_) => break,
                };
                if missed_events > 0 {
                    let marker = platform_event_bridge_resync_marker(missed_events);
                    if emit_event_to_webview(&app, &marker) {
                        missed_events = 0;
                    } else {
                        missed_events = missed_events.saturating_add(1);
                        warn!(
                            missed_events,
                            "failed to emit resync marker to Tauri webview"
                        );
                        continue;
                    }
                }

                if !emit_event_to_webview(&app, ev.event()) {
                    missed_events = missed_events.saturating_add(1);
                    warn!(missed_events, "failed to emit SDK event to Tauri webview");
                }
            }
        }
    }
}

fn emit_event_to_webview<R: tauri::Runtime>(app: &tauri::AppHandle<R>, ev: &SdkEvent) -> bool {
    let Some((name, payload)) = crate::convert::sdk_event_to_tauri(ev) else {
        return true;
    };
    app.emit(&name, payload).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_event_bridge_resync_marker_maps_to_tauri_event() {
        let event = platform_event_bridge_resync_marker(3);
        let (channel, payload) =
            crate::convert::sdk_event_to_tauri(&event).expect("resync marker should convert");

        assert_eq!(channel, "im://resync_needed");
        assert_eq!(
            payload.get("scope").and_then(|v| v.as_str()),
            Some("platform_event_bridge")
        );
        assert_eq!(
            payload.get("reason").and_then(|v| v.as_str()),
            Some("platform_event_bridge_lagged")
        );
        assert_eq!(
            payload.get("droppedEvents").and_then(|v| v.as_u64()),
            Some(3)
        );
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn sdk_logout(state: State<'_, SdkState>) -> std::result::Result<(), String> {
    state.logout().await.map_err(super::map_sdk_err)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn sdk_update_access_token(
    state: State<'_, SdkState>,
    access_token: String,
    tenant_id: Option<String>,
) -> std::result::Result<(), String> {
    state
        .client()
        .update_access_token(access_token, tenant_id.as_deref())
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn sdk_is_connected(state: State<'_, SdkState>) -> std::result::Result<bool, String> {
    Ok(state.client().is_connected().await)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn sdk_current_user_id(
    state: State<'_, SdkState>,
) -> std::result::Result<Option<String>, String> {
    Ok(state.client().current_user_id().await)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn sdk_engine_state(state: State<'_, SdkState>) -> std::result::Result<String, String> {
    let c = state.client();
    Ok(format!("{:?}", c.state()))
}

#[tauri::command(rename_all = "camelCase")]
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

#[tauri::command(rename_all = "camelCase")]
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

#[tauri::command(rename_all = "camelCase")]
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

#[tauri::command(rename_all = "camelCase")]
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
