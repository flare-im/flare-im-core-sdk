//! Flare IM Core SDK - Tauri Bindings
//!
//! # Tauri Access Layer for Flare IM
//!
//! This crate provides a clean, production-ready interface between Tauri applications and the Flare IM Core SDK.
//! It handles:
//! - SDK Lifecycle & State Management
//! - SQLite Storage Initialization
//! - Command Bridging (Tauri -> Rust -> SDK)
//! - Event Forwarding (SDK -> Rust -> Tauri)
//!
//! ## Usage
//!
//! Register the plugin in your Tauri application:
//!
//! ```rust,no_run
//! use flare_im_core_sdk_tauri::{SdkState, commands};
//!
//! tauri::Builder::default()
//!     .manage(SdkState::new())
//!     .invoke_handler(tauri::generate_handler![
//!         commands::lifecycle::sdk_init,
//!         commands::lifecycle::sdk_login,
//!         commands::lifecycle::sdk_connect,
//!         commands::lifecycle::sdk_logout,
//!         commands::lifecycle::sdk_generate_test_token,
//!         
//!         // Message Commands
//!         commands::message::sdk_create_text_message,
//!         commands::message::sdk_create_text_at_message,
//!         commands::message::sdk_create_image_message,
//!         commands::message::sdk_create_image_message_by_url,
//!         commands::message::sdk_create_sound_message,
//!         commands::message::sdk_create_sound_message_by_url,
//!         commands::message::sdk_create_video_message,
//!         commands::message::sdk_create_video_message_by_url,
//!         commands::message::sdk_create_file_message,
//!         commands::message::sdk_create_file_message_by_url,
//!         commands::message::sdk_send_message,
//!         commands::message::sdk_send_text_message,
//!         commands::message::sdk_get_message_history,
//!         commands::message::sdk_mark_read,
//!         commands::message::sdk_recall_message,
//!         commands::message::sdk_add_reaction,
//!         commands::message::sdk_remove_reaction,
//!         
//!         // Conversation Commands
//!         commands::conversation::sdk_get_conversations,
//!         commands::conversation::sdk_get_conversation_list_split,
//!         commands::conversation::sdk_get_one_conversation,
//!         commands::conversation::sdk_get_multiple_conversation,
//!         commands::conversation::sdk_get_conversation_id_by_session_type,
//!         commands::conversation::sdk_get_total_unread_msg_count,
//!         commands::conversation::sdk_get_input_states,
//!         commands::conversation::sdk_mark_conversation_as_read,
//!         commands::conversation::sdk_mark_session_read,
//!         commands::conversation::sdk_mark_all_conversations_as_read,
//!         commands::conversation::sdk_set_conversation_draft,
//!         commands::conversation::sdk_hide_conversation,
//!         commands::conversation::sdk_hide_all_conversations,
//!         commands::conversation::sdk_delete_conversation,
//!         commands::conversation::sdk_clear_conversation_messages,
//!         commands::conversation::sdk_set_conversation_info,
//!         commands::conversation::sdk_set_input_state,
//!         commands::conversation::sdk_create_session,
//!         
//!         commands::sync::sdk_sync,
//!         commands::sync::sdk_sync_session_incremental,
//!         commands::sync::sdk_get_messages,
//! ```\n
pub mod commands;
pub mod events;
pub mod state;
pub mod utils;
pub mod error;

pub use state::SdkState;
pub use error::CommandError;

/// Register event subscribers to forward SDK events to Tauri frontend
///
/// This is called internally by `sdk_init`, but exposed if manual registration is needed.
pub async fn register_event_subscribers(
    sdk: &flare_im_core_sdk::interface::facade::ImCoreSdk,
    app: tauri::AppHandle,
) -> anyhow::Result<()> {
    use events::*;
    use std::sync::Arc;
    
    // Subscribe to Message Events
    let message_forwarder = Arc::new(message::MessageEventForwarder::new(app.clone()));
    sdk.events().subscribe_message(message_forwarder).await;
    
    // Subscribe to Connection Events
    let connection_forwarder = Arc::new(connection::ConnectionEventForwarder::new(app.clone()));
    sdk.events().subscribe_connection(connection_forwarder).await;
    
    // Subscribe to Session Events
    let session_forwarder = Arc::new(session::SessionEventForwarder::new(app.clone()));
    sdk.events().subscribe_session(session_forwarder).await;
    
    // Subscribe to Conversation Events
    let conversation_forwarder = Arc::new(conversation::ConversationEventForwarder::new(app.clone()));
    sdk.events().subscribe_conversation(conversation_forwarder).await;
    
    // Subscribe to Sync Events
    let sync_forwarder = Arc::new(sync::SyncEventForwarder::new(app.clone()));
    sdk.events().subscribe_sync(sync_forwarder).await;

    Ok(())
}