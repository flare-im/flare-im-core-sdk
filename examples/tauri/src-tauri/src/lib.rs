//! Tauri IM Chat Backend
//!
//! 使用 `flare-im-core-sdk-tauri` 提供完整的 IM 功能（连接、会话、消息、同步、事件转发）

use flare_im_core_sdk_tauri::{im_invoke_handler, SdkState};

/// 初始化 Tauri 应用
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tauri::Builder::default()
        .manage(SdkState::new())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(im_invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
