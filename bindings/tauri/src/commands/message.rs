//! Message Commands
//!
//! Handles message sending, retrieval, and operations.

use tauri::State;
use crate::state::SdkState;
use crate::error::CommandError;
use flare_im_core_sdk::domain::message::{Message, DeleteType, MarkType};
use flare_im_core_sdk::interface::facade::MentionInfo;
use crate::utils::ensure_message_content_text;

/// Create a text message
#[tauri::command]
pub async fn sdk_create_text_message(
    state: State<'_, SdkState>,
    text: String,
    receiver_id: Option<String>,
) -> Result<Message, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    let mut message = sdk.message()
        .create_text_message(text, receiver_id)
        .await
        .map_err(CommandError::from)?;
        
    ensure_message_content_text(&mut message);
    Ok(message)
}

/// Create a text message with mentions
#[tauri::command]
pub async fn sdk_create_text_at_message(
    state: State<'_, SdkState>,
    text: String,
    mentions: Vec<MentionInfo>,
) -> Result<Message, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    let mut message = sdk.message()
        .create_text_at_message(text, mentions)
        .await
        .map_err(CommandError::from)?;
        
    ensure_message_content_text(&mut message);
    Ok(message)
}

/// Create an image message from file path
#[tauri::command]
pub async fn sdk_create_image_message(
    state: State<'_, SdkState>,
    file_path: String,
) -> Result<Message, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    let mut message = sdk.message()
        .create_image_message_from_full_path(file_path)
        .await
        .map_err(CommandError::from)?;
        
    ensure_message_content_text(&mut message);
    Ok(message)
}

/// Create an image message by URL
#[tauri::command]
pub async fn sdk_create_image_message_by_url(
    state: State<'_, SdkState>,
    url: String,
) -> Result<Message, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    let mut message = sdk.message()
        .create_image_message_by_url(url)
        .await
        .map_err(CommandError::from)?;
        
    ensure_message_content_text(&mut message);
    Ok(message)
}

/// Create a sound message from file path
#[tauri::command]
pub async fn sdk_create_sound_message(
    state: State<'_, SdkState>,
    file_path: String,
    duration_ms: u64,
) -> Result<Message, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    let mut message = sdk.message()
        .create_sound_message_from_full_path(file_path, duration_ms)
        .await
        .map_err(CommandError::from)?;
        
    ensure_message_content_text(&mut message);
    Ok(message)
}

/// Create a sound message by URL
#[tauri::command]
pub async fn sdk_create_sound_message_by_url(
    state: State<'_, SdkState>,
    url: String,
    duration_ms: u64,
) -> Result<Message, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    let mut message = sdk.message()
        .create_sound_message_by_url(url, duration_ms)
        .await
        .map_err(CommandError::from)?;
        
    ensure_message_content_text(&mut message);
    Ok(message)
}

/// Create a video message from file path
#[tauri::command]
pub async fn sdk_create_video_message(
    state: State<'_, SdkState>,
    file_path: String,
    duration_ms: u64,
    width: i32,
    height: i32,
) -> Result<Message, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    let mut message = sdk.message()
        .create_video_message_from_full_path(file_path, duration_ms, width, height)
        .await
        .map_err(CommandError::from)?;
        
    ensure_message_content_text(&mut message);
    Ok(message)
}

/// Create a video message by URL
#[tauri::command]
pub async fn sdk_create_video_message_by_url(
    state: State<'_, SdkState>,
    url: String,
    duration_ms: u64,
    width: i32,
    height: i32,
) -> Result<Message, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    let mut message = sdk.message()
        .create_video_message_by_url(url, duration_ms, width, height)
        .await
        .map_err(CommandError::from)?;
        
    ensure_message_content_text(&mut message);
    Ok(message)
}

/// Create a file message from file path
#[tauri::command]
pub async fn sdk_create_file_message(
    state: State<'_, SdkState>,
    file_path: String,
) -> Result<Message, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    let mut message = sdk.message()
        .create_file_message_from_full_path(file_path)
        .await
        .map_err(CommandError::from)?;
        
    ensure_message_content_text(&mut message);
    Ok(message)
}

/// Create a file message by URL
#[tauri::command]
pub async fn sdk_create_file_message_by_url(
    state: State<'_, SdkState>,
    url: String,
    file_name: String,
    file_size: u64,
    mime_type: String,
) -> Result<Message, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    let mut message = sdk.message()
        .create_file_message_by_url(url, file_name, file_size, mime_type)
        .await
        .map_err(CommandError::from)?;
        
    ensure_message_content_text(&mut message);
    Ok(message)
}

/// Send a message
#[tauri::command]
pub async fn sdk_send_message(
    state: State<'_, SdkState>,
    message: Message,
    conversation_id: String,
) -> Result<String, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    let client_msg_id = message.client_msg_id.clone();
    
    sdk.message()
        .send_message(message, conversation_id)
        .await
        .map_err(CommandError::from)?;
        
    Ok(client_msg_id)
}

/// Send a text message (Convenience)
#[tauri::command]
pub async fn sdk_send_text_message(
    state: State<'_, SdkState>,
    session_id: String,
    text: String,
    receiver_id: Option<String>,
) -> Result<String, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    // 1. Create message domain object
    let message = sdk.message()
        .create_text_message(text, receiver_id)
        .await
        .map_err(CommandError::from)?;
        
    let client_msg_id = message.client_msg_id.clone();
    
    // 2. Send message
    sdk.message()
        .send_message(message, session_id)
        .await
        .map_err(CommandError::from)?;
        
    Ok(client_msg_id)
}

/// Get message history for a conversation
#[tauri::command]
pub async fn sdk_get_message_history(
    state: State<'_, SdkState>,
    conversation_id: String,
    limit: u32,
    before_message_id: Option<String>,
) -> Result<Vec<Message>, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    // Use QueryHandler directly for flexible querying
    // Note: before_message_id is treated as cursor
    let query = flare_im_core_sdk::application::queries::ListMessagesQuery {
        conversation_id,
        limit: Some(limit as usize),
        cursor: before_message_id,
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

/// Mark conversation messages as read
#[tauri::command]
pub async fn sdk_mark_read(
    state: State<'_, SdkState>,
    conversation_id: String,
    message_ids: Vec<String>,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    // Mark specific messages as read
    sdk.message()
        .batch_mark_message_read(conversation_id, Some(message_ids), false)
        .await
        .map_err(CommandError::from)?;
        
    Ok(())
}

/// Revoke/Recall a message
#[tauri::command]
pub async fn sdk_recall_message(
    state: State<'_, SdkState>,
    message_id: String,
    reason: Option<String>,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.message()
        .recall_message(message_id, reason)
        .await
        .map_err(CommandError::from)?;
        
    Ok(())
}

/// Edit a message (Text only for now)
#[tauri::command]
pub async fn sdk_edit_message(
    state: State<'_, SdkState>,
    session_id: String, // Kept for compatibility but not used
    message_id: String,
    text: String,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    // For now we ignore session_id
    let _ = session_id;
    
    sdk.message()
        .edit_text_message(message_id, text, None)
        .await
        .map_err(CommandError::from)?;
        
    Ok(())
}

/// Delete a message
#[tauri::command]
pub async fn sdk_delete_message(
    state: State<'_, SdkState>,
    message_id: String,
    delete_type: i32, // 0=Soft, 1=Hard
    notify_others: bool,
    reason: Option<String>,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    let dt = if delete_type == 1 { DeleteType::Hard } else { DeleteType::Soft };
    
    // Use notify_others if needed, currently unused
    let _ = notify_others;
    
    sdk.message()
        .delete_message(message_id, dt, reason)
        .await
        .map_err(CommandError::from)?;
        
    Ok(())
}

/// Reply to a message
#[tauri::command]
pub async fn sdk_reply_message(
    state: State<'_, SdkState>,
    session_id: String,
    reply_to_message_id: String,
    text: String,
) -> Result<String, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    // For now we pass None for quoted_sender_id and preview, assuming Facade/Backend handles it or it's optional
    // Ideally we should look up the message to get sender_id and preview
    let msg_id = sdk.message()
        .reply_text_message(session_id, reply_to_message_id, None, None, text)
        .await
        .map_err(CommandError::from)?;
        
    Ok(msg_id)
}

/// Quote and send a message (Explicit quote)
#[tauri::command]
pub async fn sdk_quote_message(
    state: State<'_, SdkState>,
    session_id: String,
    quoted_message_id: String,
    text: String,
    preview_text: String,
) -> Result<String, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    // Create the quote message
    let message = sdk.message()
        .create_quote_text_message(quoted_message_id, None, Some(preview_text), text)
        .await
        .map_err(CommandError::from)?;
        
    let client_msg_id = message.client_msg_id.clone();
    
    // Send it
    sdk.message()
        .send_message(message, session_id)
        .await
        .map_err(CommandError::from)?;
        
    Ok(client_msg_id)
}

/// Forward messages
#[tauri::command]
pub async fn sdk_forward_message(
    state: State<'_, SdkState>,
    message_ids: Vec<String>,
    target_session_id: String,
    merge_forward: bool,
    reason: Option<String>,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    // 1. Create forward message(s)
    if merge_forward {
        // Create one merged message
        let message = sdk.message()
            .create_merge_message(message_ids)
            .await
            .map_err(CommandError::from)?;
        
        // Send
        sdk.message()
            .send_message(message, target_session_id)
            .await
            .map_err(CommandError::from)?;
    } else {
        // Forward individually
        // Note: forward_messages in Facade seems to be "Create Forward Messages" but not send?
        // Actually `forward_messages` in Facade calls `command_handler.forward_messages` which SENDS them.
        // So we just call that.
        // reason is not used in individual forward currently
        let _ = reason;
        
        sdk.message()
            .forward_messages(message_ids, target_session_id, false)
            .await
            .map_err(CommandError::from)?;
    }
        
    Ok(())
}

/// Add thread reply
#[tauri::command]
pub async fn sdk_add_thread_reply(
    state: State<'_, SdkState>,
    session_id: String,
    thread_id: String,
    text: String,
) -> Result<String, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    let msg_id = sdk.message()
        .add_thread_text_reply(session_id, thread_id, text)
        .await
        .map_err(CommandError::from)?;
        
    Ok(msg_id)
}

/// Pin a message
#[tauri::command]
pub async fn sdk_pin_message(
    state: State<'_, SdkState>,
    message_id: String,
    reason: Option<String>,
    expire_at: Option<String>, // ISO string
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    let expire = if let Some(s) = expire_at {
        Some(chrono::DateTime::parse_from_rfc3339(&s).map_err(|e| CommandError::from(e.to_string()))?.with_timezone(&chrono::Utc))
    } else {
        None
    };
    
    sdk.message()
        .pin_message(message_id, reason, expire)
        .await
        .map_err(CommandError::from)?;
        
    Ok(())
}

/// Unpin a message
#[tauri::command]
pub async fn sdk_unpin_message(
    state: State<'_, SdkState>,
    message_id: String,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.message()
        .unpin_message(message_id)
        .await
        .map_err(CommandError::from)?;
        
    Ok(())
}

/// Favorite a message
#[tauri::command]
pub async fn sdk_favorite_message(
    state: State<'_, SdkState>,
    message_id: String,
    tags: Option<Vec<String>>,
    note: Option<String>,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.message()
        .favorite_message(message_id, tags.unwrap_or_default(), note)
        .await
        .map_err(CommandError::from)?;
        
    Ok(())
}

/// Unfavorite a message
#[tauri::command]
pub async fn sdk_unfavorite_message(
    state: State<'_, SdkState>,
    message_id: String,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.message()
        .unfavorite_message(message_id)
        .await
        .map_err(CommandError::from)?;
        
    Ok(())
}

/// Mark a message
#[tauri::command]
pub async fn sdk_mark_message(
    state: State<'_, SdkState>,
    message_id: String,
    mark_type: i32,
    color: Option<String>,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    // Map int to MarkType
    let mt = match mark_type {
        0 => MarkType::Important, // Star -> Important
        1 => MarkType::Todo,
        2 => MarkType::Done,      // Pin -> Done
        _ => MarkType::Important, // Default
    };
    
    sdk.message()
        .mark_message(message_id, mt, color)
        .await
        .map_err(CommandError::from)?;
        
    Ok(())
}

/// Add reaction to a message
#[tauri::command]
pub async fn sdk_add_reaction(
    state: State<'_, SdkState>,
    session_id: String,
    message_id: String,
    emoji: String,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    // For now we ignore session_id as it's not used in add_reaction yet
    let _ = session_id;
    
    sdk.message()
        .add_reaction(message_id, emoji)
        .await
        .map_err(CommandError::from)
}

/// Remove reaction from a message
#[tauri::command]
pub async fn sdk_remove_reaction(
    state: State<'_, SdkState>,
    session_id: String,
    message_id: String,
    emoji: String,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    // For now we ignore session_id as it's not used in remove_reaction yet
    let _ = session_id;
    
    sdk.message()
        .remove_reaction(message_id, emoji)
        .await
        .map_err(CommandError::from)
}
