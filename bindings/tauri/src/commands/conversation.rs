//! 会话命令：透传 [flare_im_core_sdk::client::ConversationApi]。

use tauri::State;

use crate::state::SdkState;
use flare_im_core_sdk::model::Conversation;
use flare_im_core_sdk::model::conversation::ConversationType;

#[tauri::command]
pub async fn sdk_conversation_list(
    state: State<'_, SdkState>,
) -> std::result::Result<Vec<Conversation>, String> {
    let c = state.client();
    c.conversation()
        .map_err(|e| e.to_string())?
        .list()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_conversation_get(
    state: State<'_, SdkState>,
    conversation_id: String,
) -> std::result::Result<Option<Conversation>, String> {
    let c = state.client();
    c.conversation()
        .map_err(|e| e.to_string())?
        .get(&conversation_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_conversation_get_one(
    state: State<'_, SdkState>,
    source_id: String,
    conversation_type: String,
) -> std::result::Result<Conversation, String> {
    let ct = ConversationType::from(conversation_type.as_str());
    let c = state.client();
    c.conversation()
        .map_err(|e| e.to_string())?
        .get_one(&source_id, &ct)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_conversation_get_multiple(
    state: State<'_, SdkState>,
    conversation_ids: Vec<String>,
) -> std::result::Result<Vec<Conversation>, String> {
    let c = state.client();
    c.conversation()
        .map_err(|e| e.to_string())?
        .get_multiple(&conversation_ids)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_conversation_list_paginated(
    state: State<'_, SdkState>,
    cursor: Option<String>,
    limit: Option<u32>,
) -> std::result::Result<Vec<Conversation>, String> {
    let c = state.client();
    c.conversation()
        .map_err(|e| e.to_string())?
        .list_paginated(cursor.as_deref(), limit)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_conversation_list_raw(
    state: State<'_, SdkState>,
) -> std::result::Result<Vec<Conversation>, String> {
    let c = state.client();
    c.conversation()
        .map_err(|e| e.to_string())?
        .list_raw()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_conversation_mark_read(
    state: State<'_, SdkState>,
    conversation_id: String,
    read_seq: u64,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.conversation()
        .map_err(|e| e.to_string())?
        .mark_read(&conversation_id, read_seq)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_conversation_mark_all_read(
    state: State<'_, SdkState>,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.conversation()
        .map_err(|e| e.to_string())?
        .mark_all_read()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_conversation_delete(
    state: State<'_, SdkState>,
    conversation_id: String,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.conversation()
        .map_err(|e| e.to_string())?
        .delete(&conversation_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_conversation_set_pinned(
    state: State<'_, SdkState>,
    conversation_id: String,
    pinned: bool,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.conversation()
        .map_err(|e| e.to_string())?
        .set_pinned(&conversation_id, pinned)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sdk_conversation_update_draft(
    state: State<'_, SdkState>,
    conversation_id: String,
    draft: Option<String>,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.conversation()
        .map_err(|e| e.to_string())?
        .update_draft(&conversation_id, draft.as_deref())
        .await
        .map_err(|e| e.to_string())
}
