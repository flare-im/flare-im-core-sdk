//! 会话 Tauri 命令（[`IMClient`] / [`ConversationApi`] 薄封装）。

pub mod handlers;

use tauri::State;

use crate::state::SdkState;
use flare_im_core_sdk::model::conversation::ConversationType;
use flare_im_core_sdk::model::{Conversation, ConversationListQuery, ConversationParticipant};

pub use handlers::*;

#[tauri::command]
pub async fn sdk_conversation_list(
    state: State<'_, SdkState>,
) -> std::result::Result<Vec<Conversation>, String> {
    let api = state.conversation_api().await.map_err(|e| e.to_string())?;
    handlers::list(&api).await
}

#[tauri::command]
pub async fn sdk_conversation_list_by_query(
    state: State<'_, SdkState>,
    query: ConversationListQuery,
) -> std::result::Result<Vec<Conversation>, String> {
    let api = state.conversation_api().await.map_err(|e| e.to_string())?;
    handlers::list_by_query(&api, query).await
}

#[tauri::command]
pub async fn sdk_conversation_get(
    state: State<'_, SdkState>,
    conversation_id: String,
) -> std::result::Result<Option<Conversation>, String> {
    let api = state.conversation_api().await.map_err(|e| e.to_string())?;
    handlers::get(&api, &conversation_id).await
}

#[tauri::command]
pub async fn sdk_conversation_get_one(
    state: State<'_, SdkState>,
    source_id: String,
    conversation_type: i32,
) -> std::result::Result<Conversation, String> {
    let ct = ConversationType::from_proto_int(conversation_type);
    let api = state.conversation_api().await.map_err(|e| e.to_string())?;
    handlers::get_one(&api, &source_id, ct).await
}

#[tauri::command]
pub async fn sdk_conversation_get_group_by_user_ids(
    state: State<'_, SdkState>,
    user_ids: Vec<String>,
    display_name: Option<String>,
) -> std::result::Result<Conversation, String> {
    let api = state.conversation_api().await.map_err(|e| e.to_string())?;
    handlers::get_group_by_user_ids(&api, &user_ids, display_name.as_deref()).await
}

#[tauri::command]
pub async fn sdk_conversation_sync_participants(
    state: State<'_, SdkState>,
    conversation_id: String,
    limit: Option<i32>,
) -> std::result::Result<Vec<ConversationParticipant>, String> {
    handlers::sync_participants(&state.client(), &conversation_id, limit.unwrap_or(200)).await
}

#[tauri::command]
pub async fn sdk_conversation_list_participants(
    state: State<'_, SdkState>,
    conversation_id: String,
    offset: Option<u32>,
    limit: Option<u32>,
) -> std::result::Result<Vec<ConversationParticipant>, String> {
    let stores = state.stores().await.map_err(|e| e.to_string())?;
    handlers::list_participants(
        &stores,
        &conversation_id,
        offset.unwrap_or(0),
        limit.unwrap_or(200),
    )
    .await
}

#[tauri::command]
pub async fn sdk_conversation_get_multiple(
    state: State<'_, SdkState>,
    conversation_ids: Vec<String>,
) -> std::result::Result<Vec<Conversation>, String> {
    let api = state.conversation_api().await.map_err(|e| e.to_string())?;
    handlers::get_multiple(&api, &conversation_ids).await
}

#[tauri::command]
pub async fn sdk_conversation_list_paginated(
    state: State<'_, SdkState>,
    cursor: Option<String>,
    limit: Option<u32>,
) -> std::result::Result<Vec<Conversation>, String> {
    let api = state.conversation_api().await.map_err(|e| e.to_string())?;
    handlers::list_paginated(&api, cursor.as_deref(), limit).await
}

#[tauri::command]
pub async fn sdk_conversation_list_raw(
    state: State<'_, SdkState>,
) -> std::result::Result<Vec<Conversation>, String> {
    let api = state.conversation_api().await.map_err(|e| e.to_string())?;
    handlers::list_raw(&api).await
}

#[tauri::command]
pub async fn sdk_conversation_mark_read(
    state: State<'_, SdkState>,
    conversation_id: String,
    read_seq: u64,
) -> std::result::Result<(), String> {
    let api = state.conversation_api().await.map_err(|e| e.to_string())?;
    handlers::mark_read(&api, &conversation_id, read_seq).await
}

#[tauri::command]
pub async fn sdk_conversation_mark_all_read(
    state: State<'_, SdkState>,
) -> std::result::Result<(), String> {
    let api = state.conversation_api().await.map_err(|e| e.to_string())?;
    handlers::mark_all_read(&api).await
}

#[tauri::command]
pub async fn sdk_conversation_delete(
    state: State<'_, SdkState>,
    conversation_id: String,
) -> std::result::Result<(), String> {
    let api = state.conversation_api().await.map_err(|e| e.to_string())?;
    handlers::delete(&api, &conversation_id).await
}

#[tauri::command]
pub async fn sdk_conversation_set_pinned(
    state: State<'_, SdkState>,
    conversation_id: String,
    pinned: bool,
) -> std::result::Result<(), String> {
    let api = state.conversation_api().await.map_err(|e| e.to_string())?;
    handlers::set_pinned(&api, &conversation_id, pinned).await
}

#[tauri::command]
pub async fn sdk_conversation_set_muted(
    state: State<'_, SdkState>,
    conversation_id: String,
    muted: bool,
) -> std::result::Result<(), String> {
    let api = state.conversation_api().await.map_err(|e| e.to_string())?;
    handlers::set_muted(&api, &conversation_id, muted).await
}

#[tauri::command]
pub async fn sdk_conversation_set_archived(
    state: State<'_, SdkState>,
    conversation_id: String,
    archived: bool,
) -> std::result::Result<(), String> {
    let api = state.conversation_api().await.map_err(|e| e.to_string())?;
    handlers::set_archived(&api, &conversation_id, archived).await
}

#[tauri::command]
pub async fn sdk_conversation_mark_unread(
    state: State<'_, SdkState>,
    conversation_id: String,
) -> std::result::Result<u32, String> {
    let api = state.conversation_api().await.map_err(|e| e.to_string())?;
    handlers::mark_unread(&api, &conversation_id).await
}

#[tauri::command]
pub async fn sdk_conversation_update_draft(
    state: State<'_, SdkState>,
    conversation_id: String,
    draft: Option<String>,
) -> std::result::Result<(), String> {
    let api = state.conversation_api().await.map_err(|e| e.to_string())?;
    handlers::update_draft(&api, &conversation_id, draft.as_deref()).await
}
