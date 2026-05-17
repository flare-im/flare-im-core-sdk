//! 在线状态：查询与订阅用户 presence。

use std::collections::HashMap;

use flare_im_core_sdk::client::api::UserPresenceDto;
use tauri::{AppHandle, State};

use crate::state::SdkState;

#[tauri::command]
pub async fn sdk_get_user_presence(
    state: State<'_, SdkState>,
    user_id: String,
) -> std::result::Result<UserPresenceDto, String> {
    state
        .client()
        .get_user_presence(&user_id)
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_batch_get_user_presence(
    state: State<'_, SdkState>,
    user_ids: Vec<String>,
) -> std::result::Result<HashMap<String, UserPresenceDto>, String> {
    state
        .client()
        .batch_get_user_presence(&user_ids)
        .await
        .map_err(super::map_sdk_err)
}

#[tauri::command]
pub async fn sdk_subscribe_user_presence(
    state: State<'_, SdkState>,
    _app: AppHandle,
    user_ids: Vec<String>,
) -> std::result::Result<(), String> {
    state
        .client()
        .subscribe_user_presence(user_ids)
        .await
        .map_err(super::map_sdk_err)
}
