//! 会话 API 模块
//!
//! 实现会话查询、操作等功能

use std::ffi::c_void;

use crate::callback::{invoke_json_callback, invoke_result_callback, CallbackContext, FlareJsonCallback, FlareResultCallback};
use crate::error::FlareErrorCode;
use crate::handle::{get_instance, FlareImHandle};
use crate::json::{to_json};
use crate::string::{parse_optional_string, parse_string};

/// 获取会话列表
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `limit` - 数量限制，0 表示使用默认值
/// * `before_id` - 起始会话 ID，NULL 表示从最新开始
/// * `context` - 用户上下文
/// * `callback` - JSON 回调，返回会话 JSON 数组
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_get_conversations(
    handle: FlareImHandle,
    limit: i32,
    before_id: *const i8,
    context: *mut c_void,
    callback: FlareJsonCallback,
) {
    let result = get_conversations_inner(handle, limit, before_id, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to get conversations: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_json_callback(ctx, Err(e));
    }
}

fn get_conversations_inner(
    handle: FlareImHandle,
    limit: i32,
    before_id: *const i8,
    context: *mut c_void,
    callback: FlareJsonCallback,
) -> Result<(), FlareErrorCode> {
    let instance = get_instance(handle)?;
    let before_id = parse_optional_string(before_id)?;

    let ctx = CallbackContext {
        user_context: context,
        callback,
    };
    let client = instance.client.clone();

    instance.runtime.spawn(async move {
        let result = async {
            let api = client.conversation().map_err(|e| FlareErrorCode::from(&e))?;
            let conversations = api.list(limit, before_id.as_deref()).await.map_err(|e| FlareErrorCode::from(&e))?;
            to_json(&conversations)
        }
        .await;
        invoke_json_callback(ctx, result);
    });

    Ok(())
}

/// 获取单个会话
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `conversation_id` - 会话 ID
/// * `context` - 用户上下文
/// * `callback` - JSON 回调，返回会话 JSON
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_get_conversation(
    handle: FlareImHandle,
    conversation_id: *const i8,
    context: *mut c_void,
    callback: FlareJsonCallback,
) {
    let result = get_conversation_inner(handle, conversation_id, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to get conversation: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_json_callback(ctx, Err(e));
    }
}

fn get_conversation_inner(
    handle: FlareImHandle,
    conversation_id: *const i8,
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
            let api = client.conversation().map_err(|e| FlareErrorCode::from(&e))?;
            let conv = api.get(&conversation_id).await.map_err(|e| FlareErrorCode::from(&e))?;
            match conv {
                Some(c) => to_json(&c),
                None => Err(FlareErrorCode::NotFound),
            }
        }
        .await;
        invoke_json_callback(ctx, result);
    });

    Ok(())
}

/// 标记会话已读
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `conversation_id` - 会话 ID
/// * `context` - 用户上下文
/// * `callback` - 结果回调
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_mark_conversation_read(
    handle: FlareImHandle,
    conversation_id: *const i8,
    context: *mut c_void,
    callback: FlareResultCallback,
) {
    let result = mark_conversation_read_inner(handle, conversation_id, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to mark conversation read: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_result_callback(ctx, Err(e));
    }
}

fn mark_conversation_read_inner(
    handle: FlareImHandle,
    conversation_id: *const i8,
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
            let api = client.conversation().map_err(|e| FlareErrorCode::from(&e))?;
            api.mark_read(&conversation_id).await.map_err(|e| FlareErrorCode::from(&e))?;
            Ok(())
        }
        .await;
        invoke_result_callback(ctx, result);
    });

    Ok(())
}

/// 设置会话置顶
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `conversation_id` - 会话 ID
/// * `pinned` - 是否置顶
/// * `context` - 用户上下文
/// * `callback` - 结果回调
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_set_conversation_pinned(
    handle: FlareImHandle,
    conversation_id: *const i8,
    pinned: bool,
    context: *mut c_void,
    callback: FlareResultCallback,
) {
    let result = set_conversation_pinned_inner(handle, conversation_id, pinned, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to set conversation pinned: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_result_callback(ctx, Err(e));
    }
}

fn set_conversation_pinned_inner(
    handle: FlareImHandle,
    conversation_id: *const i8,
    pinned: bool,
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
            let api = client.conversation().map_err(|e| FlareErrorCode::from(&e))?;
            api.set_pinned(&conversation_id, pinned).await.map_err(|e| FlareErrorCode::from(&e))?;
            Ok(())
        }
        .await;
        invoke_result_callback(ctx, result);
    });

    Ok(())
}

/// 删除会话
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `conversation_id` - 会话 ID
/// * `context` - 用户上下文
/// * `callback` - 结果回调
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_delete_conversation(
    handle: FlareImHandle,
    conversation_id: *const i8,
    context: *mut c_void,
    callback: FlareResultCallback,
) {
    let result = delete_conversation_inner(handle, conversation_id, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to delete conversation: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_result_callback(ctx, Err(e));
    }
}

fn delete_conversation_inner(
    handle: FlareImHandle,
    conversation_id: *const i8,
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
            let api = client.conversation().map_err(|e| FlareErrorCode::from(&e))?;
            api.delete(&conversation_id).await.map_err(|e| FlareErrorCode::from(&e))?;
            Ok(())
        }
        .await;
        invoke_result_callback(ctx, result);
    });

    Ok(())
}

/// 更新会话草稿
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `conversation_id` - 会话 ID
/// * `draft` - 草稿内容
/// * `context` - 用户上下文
/// * `callback` - 结果回调
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_update_conversation_draft(
    handle: FlareImHandle,
    conversation_id: *const i8,
    draft: *const i8,
    context: *mut c_void,
    callback: FlareResultCallback,
) {
    let result = update_conversation_draft_inner(handle, conversation_id, draft, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to update conversation draft: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_result_callback(ctx, Err(e));
    }
}

fn update_conversation_draft_inner(
    handle: FlareImHandle,
    conversation_id: *const i8,
    draft: *const i8,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> Result<(), FlareErrorCode> {
    let instance = get_instance(handle)?;
    let conversation_id = parse_string(conversation_id)?;
    let draft = parse_optional_string(draft)?;

    let ctx = CallbackContext {
        user_context: context,
        callback,
    };
    let client = instance.client.clone();

    instance.runtime.spawn(async move {
        let result = async {
            let api = client.conversation().map_err(|e| FlareErrorCode::from(&e))?;
            api.update_draft(&conversation_id, draft.as_deref()).await.map_err(|e| FlareErrorCode::from(&e))?;
            Ok(())
        }
        .await;
        invoke_result_callback(ctx, result);
    });

    Ok(())
}
