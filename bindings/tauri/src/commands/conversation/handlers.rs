//! 会话 Tauri 命令的共享实现（唯一业务入口）。
//!
//! IM 会话读写属于 **core-sdk** 域；[`flare_im_core_sdk_tauri`] 与 [`flare_sdk_tauri`]
//! 的 `#[tauri::command]` 仅做 `SdkState` 适配，逻辑不得复制到 social-sdk。

use flare_im_core_sdk::client::IMClient;
use flare_im_core_sdk::client::api::ConversationApi;
use flare_im_core_sdk::model::conversation::ConversationType;
use flare_im_core_sdk::model::{Conversation, ConversationListQuery, ConversationParticipant};
use flare_im_core_sdk::store::StoreProvider;

pub async fn list(api: &ConversationApi) -> Result<Vec<Conversation>, String> {
    api.list().await.map_err(|e| e.to_string())
}

pub async fn list_by_query(
    api: &ConversationApi,
    query: ConversationListQuery,
) -> Result<Vec<Conversation>, String> {
    api.list_by_query(query).await.map_err(|e| e.to_string())
}

pub async fn get(
    api: &ConversationApi,
    conversation_id: &str,
) -> Result<Option<Conversation>, String> {
    api.get(conversation_id).await.map_err(|e| e.to_string())
}

pub async fn get_one(
    api: &ConversationApi,
    source_id: &str,
    conversation_type: ConversationType,
) -> Result<Conversation, String> {
    api.get_one(source_id, &conversation_type)
        .await
        .map_err(|e| e.to_string())
}

pub async fn get_group_by_user_ids(
    api: &ConversationApi,
    user_ids: &[String],
    display_name: Option<&str>,
) -> Result<Conversation, String> {
    api.get_group_by_user_ids(user_ids, display_name)
        .await
        .map_err(|e| e.to_string())
}

pub async fn sync_participants(
    client: &IMClient,
    conversation_id: &str,
    limit: i32,
) -> Result<Vec<ConversationParticipant>, String> {
    client
        .sync_conversation_participants(conversation_id, limit)
        .await
        .map_err(|e| e.to_string())
}

pub async fn list_participants(
    stores: &StoreProvider,
    conversation_id: &str,
    offset: u32,
    limit: u32,
) -> Result<Vec<ConversationParticipant>, String> {
    let Some(store) = stores.conversation_participants.as_ref() else {
        return Ok(Vec::new());
    };
    store
        .list(conversation_id, offset, limit)
        .await
        .map_err(|e| e.to_string())
}

pub async fn get_multiple(
    api: &ConversationApi,
    conversation_ids: &[String],
) -> Result<Vec<Conversation>, String> {
    api.get_multiple(conversation_ids)
        .await
        .map_err(|e| e.to_string())
}

pub async fn list_paginated(
    api: &ConversationApi,
    cursor: Option<&str>,
    limit: Option<u32>,
) -> Result<Vec<Conversation>, String> {
    api.list_paginated(cursor, limit)
        .await
        .map_err(|e| e.to_string())
}

pub async fn list_raw(api: &ConversationApi) -> Result<Vec<Conversation>, String> {
    api.list_raw().await.map_err(|e| e.to_string())
}

pub async fn mark_read(
    api: &ConversationApi,
    conversation_id: &str,
    read_seq: u64,
) -> Result<(), String> {
    api.mark_read(conversation_id, read_seq)
        .await
        .map_err(|e| e.to_string())
}

pub async fn mark_all_read(api: &ConversationApi) -> Result<(), String> {
    api.mark_all_read().await.map_err(|e| e.to_string())
}

pub async fn delete(api: &ConversationApi, conversation_id: &str) -> Result<(), String> {
    api.delete(conversation_id).await.map_err(|e| e.to_string())
}

pub async fn set_pinned(
    api: &ConversationApi,
    conversation_id: &str,
    pinned: bool,
) -> Result<(), String> {
    api.set_pinned(conversation_id, pinned)
        .await
        .map_err(|e| e.to_string())
}

pub async fn set_muted(
    api: &ConversationApi,
    conversation_id: &str,
    muted: bool,
) -> Result<(), String> {
    api.set_muted(conversation_id, muted)
        .await
        .map_err(|e| e.to_string())
}

pub async fn set_archived(
    api: &ConversationApi,
    conversation_id: &str,
    archived: bool,
) -> Result<(), String> {
    api.set_archived(conversation_id, archived)
        .await
        .map_err(|e| e.to_string())
}

pub async fn mark_unread(api: &ConversationApi, conversation_id: &str) -> Result<u32, String> {
    api.mark_unread(conversation_id)
        .await
        .map_err(|e| e.to_string())
}

pub async fn update_draft(
    api: &ConversationApi,
    conversation_id: &str,
    draft: Option<&str>,
) -> Result<(), String> {
    api.update_draft(conversation_id, draft)
        .await
        .map_err(|e| e.to_string())
}
