//! Lifecycle Commands
//!
//! Handles SDK initialization, authentication, and connection management.

use std::sync::Arc;
use tauri::{AppHandle, State, Emitter, Manager};
use crate::state::SdkState;
use crate::error::CommandError;
use flare_im_core_sdk::{
    config::{
        TransportProtocol, NetworkConfig, StorageConfig, 
        SyncConfig, MediaConfig, LogConfig, AdvancedConfig
    },
    interface::facade::ImCoreSdk,
    domain::repository::{EventStore, MessageRepository, ConversationRepository},
};
use flare_im_core_sdk_storage_sqlite::create_storage;
use std::path::PathBuf;

#[derive(Debug, serde::Deserialize)]
pub struct InitOptions {
    // Shortcuts / Legacy
    pub server_url: Option<String>,
    pub user_id: Option<String>,
    
    // Detailed Configuration Overrides
    pub network: Option<NetworkConfig>,
    pub storage: Option<StorageConfig>,
    pub sync: Option<SyncConfig>,
    pub media: Option<MediaConfig>,
    pub log: Option<LogConfig>,
    pub advanced: Option<AdvancedConfig>,
}

/// Initialize the IM SDK
///
/// This must be called before any other command.
/// It sets up the local database, network configuration, and core services.
#[tauri::command]
pub async fn sdk_init(
    app: AppHandle,
    state: State<'_, SdkState>,
    options: Option<InitOptions>,
) -> Result<(), CommandError> {
    if state.is_initialized().await {
        return Ok(());
    }

    let options = options.unwrap_or(InitOptions {
        server_url: None,
        user_id: None,
        network: None,
        storage: None,
        sync: None,
        media: None,
        log: None,
        advanced: None,
    });

    // 1. Determine App Data Directory
    // Default to "." if resolution fails (though it shouldn't in a valid Tauri app)
    let mut app_dir = app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("."));
    
    // Ensure the directory exists
    if let Err(e) = std::fs::create_dir_all(&app_dir) {
        eprintln!("⚠️ 警告: 无法创建应用数据目录 {}: {}", app_dir.display(), e);
        // 如果无法创建标准应用数据目录，使用临时目录
        let temp_dir = std::env::temp_dir().join("flare-im-tauri");
        std::fs::create_dir_all(&temp_dir).map_err(|create_err| {
            format!("无法创建标准目录或临时目录 - 主目录错误: {}, 临时目录错误: {}", e, create_err)
        })?;
        app_dir = temp_dir;
    }

    // 2. Build Configuration
    // Let's construct the SdkConfig manually to support full flexibility
    let mut config = flare_im_core_sdk::config::SdkConfig::default();

    // Store network config to avoid multiple moves
    let network_config_option = options.network.clone();

    // Apply Network Overrides
    if let Some(network) = network_config_option {
        config.network = network;
    } else if let Some(url) = options.server_url {
        // Fallback to simple server_url if network config is missing
        let (ws_url, quic_url) = parse_server_url(&url);
        config.network.websocket_url = Some(ws_url);
        config.network.quic_url = Some(quic_url);
        // Enable protocol race by default for simple config
        config.network.protocol_race = Some(flare_im_core_sdk::config::ProtocolRaceConfig {
            protocols: vec![TransportProtocol::WebSocket, TransportProtocol::Quic],
            timeout_secs: 5,
        });
    } else {
        // Env var fallback
        let url = std::env::var("FLARE_IM_SERVER_URL")
            .unwrap_or_else(|_| "ws://localhost:60051".to_string());
        let (ws_url, quic_url) = parse_server_url(&url);
        config.network.websocket_url = Some(ws_url);
        config.network.quic_url = Some(quic_url);
    }

    // --- Storage Configuration ---
    if let Some(storage) = options.storage {
        config.storage = storage;
    }
    
    // Auto-resolve storage path if not set
    if config.storage.path.is_none() {
        config.storage.path = Some(app_dir.clone());
    }
    
    // Auto-resolve db filename if user_id is provided and filename is default
    if let Some(uid) = &options.user_id {
        if config.storage.db_filename == "flare_im.db" {
            config.storage.db_filename = format!("flare_{}.db", uid);
        }
    }

    // --- Media Configuration ---
    if let Some(media) = options.media {
        config.media = media;
    }
    
    // Auto-resolve media cache path if not set
    if config.media.cache_path.is_none() {
        let cache_path = app_dir.join("media_cache");
        if !cache_path.exists() {
            let _ = std::fs::create_dir_all(&cache_path);
        }
        config.media.cache_path = Some(cache_path);
    }

    // --- Other Configurations ---
    if let Some(sync) = options.sync {
        config.sync = sync;
    }
    if let Some(log) = options.log {
        config.log = log;
    }
    if let Some(advanced) = options.advanced {
        config.advanced = advanced;
    }

    // 3. Initialize Infrastructure (Storage)
    // We need the full DB URL for sqlx
    let db_path = config.storage.path.as_ref().unwrap().join(&config.storage.db_filename);
    let db_url = format!("sqlite:{}?mode=rwc", db_path.to_string_lossy());
    
    app.emit("sdk_log", format!("Initializing SDK with DB: {}", db_url)).map_err(|e| CommandError::from(e.to_string()))?;

    let (event_store, message_repo, conversation_repo) = create_storage(&db_url)
        .await
        .map_err(CommandError::from)?;

    // Cast to Trait Objects
    let event_store: Arc<dyn EventStore> = event_store;
    let message_repo: Arc<dyn MessageRepository> = message_repo;
    let conversation_repo: Arc<dyn ConversationRepository> = conversation_repo;

    // 4. Create SDK Instance
    let sdk = ImCoreSdk::new(
        config,
        event_store,
        message_repo,
        conversation_repo,
    ).await.map_err(CommandError::from)?;

    let sdk = Arc::new(sdk);

    // 5. Register Event Subscribers (Forward SDK events to Tauri)
    crate::register_event_subscribers(&sdk, app.clone())
        .await
        .map_err(CommandError::from)?;

    // 6. Update State
    state.set_sdk(sdk).await;
    
    app.emit("sdk_log", "SDK Initialized Successfully").map_err(|e| CommandError::from(e.to_string()))?;

    Ok(())
}

/// Login to the IM Server
#[tauri::command]
pub async fn sdk_login(
    state: State<'_, SdkState>,
    user_id: String,
    token: String,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    // Domain Command: Login
    sdk.login(user_id, token)
        .await
        .map_err(CommandError::from)?;
        
    Ok(())
}

/// Connect to the network
#[tauri::command]
pub async fn sdk_connect(state: State<'_, SdkState>) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    // Domain Command: Connect
    sdk.connect()
        .await
        .map_err(CommandError::from)?;
        
    Ok(())
}

/// Logout and disconnect
#[tauri::command]
pub async fn sdk_logout(state: State<'_, SdkState>) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    // Domain Command: Logout
    sdk.logout()
        .await
        .map_err(CommandError::from)?;
        
    Ok(())
}

/// Generate a test token for development
/// 
/// This is NOT for production use.
#[tauri::command]
pub async fn sdk_generate_test_token(user_id: String) -> Result<String, CommandError> {
    flare_im_core_sdk::shared::utils::generate_test_token(&user_id).map_err(|e| CommandError::from(e.to_string()))
}

// Helper to parse server URL
fn parse_server_url(url: &str) -> (String, String) {
    if let Some(port) = url.split(':').nth(2).and_then(|p| p.split('/').next()) {
         let host = url.split("://").nth(1).and_then(|s| s.split(':').next()).unwrap_or("localhost");
         let ws_url = format!("ws://{}:{}", host, port);
         // Assuming QUIC is on next port for dev, or same port for prod (usually different)
         // Here we just use a convention or the same config
         let quic_url = format!("quic://{}:{}", host, port.replace("60051", "60052"));
         (ws_url, quic_url)
    } else {
        ("ws://localhost:60051".to_string(), "quic://localhost:60052".to_string())
    }
}
