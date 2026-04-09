//! 消息 API 模块
//!
//! 实现消息构建、发送、查询、操作等功能

use std::ffi::c_void;

use flare_im_core_sdk::model::IMMessage;

use crate::callback::{invoke_json_callback, invoke_result_callback, invoke_string_callback, CallbackContext, FlareJsonCallback, FlareResultCallback, FlareStringCallback};
use crate::error::FlareErrorCode;
use crate::handle::{get_instance, FlareImHandle};
use crate::json::{parse_json, to_json};
use crate::string::parse_string;

/// 创建文本消息
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `conversation_id` - 会话 ID
/// * `text` - 文本内容
/// * `context` - 用户上下文
/// * `callback` - JSON 回调，返回消息 JSON
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_create_text_message(
    handle: FlareImHandle,
    conversation_id: *const i8,
    text: *const i8,
    context: *mut c_void,
    callback: FlareJsonCallback,
) {
    let result = create_text_message_inner(handle, conversation_id, text, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to create text message: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_json_callback(ctx, Err(e));
    }
}

fn create_text_message_inner(
    handle: FlareImHandle,
    conversation_id: *const i8,
    text: *const i8,
    context: *mut c_void,
    callback: FlareJsonCallback,
) -> Result<(), FlareErrorCode> {
    let instance = get_instance(handle)?;
    let conversation_id = parse_string(conversation_id)?;
    let text = parse_string(text)?;

    let ctx = CallbackContext {
        user_context: context,
        callback,
    };
    let client = instance.client.clone();

    instance.runtime.spawn(async move {
        let result = async {
            let build_api = client.message_build().map_err(|e| FlareErrorCode::from(&e))?;
            let msg = build_api.create_text(&conversation_id, &text).await.map_err(|e| FlareErrorCode::from(&e))?;
            to_json(&msg)
        }
        .await;
        invoke_json_callback(ctx, result);
    });

    Ok(())
}

/// 发送消息
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `message_json` - 消息 JSON，由 create_*_message 返回
/// * `context` - 用户上下文
/// * `callback` - 字符串回调，返回 client_msg_id
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_send_message(
    handle: FlareImHandle,
    message_json: *const i8,
    context: *mut c_void,
    callback: FlareStringCallback,
) {
    let result = send_message_inner(handle, message_json, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to send message: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_string_callback(ctx, Err(e));
    }
}

fn send_message_inner(
    handle: FlareImHandle,
    message_json: *const i8,
    context: *mut c_void,
    callback: FlareStringCallback,
) -> Result<(), FlareErrorCode> {
    let instance = get_instance(handle)?;
    let message: IMMessage = parse_json(message_json)?;

    let ctx = CallbackContext {
        user_context: context,
        callback,
    };
    let client = instance.client.clone();

    instance.runtime.spawn(async move {
        let result = async {
            let api = client.message().map_err(|e| FlareErrorCode::from(&e))?;
            let client_msg_id = api.send(message).await.map_err(|e| FlareErrorCode::from(&e))?;
            Ok(client_msg_id)
        }
        .await;
        invoke_string_callback(ctx, result);
    });

    Ok(())
}

/// 获取消息
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `conversation_id` - 会话 ID
/// * `message_id` - 消息 ID（server_msg_id 或 client_msg_id）
/// * `context` - 用户上下文
/// * `callback` - JSON 回调，返回消息 JSON
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_get_message(
    handle: FlareImHandle,
    conversation_id: *const i8,
    message_id: *const i8,
    context: *mut c_void,
    callback: FlareJsonCallback,
) {
    let result = get_message_inner(handle, conversation_id, message_id, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to get message: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_json_callback(ctx, Err(e));
    }
}

fn get_message_inner(
    handle: FlareImHandle,
    conversation_id: *const i8,
    message_id: *const i8,
    context: *mut c_void,
    callback: FlareJsonCallback,
) -> Result<(), FlareErrorCode> {
    let instance = get_instance(handle)?;
    let conversation_id = parse_string(conversation_id)?;
    let message_id = parse_string(message_id)?;

    let ctx = CallbackContext {
        user_context: context,
        callback,
    };
    let client = instance.client.clone();

    instance.runtime.spawn(async move {
        let result = async {
            let api = client.message().map_err(|e| FlareErrorCode::from(&e))?;
            let msg = api.get(&conversation_id, &message_id).await.map_err(|e| FlareErrorCode::from(&e))?;
            match msg {
                Some(m) => to_json(&m),
                None => Err(FlareErrorCode::NotFound),
            }
        }
        .await;
        invoke_json_callback(ctx, result);
    });

    Ok(())
}

/// 获取消息列表
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `conversation_id` - 会话 ID
/// * `before_seq` - 起始序列号，0 表示从最新开始
/// * `limit` - 数量限制
/// * `context` - 用户上下文
/// * `callback` - JSON 回调，返回消息 JSON 数组
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_list_messages(
    handle: FlareImHandle,
    conversation_id: *const i8,
    before_seq: u64,
    limit: i32,
    context: *mut c_void,
    callback: FlareJsonCallback,
) {
    let result = list_messages_inner(handle, conversation_id, before_seq, limit, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to list messages: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_json_callback(ctx, Err(e));
    }
}

fn list_messages_inner(
    handle: FlareImHandle,
    conversation_id: *const i8,
    before_seq: u64,
    limit: i32,
    context: *mut c_void,
    callback: FlareJsonCallback,
) -> Result<(), FlareErrorCode> {
    let instance = get_instance(handle)?;
    let conversation_id = parse_string(conversation_id)?;

    let ctx = CallbackContext {
        user_context: context,
        callback,
    };
    let client = instance.client.clone();

    instance.runtime.spawn(async move {
        let result = async {
            let api = client.message().map_err(|e| FlareErrorCode::from(&e))?;
            let messages = api.list(&conversation_id, before_seq, limit).await.map_err(|e| FlareErrorCode::from(&e))?;
            to_json(&messages)
        }
        .await;
        invoke_json_callback(ctx, result);
    });

    Ok(())
}

/// 撤回消息
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `conversation_id` - 会话 ID
/// * `message_id` - 消息 ID
/// * `context` - 用户上下文
/// * `callback` - 结果回调
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_recall_message(
    handle: FlareImHandle,
    conversation_id: *const i8,
    message_id: *const i8,
    context: *mut c_void,
    callback: FlareResultCallback,
) {
    let result = recall_message_inner(handle, conversation_id, message_id, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to recall message: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_result_callback(ctx, Err(e));
    }
}

fn recall_message_inner(
    handle: FlareImHandle,
    conversation_id: *const i8,
    message_id: *const i8,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> Result<(), FlareErrorCode> {
    let instance = get_instance(handle)?;
    let conversation_id = parse_string(conversation_id)?;
    let message_id = parse_string(message_id)?;

    let ctx = CallbackContext {
        user_context: context,
        callback,
    };
    let client = instance.client.clone();

    instance.runtime.spawn(async move {
        let result = async {
            let api = client.message().map_err(|e| FlareErrorCode::from(&e))?;
            api.recall(&conversation_id, &message_id).await.map_err(|e| FlareErrorCode::from(&e))?;
            Ok(())
        }
        .await;
        invoke_result_callback(ctx, result);
    });

    Ok(())
}

/// 删除消息
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `conversation_id` - 会话 ID
/// * `message_id` - 消息 ID
/// * `context` - 用户上下文
/// * `callback` - 结果回调
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_delete_message(
    handle: FlareImHandle,
    conversation_id: *const i8,
    message_id: *const i8,
    context: *mut c_void,
    callback: FlareResultCallback,
) {
    let result = delete_message_inner(handle, conversation_id, message_id, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to delete message: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_result_callback(ctx, Err(e));
    }
}

fn delete_message_inner(
    handle: FlareImHandle,
    conversation_id: *const i8,
    message_id: *const i8,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> Result<(), FlareErrorCode> {
    let instance = get_instance(handle)?;
    let conversation_id = parse_string(conversation_id)?;
    let message_id = parse_string(message_id)?;

    let ctx = CallbackContext {
        user_context: context,
        callback,
    };
    let client = instance.client.clone();

    instance.runtime.spawn(async move {
        let result = async {
            let api = client.message().map_err(|e| FlareErrorCode::from(&e))?;
            api.delete(&conversation_id, &message_id).await.map_err(|e| FlareErrorCode::from(&e))?;
            Ok(())
        }
        .await;
        invoke_result_callback(ctx, result);
    });

    Ok(())
}

/// 标记已读
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `conversation_id` - 会话 ID
/// * `read_seq` - 已读序列号
/// * `context` - 用户上下文
/// * `callback` - 结果回调
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_mark_read(
    handle: FlareImHandle,
    conversation_id: *const i8,
    read_seq: u64,
    context: *mut c_void,
    callback: FlareResultCallback,
) {
    let result = mark_read_inner(handle, conversation_id, read_seq, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to mark read: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_result_callback(ctx, Err(e));
    }
}

fn mark_read_inner(
    handle: FlareImHandle,
    conversation_id: *const i8,
    read_seq: u64,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> Result<(), FlareErrorCode> {
    let instance = get_instance(handle)?;
    let conversation_id = parse_string(conversation_id)?;

    let ctx = CallbackContext {
        user_context: context,
        callback,
    };
    let client = instance.client.clone();

    instance.runtime.spawn(async move {
        let result = async {
            let api = client.message().map_err(|e| FlareErrorCode::from(&e))?;
            api.mark_read(&conversation_id, read_seq).await.map_err(|e| FlareErrorCode::from(&e))?;
            Ok(())
        }
        .await;
        invoke_result_callback(ctx, result);
    });

    Ok(())
}
