//! Flare IM SDK - C ABI Bindings
//!
//! 跨平台 C ABI SDK,支持 iOS、Android、Flutter、鸿蒙、C/C++、Node、Unity
//!
//! # 架构原则
//!
//! - Rust 内部复杂,C ABI 外部简单
//! - 所有对象通过 handle 管理
//! - 异步 API 使用 callback
//! - 统一 error code 错误模型
//! - 显式内存管理
//!
//! # 线程安全
//!
//! - callback 可能来自任意线程
//! - 禁止在 callback 中阻塞或持锁
//! - 所有 API 线程安全

// 核心模块
mod abi;
mod error_convert;
mod executor;
mod ffi_runtime;
mod helpers;
mod registry;
mod session;
mod types;

// API 模块
mod client_sync;
mod conversation;
mod dispatch;
mod event;
mod lifecycle;
mod media;
mod message;

// 重新导出公开类型
pub use types::{
    FlareBytes, FlareBytesView, FlareError, FlareHandle, FlareProgressCallback,
    FlareResultCallback, FlareString, FlareStringView, FlareSubscriptionHandle, FlareTaskHandle,
};

// 重新导出生命周期 API
pub use lifecycle::{
    flare_sdk_create, flare_sdk_current_user_id, flare_sdk_data_root,
    flare_sdk_ffi_contract_version,
    flare_sdk_generate_test_token, flare_sdk_hard_reset, flare_sdk_init, flare_sdk_is_connected,
    flare_sdk_login, flare_sdk_logout, flare_sdk_release, flare_sdk_session_active,
    flare_sdk_uninit, flare_sdk_version,
};

// 与 IMClient 直接方法对齐（状态、断开、同步、输入态等）
pub use client_sync::{
    flare_sdk_batch_get_user_presence, flare_sdk_disconnect, flare_sdk_get_user_presence,
    flare_sdk_mark_session_read, flare_sdk_set_conversation_input_state, flare_sdk_state,
    flare_sdk_subscribe_user_presence, flare_sdk_sync_conversation, flare_sdk_sync_messages,
};

// 重新导出消息 API
pub use message::{
    flare_message_create_text, flare_message_delete, flare_message_list, flare_message_recall,
    flare_message_send,
};

// MessageApi / MessageBuildApi 其余能力：JSON 分发（op + params）
pub use dispatch::{flare_message_build_json, flare_message_dispatch_json};

// 重新导出会话 API
pub use conversation::{
    flare_conversation_delete, flare_conversation_get, flare_conversation_get_one,
    flare_conversation_list, flare_conversation_list_by_query_json,
    flare_conversation_mark_all_read, flare_conversation_mark_read, flare_conversation_set_pinned,
    flare_conversation_update_draft,
};

// 重新导出媒体 API
pub use media::{
    flare_media_cache_remote, flare_media_cache_stats, flare_media_clear_cache,
    flare_media_get_url, flare_media_resolve_access, flare_media_set_cache_max_bytes,
    flare_media_set_cache_root,
};

// 重新导出事件 API
pub use event::{flare_event_subscribe, flare_event_unsubscribe, flare_event_unsubscribe_all};

// 重新导出内存管理 API
pub use helpers::{flare_bytes_free, flare_error_free, flare_error_heap_free, flare_string_free};

/// 初始化日志系统
///
/// 应在首次使用 SDK 前调用
#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_init_logging() {
    abi::catch_ffi_void(|| {
        let _ = tracing_subscriber::fmt::try_init();
    });
}
