//! Conversation Commands
//!
//! Handles conversation listing, creation, and updates.

use tauri::State;
use crate::state::SdkState;
use crate::error::CommandError;
use flare_im_core_sdk::domain::conversation::{Conversation, InputStateType};

/// Get all conversations list
#[tauri::command]
pub async fn sdk_get_conversations(
    state: State<'_, SdkState>,
) -> Result<Vec<Conversation>, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.conversation()
        .get_all_conversation_list()
        .await
        .map_err(CommandError::from)
}

/// Get conversations with pagination
#[tauri::command]
pub async fn sdk_get_conversation_list_split(
    state: State<'_, SdkState>,
    page: usize,
    page_size: usize,
) -> Result<(Vec<Conversation>, usize), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.conversation()
        .get_conversation_list_split(page, page_size)
        .await
        .map_err(CommandError::from)
}

/// Get a single conversation by ID
#[tauri::command]
pub async fn sdk_get_one_conversation(
    state: State<'_, SdkState>,
    conversation_id: String,
) -> Result<Conversation, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.conversation()
        .get_one_conversation(conversation_id)
        .await
        .map_err(CommandError::from)
}

/// Get multiple conversations by IDs
#[tauri::command]
pub async fn sdk_get_multiple_conversation(
    state: State<'_, SdkState>,
    conversation_ids: Vec<String>,
) -> Result<Vec<Conversation>, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.conversation()
        .get_multiple_conversation(conversation_ids)
        .await
        .map_err(CommandError::from)
}

/// Get conversation IDs by session type
#[tauri::command]
pub async fn sdk_get_conversation_id_by_session_type(
    state: State<'_, SdkState>,
    conversation_type: String,
    user_id: Option<String>,
) -> Result<Vec<String>, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.conversation()
        .get_conversation_id_by_session_type(conversation_type, user_id)
        .await
        .map_err(CommandError::from)
}

/// Get total unread message count
#[tauri::command]
pub async fn sdk_get_total_unread_msg_count(
    state: State<'_, SdkState>,
) -> Result<u32, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.conversation()
        .get_total_unread_msg_count()
        .await
        .map_err(CommandError::from)
}

/// Get input states for a conversation
#[tauri::command]
pub async fn sdk_get_input_states(
    state: State<'_, SdkState>,
    conversation_id: String,
) -> Result<Option<serde_json::Value>, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.conversation()
        .get_input_states(conversation_id)
        .await
        .map_err(CommandError::from)
}

/// Mark conversation as read
#[tauri::command]
pub async fn sdk_mark_conversation_as_read(
    state: State<'_, SdkState>,
    conversation_id: String,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.conversation()
        .mark_conversation_message_as_read(conversation_id)
        .await
        .map_err(CommandError::from)
}

/// Mark session as read with last sequence (alias for conversation)
#[tauri::command]
pub async fn sdk_mark_session_read(
    state: State<'_, SdkState>,
    session_id: String,
    last_seq: Option<u64>,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    // 当前不支持 last_seq 精确定位，统一调用会话标记已读
    let _ = last_seq;
    sdk.conversation()
        .mark_conversation_message_as_read(session_id)
        .await
        .map_err(CommandError::from)
}

/// Mark all conversations as read
#[tauri::command]
pub async fn sdk_mark_all_conversations_as_read(
    state: State<'_, SdkState>,
) -> Result<usize, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.conversation()
        .mark_all_conversations_as_read()
        .await
        .map_err(CommandError::from)
}

/// Set conversation draft
#[tauri::command]
pub async fn sdk_set_conversation_draft(
    state: State<'_, SdkState>,
    conversation_id: String,
    draft: Option<String>,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.conversation()
        .set_conversation_draft(conversation_id, draft)
        .await
        .map_err(CommandError::from)
}

/// Hide a conversation
#[tauri::command]
pub async fn sdk_hide_conversation(
    state: State<'_, SdkState>,
    conversation_id: String,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.conversation()
        .hide_conversation(conversation_id)
        .await
        .map_err(CommandError::from)
}

/// Hide all conversations
#[tauri::command]
pub async fn sdk_hide_all_conversations(
    state: State<'_, SdkState>,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.conversation()
        .hide_all_conversation()
        .await
        .map_err(CommandError::from)
}

/// Delete a conversation
#[tauri::command]
pub async fn sdk_delete_conversation(
    state: State<'_, SdkState>,
    conversation_id: String,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.conversation()
        .delete_conversation_and_delete_all_msg(conversation_id)
        .await
        .map_err(CommandError::from)?;
        
    Ok(())
}

/// Clear conversation messages
#[tauri::command]
pub async fn sdk_clear_conversation_messages(
    state: State<'_, SdkState>,
    conversation_id: String,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.conversation()
        .clear_conversation_and_delete_all_msg(conversation_id)
        .await
        .map_err(CommandError::from)
}

/// Set conversation info
#[tauri::command]
pub async fn sdk_set_conversation_info(
    state: State<'_, SdkState>,
    conversation_id: String,
    display_name: Option<String>,
    avatar_url: Option<String>,
    description: Option<String>,
    announcement: Option<String>,
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.conversation()
        .set_conversation(conversation_id, display_name, avatar_url, description, announcement)
        .await
        .map_err(CommandError::from)
}

/// Set input state
#[tauri::command]
pub async fn sdk_set_input_state(
    state: State<'_, SdkState>,
    conversation_id: String,
    state_type: String, // String to avoid enum parsing issues from frontend
) -> Result<(), CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    // Parse state_type
    let input_state = match state_type.as_str() {
        "Typing" => InputStateType::Typing,
        "Stopped" => InputStateType::Stopped,
        "Recording" => InputStateType::Recording,
        "Uploading" => InputStateType::Uploading,
        _ => InputStateType::Typing, // Default
    };
    
    sdk.conversation()
        .change_input_states(conversation_id, input_state)
        .await
        .map_err(CommandError::from)
}

/// Mark all conversations as read (alias)
#[tauri::command]
pub async fn sdk_mark_all_read(
    state: State<'_, SdkState>,
) -> Result<usize, CommandError> {
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    sdk.conversation()
        .mark_all_conversations_as_read()
        .await
        .map_err(CommandError::from)
}

/// Create a new session/conversation
#[tauri::command]
pub async fn sdk_create_session(
    state: State<'_, SdkState>,
    session_type: String,
    business_type: String,
    display_name: Option<String>,
    peer_id: Option<String>,
    current_user_id: Option<String>,
) -> Result<String, CommandError> {
    use flare_im_core_sdk::domain::conversation::{Conversation, ConversationParticipant};
    use chrono::Utc;
    use std::collections::HashMap;
    use flare_im_core_sdk::shared::utils::{
        generate_single_chat_conversation_id,
        generate_group_conversation_id,
        validate_conversation_id,
    };
    
    let sdk = state.get_sdk().await.ok_or("SDK not initialized")?;
    
    // Build conversation_id using shared utils
    let conversation_id = if session_type == "single" {
        let me = current_user_id.clone().ok_or_else(|| CommandError::from("current_user_id is required"))?;
        let other = peer_id.clone().ok_or_else(|| CommandError::from("peer_id is required for single session"))?;
        generate_single_chat_conversation_id(&me, &other)
    } else if session_type == "group" {
        // group_id 来源：显示名称或随机 UUID（仅用于示例）
        let group_id = display_name.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        generate_group_conversation_id(&group_id)
    } else {
        return Err(CommandError::from(format!("Unsupported session_type: {}", session_type)));
    };
    
    // Validate conversation_id
    if let Err(e) = validate_conversation_id(&conversation_id) {
        return Err(CommandError::from(format!("Invalid conversation_id generated: {}", e)));
    }
    
    // Create conversation
    let mut conversation = Conversation::new(conversation_id.clone(), session_type.clone());
    if !business_type.is_empty() {
        conversation.business_type = Some(business_type);
    }
    if let Some(name) = display_name {
        conversation.display_name = name;
    }
    
    // Add participants for single chat
    if session_type == "single" {
        if let Some(me) = current_user_id {
            let p = ConversationParticipant {
                user_id: me,
                roles: vec![],
                muted: false,
                pinned: false,
                attributes: HashMap::new(),
                joined_at: Utc::now(),
                nickname: None,
            };
            conversation.add_participant(p);
        }
        if let Some(other) = peer_id {
            let p = ConversationParticipant {
                user_id: other,
                roles: vec![],
                muted: false,
                pinned: false,
                attributes: HashMap::new(),
                joined_at: Utc::now(),
                nickname: None,
            };
            conversation.add_participant(p);
        }
    } else if session_type == "group" {
        // 群聊至少包含当前用户自己作为创建者参与者（可选）
        if let Some(me) = current_user_id {
            let p = ConversationParticipant {
                user_id: me,
                roles: vec!["owner".to_string()],
                muted: false,
                pinned: false,
                attributes: HashMap::new(),
                joined_at: Utc::now(),
                nickname: None,
            };
            conversation.add_participant(p);
        }
    }
    
    // Persist conversation
    sdk.sdk_context()
        .conversation_repository
        .save(&conversation)
        .await
        .map_err(CommandError::from)?;
    
    Ok(conversation.conversation_id)
}
