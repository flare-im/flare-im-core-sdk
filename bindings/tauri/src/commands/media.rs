//! 媒体命令：带进度发送、网关 URL、本地缓存与用户下载目录落盘。

use std::sync::{Arc, Mutex};

use tauri::Emitter;
use tauri::State;

use crate::model::{SendAckPayload, UploadProgressPayload};
use crate::state::SdkState;
use flare_im_core_sdk::application::{FileDownloadProgress, FileDownloadProgressCallback};
use flare_im_core_sdk::client::UploadProgressCallback;
use flare_im_core_sdk::model::{
    IMMessage, MediaAccessUrl, MediaCacheEntryVo, MediaCacheStatsVo, MediaResolvedAccess,
};

#[tauri::command]
pub async fn sdk_send_with_media_progress(
    state: State<'_, SdkState>,
    app: tauri::AppHandle,
    message: IMMessage,
) -> std::result::Result<SendAckPayload, String> {
    let app_emit = app.clone();
    let progress_cb: UploadProgressCallback = Arc::new(move |progress| {
        let payload: UploadProgressPayload = progress.into();
        let _ = app_emit.emit("im://upload_progress", payload);
    });
    let ack = state
        .message_api()
        .await
        .map_err(|e| e.to_string())?
        .send_with_media_progress(message, Some(progress_cb))
        .await
        .map_err(|e| e.to_string())?;
    Ok(ack.into())
}

#[tauri::command]
pub async fn sdk_get_file_url(
    state: State<'_, SdkState>,
    file_id: String,
    expires_in: Option<i32>,
) -> std::result::Result<MediaAccessUrl, String> {
    state.media_api().await.map_err(|e| e.to_string())?
        .get_file_url(&file_id, expires_in.unwrap_or(3600))
        .await
        .map_err(|e| e.to_string())
}

/// 网关临时直链（`download: true`）。
#[tauri::command]
pub async fn sdk_media_temp_download_url(
    state: State<'_, SdkState>,
    file_id: String,
    expires_in: Option<i32>,
) -> std::result::Result<MediaAccessUrl, String> {
    state.media_api().await.map_err(|e| e.to_string())?
        .get_temp_url_for_file_download(&file_id, expires_in.unwrap_or(3600))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_user_file_download_get_saved_path(
    state: State<'_, SdkState>,
    download_key: String,
) -> std::result::Result<Option<String>, String> {
    state.media_api().await.map_err(|e| e.to_string())?
        .user_download_get_saved_path(&download_key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_user_file_download_delete_record(
    state: State<'_, SdkState>,
    download_key: String,
) -> std::result::Result<(), String> {
    state.media_api().await.map_err(|e| e.to_string())?
        .user_download_delete_record(&download_key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_set_file_download_subfolder(
    state: State<'_, SdkState>,
    name: String,
) -> std::result::Result<(), String> {
    state.media_api().await.map_err(|e| e.to_string())?
        .user_download_set_subfolder(&name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_get_file_download_subfolder(
    state: State<'_, SdkState>,
) -> std::result::Result<String, String> {
    state.media_api().await.map_err(|e| e.to_string())?
        .user_download_get_subfolder()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_cancel_user_file_download(
    state: State<'_, SdkState>,
    download_key: String,
) -> std::result::Result<bool, String> {
    Ok(state
        .media_api()
        .await
        .map_err(|e| e.to_string())?
        .cancel_user_file_download(&download_key))
}

#[tauri::command]
pub async fn sdk_download_file_to_downloads(
    state: State<'_, SdkState>,
    download_key: String,
    display_file_name: String,
    source_path: Option<String>,
    source_url: Option<String>,
    remote_file_id: Option<String>,
    expires_in: Option<i32>,
    channel: tauri::ipc::Channel<FileDownloadProgress>,
) -> std::result::Result<String, String> {
    let api = state.media_api().await.map_err(|e| e.to_string())?;
    let channel = Mutex::new(channel);
    let cb: FileDownloadProgressCallback = Arc::new(move |p: FileDownloadProgress| {
        if let Ok(g) = channel.lock() {
            let _ = g.send(p);
        }
    });
    api.download_file_to_user_downloads_folder(
        &download_key,
        &display_file_name,
        source_path.as_deref(),
        source_url.as_deref(),
        remote_file_id.as_deref(),
        expires_in.unwrap_or(3600),
        Some(cb),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn sdk_path_exists(path: String) -> bool {
    let p = path.trim();
    !p.is_empty() && std::path::Path::new(p).is_file()
}

#[tauri::command]
pub async fn sdk_resolve_media_access(
    state: State<'_, SdkState>,
    file_id: String,
    expires_in: Option<i32>,
) -> std::result::Result<MediaResolvedAccess, String> {
    state.media_api().await.map_err(|e| e.to_string())?
        .resolve_media_access(&file_id, expires_in.unwrap_or(3600))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_cache_remote_media(
    state: State<'_, SdkState>,
    file_id: String,
    expires_in: Option<i32>,
) -> std::result::Result<MediaCacheEntryVo, String> {
    state.media_api().await.map_err(|e| e.to_string())?
        .cache_remote_media(&file_id, expires_in.unwrap_or(3600))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_media_cache_stats(
    state: State<'_, SdkState>,
) -> std::result::Result<MediaCacheStatsVo, String> {
    state.media_api().await.map_err(|e| e.to_string())?
        .media_cache_stats()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_set_media_cache_max_bytes(
    state: State<'_, SdkState>,
    max_bytes: u64,
) -> std::result::Result<(), String> {
    state.media_api().await.map_err(|e| e.to_string())?
        .set_media_cache_max_bytes(max_bytes)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_set_media_cache_root(
    state: State<'_, SdkState>,
    absolute_path: Option<String>,
) -> std::result::Result<(), String> {
    state.media_api().await.map_err(|e| e.to_string())?
        .set_media_cache_root(absolute_path.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_clear_media_cache(state: State<'_, SdkState>) -> std::result::Result<(), String> {
    state.media_api().await.map_err(|e| e.to_string())?
        .clear_media_cache()
        .await
        .map_err(|e| e.to_string())
}
