//! Sync Commands
//!
//! Handles manual synchronization requests.

use tauri::State;
use crate::state::SdkState;
use crate::error::CommandError;
use flare_im_core_sdk::domain::message::Message;

/// Trigger a full bootstrap sync
#[tauri::command]
pub async fn sdk_sync(
    state: State<'_, SdkState>,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.bootstrap_sync()
        .await
        .map_err(CommandError::from)?;
        
    Ok(())
}

/// Incremental sync for a specific conversation/session
#[tauri::command]
pub async fn sdk_sync_session_incremental(
    state: State<'_, SdkState>,
    session_id: String,
) -> Result<u64, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    // 使用 QueryHandler 拉取增量消息，并返回拉取条数
    // 这里简单实现为重新查询最近消息条数，不做复杂游标管理
    let query = flare_im_core_sdk::application::queries::ListMessagesQuery {
        conversation_id: session_id.clone(),
        limit: Some(200),
        cursor: None,
    };
    
    let messages = sdk.sdk_context()
        .query_handler
        .list_messages(query)
        .await
        .map_err(CommandError::from)?;
    
    Ok(messages.len() as u64)
}

use crate::utils::ensure_message_content_text;

/// Get messages for a conversation
#[tauri::command]
pub async fn sdk_get_messages(
    state: State<'_, SdkState>,
    session_id: String,
    limit: Option<usize>,
    cursor: Option<String>,
) -> Result<Vec<Message>, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    let query = flare_im_core_sdk::application::queries::ListMessagesQuery {
        conversation_id: session_id,
        limit,
        cursor,
    };
    
    let mut messages = sdk.sdk_context()
        .query_handler
        .list_messages(query)
        .await
        .map_err(CommandError::from)?;
        
    // Ensure content text is available for frontend
    for msg in &mut messages {
        ensure_message_content_text(msg);
    }
    
    Ok(messages)
}
