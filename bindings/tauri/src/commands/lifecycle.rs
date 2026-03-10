//! 生命周期命令：init / connect / disconnect / token

use std::path::PathBuf;
use tauri::Manager;
use tauri::State;
use tracing::info;

use crate::model::{SdkConfigOptions, SdkInitArgs};
use crate::state::SdkState;
use flare_im_core_sdk::client::im_client::IMClient;
use flare_im_core_sdk::store::StoreProvider;
use flare_im_core_sdk::util::generate_test_token;
use flare_im_core_sdk::client::config::SdkConfig;
use flare_im_core_sdk_storage_sqlite::create_stores;

/// 初始化 SDK 配置（可选：environment、sdkConfig）。
/// 前端传对象 { environment?, sdkConfig? }；ws_url 在 sdkConfig.wsUrl；不传 db_path，按 environment 解析数据目录。
#[tauri::command]
pub async fn sdk_init(
    state: State<'_, SdkState>,
    args: SdkInitArgs,
) -> std::result::Result<(), String> {
    state.set_config(args.environment, args.sdk_config).await;
    Ok(())
}

/// 从 sdk_config 或环境变量得到 ws_url 默认值
fn default_ws_url(sdk_config: Option<&SdkConfigOptions>) -> String {
    sdk_config
        .and_then(|c| c.ws_url.as_deref())
        .map(String::from)
        .or_else(|| std::env::var("FLARE_IM_SERVER_URL").ok())
        .unwrap_or_else(|| "ws://localhost:60051".to_string())
}

/// 由 ws_url 与 sdk_config 合并得到 SdkConfig（ws_url 可来自 sdk_config 或默认）
fn build_sdk_config(ws_url: &str, overlay: Option<&SdkConfigOptions>) -> SdkConfig {
    let mut config = SdkConfig::new(ws_url);
    if let Some(o) = overlay {
        if let Some(u) = &o.ws_url {
            config.ws_url = Some(u.clone());
        }
        if o.quic_url.is_some() {
            config.quic_url = o.quic_url.clone();
        }
        if o.http_url.is_some() {
            config.http_url = o.http_url.clone();
        }
        if o.connect_timeout_secs.is_some() {
            config.connect_timeout_secs = o.connect_timeout_secs;
        }
        if o.reconnect_interval_secs.is_some() {
            config.reconnect_interval_secs = o.reconnect_interval_secs;
        }
        if o.max_reconnect_attempts.is_some() {
            config.max_reconnect_attempts = o.max_reconnect_attempts;
        }
        if o.sync_batch_size.is_some() {
            config.sync_batch_size = o.sync_batch_size;
        }
        if o.ack_timeout_secs.is_some() {
            config.ack_timeout_secs = o.ack_timeout_secs;
        }
        if o.ack_max_retries.is_some() {
            config.ack_max_retries = o.ack_max_retries;
        }
        if let Some(b) = o.enable_metrics {
            config.enable_metrics = b;
        }
    }
    config
}

/// 开发环境数据目录：temp-data（cwd 为 examples/tauri 时 ../temp-data；为 src-tauri 时 ../../temp-data，均指向 flare-im-core-sdk/temp-data）
fn dev_data_dir() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let base = if cwd.file_name().map(|n| n == "src-tauri").unwrap_or(false) {
        cwd.join("..").join("..")
    } else {
        cwd.join("..")
    };
    let temp_data = base.join("temp-data");
    let _ = std::fs::create_dir_all(&temp_data);
    temp_data
}

/// 按环境解析数据库路径：development → temp-data；production → Tauri 应用目录（environment 大小写不敏感）
fn resolve_db_path(app: &tauri::AppHandle, environment: Option<&str>) -> PathBuf {
    let is_dev = environment
        .map(|s| s.to_lowercase() == "development")
        .unwrap_or(false);
    if is_dev {
        return dev_data_dir().join("flare_im_sdk.db");
    }
    let path_resolver = app.path();
    for dir_result in [
        path_resolver.app_data_dir(),
        path_resolver.app_cache_dir(),
        path_resolver.app_local_data_dir(),
    ] {
        if let Ok(dir) = dir_result {
            if std::fs::create_dir_all(&dir).is_ok() {
                return dir.join("flare_im_sdk.db");
            }
        }
    }
    let temp_subdir = std::env::temp_dir().join("flare_im_sdk");
    let _ = std::fs::create_dir_all(&temp_subdir);
    temp_subdir.join("flare_im_sdk.db")
}

/// 登录：创建 SQLite 存储、构建 IMClient、使用传入的 userId/token 连接并启动事件转发。对外仅暴露此命令，前端传 { userId, token }（camelCase）。
#[tauri::command]
pub async fn sdk_login(
    state: State<'_, SdkState>,
    app: tauri::AppHandle,
    user_id: String,
    mut token: String,
) -> std::result::Result<(), String> {
    state.set_app_handle(app.clone()).await;

    let (environment, sdk_config) = state.config().await;
    info!(environment = ?environment, "sdk_login using config");
    let ws_url = default_ws_url(sdk_config.as_ref());
    let db_path = resolve_db_path(&app, environment.as_deref());
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let path_str = db_path.to_string_lossy().replace('\\', "/");
    let database_url = if path_str.starts_with('/') {
        format!("sqlite://{}?mode=rwc", path_str)
    } else {
        format!("sqlite:///{}?mode=rwc", path_str)
    };
    info!(db = %db_path.display(), "Opening SQLite store");

    let (msg_store, conv_store, cursor_store) = create_stores(&database_url)
        .await
        .map_err(|e| format!("create_stores failed: {}", e))?;

    let stores = StoreProvider {
        messages: msg_store,
        conversations: conv_store,
        cursors: cursor_store,
    };

    if token.trim().is_empty() {
        token = resolve_token_for_connect(&user_id)?;
    }

    let config = build_sdk_config(ws_url.as_str(), sdk_config.as_ref());
    let mut client = IMClient::builder().config(config).stores(stores).build();

    client
        .connect(&user_id, &token)
        .await
        .map_err(|e: flare_im_core_sdk::error::SdkError| format!("connect failed: {}", e))?;

    state.set_client(client).await;
    state.set_current_user(Some(user_id.clone())).await;

    info!(user_id = %user_id, "SDK logged in");
    spawn_event_forwarder(state.inner.clone(), app);
    Ok(())
}

/// 断开并清理
#[tauri::command]
pub async fn sdk_logout(state: State<'_, SdkState>) -> std::result::Result<(), String> {
    if let Some(mut client) = state.take_client().await {
        let _ = client.disconnect().await;
    }
    state.set_current_user(None).await;
    info!("SDK logged out");
    Ok(())
}

/// 生成测试用 JWT Token，供前端传入 secret / issuer / user_id / tenant_id
#[tauri::command]
pub async fn sdk_generate_test_token(
    secret: String,
    issuer: String,
    user_id: String,
    tenant_id: Option<String>,
) -> std::result::Result<String, String> {
    let secret = if secret.is_empty() { "insecure-secret" } else { secret.as_str() };
    let issuer = if issuer.is_empty() { "flare-im-core" } else { issuer.as_str() };
    let tenant = tenant_id.as_deref().unwrap_or("default");
    let token = generate_test_token(secret, issuer, &user_id, 3600, None, Some(tenant))
        .map_err(|e: flare_im_core_sdk::error::SdkError| e.to_string())?;
    Ok(token)
}

fn resolve_token_for_connect(user_id: &str) -> std::result::Result<String, String> {
    if let Ok(token) = std::env::var("FLARE_IM_TOKEN") {
        let t = token.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    if let Ok(token) = std::env::var("TOKEN") {
        let t = token.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    generate_test_token(
        "insecure-secret",
        "flare-im-core",
        user_id,
        3600,
        None,
        Some("default"),
    )
    .map_err(|e: flare_im_core_sdk::error::SdkError| e.to_string())
}

/// 统一事件转发：SdkEvent → (im://*, payload)，同步开始/完成/失败与会话、消息均由 SDK 回调驱动
fn spawn_event_forwarder(
    state_inner: std::sync::Arc<tokio::sync::RwLock<crate::state::SdkStateInner>>,
    app: tauri::AppHandle,
) {
    use flare_im_core_sdk::event::{MessageEvent, SdkEvent};
    use crate::convert::{message_to_model, sdk_event_to_tauri};
    use crate::model::EventPayload;
    use tauri::Emitter;

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("event forwarder runtime");
        rt.block_on(async move {
            let (bus, msg_store) = {
                let g = state_inner.read().await;
                let client = g.client.as_ref().expect("client set");
                (client.bus().clone(), client.engine().stores().messages.clone())
            };
            let mut rx = bus.subscribe();

            while let Some(ev) = rx.recv().await {
                let app = app.clone();

                if let Some((name, payload)) = sdk_event_to_tauri(ev.as_ref()) {
                    let _ = app.emit(&name, payload);
                    if name == "im://conversations_synced" {
                        let _ = app.emit("im://unread", ());
                    }
                    continue;
                }

                if let SdkEvent::Message(MessageEvent::SendAck { ack }) = ev.as_ref() {
                    let id = if ack.server_msg_id.is_empty() {
                        &ack.client_msg_id
                    } else {
                        &ack.server_msg_id
                    };
                    if let Ok(Some(m)) = msg_store.get(id).await {
                        let _ = app.emit("im://message", EventPayload::Message(message_to_model(&m)));
                    }
                }
            }
        });
    });
}
