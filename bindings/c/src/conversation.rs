//! 会话 API - 会话查询、操作
//!
//! 透传 `ConversationApi`。

use std::ffi::{c_char, c_void};

use flare_im_core_sdk::model::ConversationListQuery;
use flare_im_core_sdk::model::conversation::ConversationType as SdkConversationType;

use crate::abi;
use crate::error_convert::FLARE_ERR_INVALID_PARAM;
use crate::executor::{CallbackContext, execute_async, execute_async_unit, return_error};
use crate::helpers::{c_str_to_string, parse_json, to_json_string};
use crate::registry::require_instance;
use crate::types::{FlareHandle, FlareResultCallback};

#[unsafe(no_mangle)]
pub extern "C" fn flare_conversation_list(
    handle: FlareHandle,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();

        execute_async(
            instance,
            ctx,
            async move {
                let api = inst.conversation_api().await?;
                api.list().await
            },
            |conversations| to_json_string(&conversations),
        );

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_conversation_list_by_query_json(
    handle: FlareHandle,
    query_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let query_value = match parse_json(query_json) {
            Ok(v) => v,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid query JSON");
                return code;
            }
        };
        let query: ConversationListQuery = match serde_json::from_value(query_value) {
            Ok(q) => q,
            Err(_) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, FLARE_ERR_INVALID_PARAM, "Invalid query");
                return FLARE_ERR_INVALID_PARAM;
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();

        execute_async(
            instance,
            ctx,
            async move {
                let api = inst.conversation_api().await?;
                api.list_by_query(query).await
            },
            |conversations| to_json_string(&conversations),
        );

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_conversation_get(
    handle: FlareHandle,
    conversation_id: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let conversation_id = match c_str_to_string(conversation_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid conversation_id");
                return code;
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();

        execute_async(
            instance,
            ctx,
            async move {
                let api = inst.conversation_api().await?;
                api.get(&conversation_id).await
            },
            |conversation| match conversation {
                Some(c) => to_json_string(&c),
                None => Ok("null".to_string()),
            },
        );

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_conversation_mark_read(
    handle: FlareHandle,
    conversation_id: *const c_char,
    read_seq: u64,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let conversation_id = match c_str_to_string(conversation_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid conversation_id");
                return code;
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();

        execute_async_unit(instance, ctx, async move {
            let api = inst.conversation_api().await?;
            api.mark_read(&conversation_id, read_seq).await
        });

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_conversation_set_pinned(
    handle: FlareHandle,
    conversation_id: *const c_char,
    pinned: bool,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let conversation_id = match c_str_to_string(conversation_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid conversation_id");
                return code;
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();

        execute_async_unit(instance, ctx, async move {
            let api = inst.conversation_api().await?;
            api.set_pinned(&conversation_id, pinned).await
        });

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_conversation_delete(
    handle: FlareHandle,
    conversation_id: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let conversation_id = match c_str_to_string(conversation_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid conversation_id");
                return code;
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();

        execute_async_unit(instance, ctx, async move {
            let api = inst.conversation_api().await?;
            api.delete(&conversation_id).await
        });

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_conversation_update_draft(
    handle: FlareHandle,
    conversation_id: *const c_char,
    draft: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let conversation_id = match c_str_to_string(conversation_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid conversation_id");
                return code;
            }
        };

        let draft = match abi::read_c_str_opt(draft) {
            Ok(o) => o,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid draft");
                return code;
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();

        execute_async_unit(instance, ctx, async move {
            let api = inst.conversation_api().await?;
            api.update_draft(&conversation_id, draft.as_deref()).await
        });

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_conversation_get_one(
    handle: FlareHandle,
    source_id: *const c_char,
    conversation_type_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let source_id = match c_str_to_string(source_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid source_id");
                return code;
            }
        };
        let conv_type: SdkConversationType = match parse_json(conversation_type_json) {
            Ok(t) => t,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid conversation_type JSON");
                return code;
            }
        };
        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();
        execute_async(
            instance,
            ctx,
            async move {
                let api = inst.conversation_api().await?;
                api.get_one(&source_id, &conv_type).await
            },
            |c| to_json_string(&c),
        );
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_conversation_mark_all_read(
    handle: FlareHandle,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();
        execute_async_unit(instance, ctx, async move {
            let api = inst.conversation_api().await?;
            api.mark_all_read().await
        });
        0
    })
}
