//! Flare IM Core SDK — Tauri 命令薄封装：透传 [`flare_im_core_sdk::client::IMClient`] 与 Facade；
//! SdkEvent → `im://*` 由 `sdk_login` 内 `spawn` 转发，无绑定层业务逻辑。
//!
//! IPC JSON 字段为 **snake_case**（与 core-sdk 一致）。Web 宿主若希望业务层只用 camelCase，在宿主侧加薄封装即可，bindings 不维护第二套 serde 命名。
//!
//! ## 性能
//! - [`SdkState`](state::SdkState) 仅持有一个 [`IMClient`]，命令热路径为 `Arc` 克隆，无绑定层异步锁。
//! - `sdk_init` 仅需 `SdkInitArgs`；`sdk_login` 的 `AppHandle` 由 Tauri 注入，勿写入 invoke 参数。

pub mod commands;
pub mod convert;
pub mod model;
pub mod state;

pub use model::{SdkConfigOptions, SendAckPayload};
pub use state::SdkState;

use commands::{
    conversation::{
        sdk_conversation_delete, sdk_conversation_get, sdk_conversation_get_multiple,
        sdk_conversation_get_one, sdk_conversation_list, sdk_conversation_list_paginated,
        sdk_conversation_list_raw, sdk_conversation_mark_all_read, sdk_conversation_mark_read,
        sdk_conversation_set_pinned, sdk_conversation_update_draft,
    },
    host_util::sdk_save_preview_jpeg_temp,
    lifecycle::{
        sdk_current_user_id, sdk_engine_state, sdk_generate_test_token, sdk_init, sdk_is_connected,
        sdk_login, sdk_logout, sdk_mark_session_read, sdk_set_conversation_input_state,
        sdk_sync_conversation, sdk_sync_messages,
    },
    media::{
        sdk_cache_remote_media, sdk_cancel_user_file_download, sdk_clear_media_cache,
        sdk_download_file_to_downloads, sdk_get_file_download_subfolder, sdk_get_file_url,
        sdk_media_cache_stats, sdk_media_temp_download_url, sdk_path_exists,
        sdk_resolve_media_access, sdk_send_with_media_progress, sdk_set_file_download_subfolder,
        sdk_set_media_cache_max_bytes, sdk_set_media_cache_root,
        sdk_user_file_download_get_saved_path,
        sdk_user_file_download_delete_record,
    },
    message::{
        sdk_add_reaction, sdk_create_announcement, sdk_create_audio, sdk_create_card,
        sdk_create_custom, sdk_create_emoji, sdk_create_file, sdk_create_forward,
        sdk_create_image, sdk_create_image_with_thumbnail, sdk_create_link_card, sdk_create_location,
        sdk_create_mini_program, sdk_create_notification, sdk_create_placeholder,
        sdk_create_quote, sdk_create_schedule, sdk_create_sticker,
        sdk_create_system, sdk_create_task, sdk_create_text, sdk_create_thread_reply,
        sdk_create_video, sdk_create_vote, sdk_delete_message, sdk_edit,
        sdk_edit_text_by_message_id,
        sdk_get_message, sdk_get_message_raw, sdk_list_messages, sdk_mark, sdk_mark_by_message_id,
        sdk_mark_read, sdk_mark_read_with_ids, sdk_mark_with_color, sdk_pin, sdk_pin_by_message_id,
        sdk_recall, sdk_remove_reaction, sdk_search_messages, sdk_send, sdk_typing, sdk_unmark,
        sdk_unmark_by_message_id, sdk_unpin, sdk_unpin_by_message_id,
    },
    rich_doc_v2::{
        sdk_rich_doc_v2_create_message, sdk_rich_doc_v2_edit_message,
        sdk_rich_doc_v2_normalize_from_doc_json, sdk_rich_doc_v2_normalize_from_html,
        sdk_rich_doc_v2_normalize_from_markdown,
    },
};

/// 返回 IM 命令的 invoke handler。应用内使用：`.invoke_handler(flare_im_core_sdk_tauri::im_invoke_handler())`
#[must_use]
pub fn im_invoke_handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + 'static {
    tauri::generate_handler![
        sdk_save_preview_jpeg_temp,
        // lifecycle
        sdk_init,
        sdk_login,
        sdk_logout,
        sdk_is_connected,
        sdk_current_user_id,
        sdk_generate_test_token,
        sdk_engine_state,
        sdk_sync_conversation,
        sdk_sync_messages,
        sdk_mark_session_read,
        sdk_set_conversation_input_state,
        // message build + send
        sdk_create_text,
        sdk_create_quote,
        sdk_create_thread_reply,
        sdk_create_forward,
        sdk_create_image,
        sdk_create_image_with_thumbnail,
        sdk_create_video,
        sdk_create_audio,
        sdk_create_file,
        sdk_create_location,
        sdk_create_card,
        sdk_create_sticker,
        sdk_create_emoji,
        sdk_create_link_card,
        sdk_create_mini_program,
        sdk_rich_doc_v2_normalize_from_markdown,
        sdk_rich_doc_v2_normalize_from_html,
        sdk_rich_doc_v2_normalize_from_doc_json,
        sdk_rich_doc_v2_create_message,
        sdk_rich_doc_v2_edit_message,
        sdk_create_system,
        sdk_create_notification,
        sdk_create_vote,
        sdk_create_task,
        sdk_create_schedule,
        sdk_create_announcement,
        sdk_create_custom,
        sdk_create_placeholder,
        sdk_send,
        sdk_send_with_media_progress,
        sdk_recall,
        sdk_edit,
        sdk_edit_text_by_message_id,
        sdk_delete_message,
        sdk_mark_read,
        sdk_mark_read_with_ids,
        sdk_typing,
        sdk_add_reaction,
        sdk_remove_reaction,
        sdk_pin,
        sdk_unpin,
        sdk_pin_by_message_id,
        sdk_unpin_by_message_id,
        sdk_mark,
        sdk_mark_with_color,
        sdk_unmark,
        sdk_mark_by_message_id,
        sdk_unmark_by_message_id,
        sdk_get_file_url,
        sdk_media_temp_download_url,
        sdk_user_file_download_get_saved_path,
        sdk_user_file_download_delete_record,
        sdk_set_file_download_subfolder,
        sdk_get_file_download_subfolder,
        sdk_cancel_user_file_download,
        sdk_download_file_to_downloads,
        sdk_path_exists,
        sdk_resolve_media_access,
        sdk_cache_remote_media,
        sdk_media_cache_stats,
        sdk_set_media_cache_max_bytes,
        sdk_set_media_cache_root,
        sdk_clear_media_cache,
        sdk_get_message,
        sdk_get_message_raw,
        sdk_list_messages,
        sdk_search_messages,
        // conversation
        sdk_conversation_list,
        sdk_conversation_get,
        sdk_conversation_get_one,
        sdk_conversation_get_multiple,
        sdk_conversation_list_paginated,
        sdk_conversation_list_raw,
        sdk_conversation_mark_read,
        sdk_conversation_mark_all_read,
        sdk_conversation_delete,
        sdk_conversation_set_pinned,
        sdk_conversation_update_draft,
    ]
}
