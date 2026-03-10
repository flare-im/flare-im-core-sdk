//! Flare IM Core SDK - Tauri 绑定
//!
//! 基于 `flare_im_core_sdk` 的 IMClient、MessageApi、ConversationApi，
//! 暴露 Tauri 命令与事件（im://message / im://unread 等），供前端调用。
//!
//! 模式：SDK Core ← 语言模型（model）→ App；binding 仅做 proto → model 转换，不直接使用 JSON。

pub mod convert;
pub mod model;
pub mod state;
pub mod commands;

pub use model::SdkConfigOptions;
pub use state::SdkState;

// 在 bindings crate 内展开 generate_handler!，使 __cmd__* 宏可见；示例应用直接使用 im_invoke_handler()
use commands::{
    conversation::{
        sdk_create_session, sdk_delete_conversation, sdk_get_conversation_id_by_session_type,
        sdk_get_conversation_list_split, sdk_get_conversations, sdk_get_multiple_conversation,
        sdk_get_one_conversation, sdk_get_total_unread_msg_count, sdk_hide_all_conversations,
        sdk_hide_conversation, sdk_mark_all_read, sdk_mark_conversation_as_read,
        sdk_set_conversation_draft, sdk_set_conversation_info, sdk_set_input_state,
        sdk_clear_conversation_messages, sdk_get_input_states,
    },
    lifecycle::{
        sdk_generate_test_token, sdk_init, sdk_login, sdk_logout,
    },
    message::{
        sdk_add_reaction, sdk_add_thread_reply, sdk_delete_message, sdk_edit_message,
        sdk_favorite_message, sdk_forward_message, sdk_get_messages, sdk_mark_message,
        sdk_mark_read, sdk_mark_session_read, sdk_pin_message, sdk_quote_message,
        sdk_recall_message, sdk_remove_reaction, sdk_reply_message, sdk_send_text_message,
        sdk_unfavorite_message, sdk_unpin_message,
    },
    sync::{sdk_sync, sdk_sync_session_incremental},
};

/// 返回 IM 命令的 invoke handler，在应用内使用：`.invoke_handler(flare_im_core_sdk_tauri::im_invoke_handler())`
#[must_use]
pub fn im_invoke_handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + 'static {
    tauri::generate_handler![
        sdk_init,
        sdk_login,
        sdk_logout,
        sdk_generate_test_token,
        sdk_send_text_message,
        sdk_get_messages,
        sdk_mark_session_read,
        sdk_mark_read,
        sdk_recall_message,
        sdk_edit_message,
        sdk_delete_message,
        sdk_add_reaction,
        sdk_remove_reaction,
        sdk_pin_message,
        sdk_unpin_message,
        sdk_mark_message,
        sdk_quote_message,
        sdk_reply_message,
        sdk_add_thread_reply,
        sdk_forward_message,
        sdk_favorite_message,
        sdk_unfavorite_message,
        sdk_get_conversations,
        sdk_get_one_conversation,
        sdk_get_conversation_id_by_session_type,
        sdk_get_total_unread_msg_count,
        sdk_mark_conversation_as_read,
        sdk_mark_all_read,
        sdk_delete_conversation,
        sdk_create_session,
        sdk_set_input_state,
        sdk_get_conversation_list_split,
        sdk_get_multiple_conversation,
        sdk_get_input_states,
        sdk_set_conversation_draft,
        sdk_hide_conversation,
        sdk_hide_all_conversations,
        sdk_clear_conversation_messages,
        sdk_set_conversation_info,
        sdk_sync,
        sdk_sync_session_incremental,
    ]
}
