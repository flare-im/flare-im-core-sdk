//! 媒体命令：带进度发送、网关 URL、本地缓存与用户下载目录落盘。

use std::sync::{Arc, Mutex};

use serde::Deserialize;
use tauri::Emitter;
use tauri::State;

use crate::model::{MediaDeleteProgressPayload, SendAckPayload, UploadProgressPayload};
use crate::state::SdkState;
use flare_im_core_sdk::application::{
    FileDownloadProgress, FileDownloadProgressCallback, UserFileDownloadRequest,
};
use flare_im_core_sdk::client::UploadProgressCallback;
use flare_im_core_sdk::model::{
    IMMessage, MediaAccessUrl, MediaCacheEntryVo, MediaCacheStatsVo, MediaResolvedAccess,
    UploadOptions, UploadedMedia,
};

#[derive(Debug, Deserialize)]
pub struct DownloadFileToDownloadsPayload {
    pub download_key: String,
    pub display_file_name: String,
    pub source_path: Option<String>,
    pub source_url: Option<String>,
    pub remote_file_id: Option<String>,
    pub expires_in: Option<i32>,
}

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
    state
        .media_api()
        .await
        .map_err(|e| e.to_string())?
        .get_file_url(&file_id, expires_in.unwrap_or(3600))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_media_upload_file(
    state: State<'_, SdkState>,
    path: String,
    chunk_size: Option<usize>,
    channel: tauri::ipc::Channel<UploadProgressPayload>,
) -> std::result::Result<UploadedMedia, String> {
    upload_media_from_path(state, path, chunk_size, channel, MediaUploadKind::File).await
}

#[tauri::command]
pub async fn sdk_media_upload_image(
    state: State<'_, SdkState>,
    path: String,
    chunk_size: Option<usize>,
    channel: tauri::ipc::Channel<UploadProgressPayload>,
) -> std::result::Result<UploadedMedia, String> {
    upload_media_from_path(state, path, chunk_size, channel, MediaUploadKind::Image).await
}

#[tauri::command]
pub async fn sdk_media_upload_video(
    state: State<'_, SdkState>,
    path: String,
    chunk_size: Option<usize>,
    channel: tauri::ipc::Channel<UploadProgressPayload>,
) -> std::result::Result<UploadedMedia, String> {
    upload_media_from_path(state, path, chunk_size, channel, MediaUploadKind::Video).await
}

#[tauri::command]
pub async fn sdk_media_delete_file(
    state: State<'_, SdkState>,
    file_id: String,
    hard_delete: Option<bool>,
    channel: tauri::ipc::Channel<MediaDeleteProgressPayload>,
) -> std::result::Result<bool, String> {
    let fid = file_id.trim().to_string();
    let _ = channel.send(MediaDeleteProgressPayload {
        file_id: fid.clone(),
        phase: "Started".to_string(),
        done: false,
    });
    let result = state
        .media_api()
        .await
        .map_err(|e| e.to_string())?
        .delete_file(&fid, hard_delete.unwrap_or(false))
        .await
        .map_err(|e| e.to_string());
    let _ = channel.send(MediaDeleteProgressPayload {
        file_id: fid,
        phase: if result.is_ok() { "Finished" } else { "Failed" }.to_string(),
        done: true,
    });
    result
}

#[derive(Debug, Clone, Copy)]
enum MediaUploadKind {
    File,
    Image,
    Video,
}

async fn upload_media_from_path(
    state: State<'_, SdkState>,
    path: String,
    chunk_size: Option<usize>,
    channel: tauri::ipc::Channel<UploadProgressPayload>,
    kind: MediaUploadKind,
) -> std::result::Result<UploadedMedia, String> {
    let api = state.media_api().await.map_err(|e| e.to_string())?;
    let channel = Mutex::new(channel);
    let cb: UploadProgressCallback = Arc::new(move |p| {
        if let Ok(g) = channel.lock() {
            let _ = g.send(p.into());
        }
    });
    let options = chunk_size.map(|chunk_size| UploadOptions { chunk_size });
    match kind {
        MediaUploadKind::File => {
            api.upload_file_from_path_with_progress(&path, options, Some(cb))
                .await
        }
        MediaUploadKind::Image => {
            api.upload_image_from_path_with_progress(&path, options, Some(cb))
                .await
        }
        MediaUploadKind::Video => {
            api.upload_video_from_path_with_progress(&path, options, Some(cb))
                .await
        }
    }
    .map_err(|e| e.to_string())
}

/// 网关临时直链（`download: true`）。
#[tauri::command]
pub async fn sdk_media_temp_download_url(
    state: State<'_, SdkState>,
    file_id: String,
    expires_in: Option<i32>,
) -> std::result::Result<MediaAccessUrl, String> {
    state
        .media_api()
        .await
        .map_err(|e| e.to_string())?
        .get_temp_url_for_file_download(&file_id, expires_in.unwrap_or(3600))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_user_file_download_get_saved_path(
    state: State<'_, SdkState>,
    download_key: String,
) -> std::result::Result<Option<String>, String> {
    state
        .media_api()
        .await
        .map_err(|e| e.to_string())?
        .user_download_get_saved_path(&download_key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_user_file_download_delete_record(
    state: State<'_, SdkState>,
    download_key: String,
) -> std::result::Result<(), String> {
    state
        .media_api()
        .await
        .map_err(|e| e.to_string())?
        .user_download_delete_record(&download_key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_set_file_download_subfolder(
    state: State<'_, SdkState>,
    name: String,
) -> std::result::Result<(), String> {
    state
        .media_api()
        .await
        .map_err(|e| e.to_string())?
        .user_download_set_subfolder(&name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_get_file_download_subfolder(
    state: State<'_, SdkState>,
) -> std::result::Result<String, String> {
    state
        .media_api()
        .await
        .map_err(|e| e.to_string())?
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
    payload: DownloadFileToDownloadsPayload,
    channel: tauri::ipc::Channel<FileDownloadProgress>,
) -> std::result::Result<String, String> {
    let api = state.media_api().await.map_err(|e| e.to_string())?;
    let channel = Mutex::new(channel);
    let cb: FileDownloadProgressCallback = Arc::new(move |p: FileDownloadProgress| {
        if let Ok(g) = channel.lock() {
            let _ = g.send(p);
        }
    });
    api.download_file_to_user_downloads_folder(UserFileDownloadRequest {
        download_key: payload.download_key,
        display_file_name: payload.display_file_name,
        source_path: payload.source_path,
        source_http_url: payload.source_url,
        remote_file_id: payload.remote_file_id,
        expires_in: payload.expires_in.unwrap_or(3600),
        on_progress: Some(cb),
    })
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
    state
        .media_api()
        .await
        .map_err(|e| e.to_string())?
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
    state
        .media_api()
        .await
        .map_err(|e| e.to_string())?
        .cache_remote_media(&file_id, expires_in.unwrap_or(3600))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_media_cache_stats(
    state: State<'_, SdkState>,
) -> std::result::Result<MediaCacheStatsVo, String> {
    state
        .media_api()
        .await
        .map_err(|e| e.to_string())?
        .media_cache_stats()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_set_media_cache_max_bytes(
    state: State<'_, SdkState>,
    max_bytes: u64,
) -> std::result::Result<(), String> {
    state
        .media_api()
        .await
        .map_err(|e| e.to_string())?
        .set_media_cache_max_bytes(max_bytes)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_set_media_cache_root(
    state: State<'_, SdkState>,
    absolute_path: Option<String>,
) -> std::result::Result<(), String> {
    state
        .media_api()
        .await
        .map_err(|e| e.to_string())?
        .set_media_cache_root(absolute_path.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_clear_media_cache(state: State<'_, SdkState>) -> std::result::Result<(), String> {
    state
        .media_api()
        .await
        .map_err(|e| e.to_string())?
        .clear_media_cache()
        .await
        .map_err(|e| e.to_string())
}
