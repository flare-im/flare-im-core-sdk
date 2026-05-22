//! 消息 API - 消息构建、发送、查询、操作
//!
//! 透传 `MessageBuildApi` / `MessageApi`，无业务分支。

use std::ffi::{c_char, c_void};

use crate::abi;
use crate::dispatch::send_ack_to_json;
use crate::executor::{CallbackContext, execute_async, execute_async_unit, return_error};
use crate::helpers::{c_str_to_string, parse_json, to_json_string};
use crate::registry::require_instance;
use crate::types::{FlareHandle, FlareResultCallback};

#[unsafe(no_mangle)]
pub extern "C" fn flare_message_create_text(
    handle: FlareHandle,
    conversation_id: *const c_char,
    text: *const c_char,
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

        let text = match c_str_to_string(text) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid text");
                return code;
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();

        execute_async(
            instance,
            ctx,
            async move {
                let build_api = inst.message_build_api().await?;
                build_api.create_text(&conversation_id, &text).await
            },
            |msg| to_json_string(&msg),
        );

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_message_send(
    handle: FlareHandle,
    message_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let message: flare_im_core_sdk::model::IMMessage = match parse_json(message_json) {
            Ok(m) => m,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid message JSON");
                return code;
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();

        execute_async(
            instance,
            ctx,
            async move {
                let api = inst.message_api().await?;
                api.send(message).await
            },
            |send_ack| send_ack_to_json(&send_ack),
        );

        0
    })
}

/// `before_seq == 0`：打开会话首屏，拉取本地最新一页；翻页时传当前已展示批次中的最小 `seq`。
#[unsafe(no_mangle)]
pub extern "C" fn flare_message_list(
    handle: FlareHandle,
    conversation_id: *const c_char,
    before_seq: u64,
    limit: i32,
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
                let api = inst.message_api().await?;
                api.list(&conversation_id, before_seq, limit as u32).await
            },
            |messages| to_json_string(&messages),
        );

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_message_recall(
    handle: FlareHandle,
    conversation_id: *const c_char,
    message_id: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let _conversation_id = match c_str_to_string(conversation_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid conversation_id");
                return code;
            }
        };

        let message_id = match c_str_to_string(message_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid message_id");
                return code;
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();

        execute_async_unit(instance, ctx, async move {
            let api = inst.message_api().await?;
            api.recall(&message_id).await
        });

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_message_delete(
    handle: FlareHandle,
    conversation_id: *const c_char,
    message_id: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let _conversation_id = match c_str_to_string(conversation_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid conversation_id");
                return code;
            }
        };

        let message_id = match c_str_to_string(message_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid message_id");
                return code;
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();

        execute_async_unit(instance, ctx, async move {
            let api = inst.message_api().await?;
            api.delete(&message_id).await
        });

        0
    })
}
