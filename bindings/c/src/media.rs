//! 媒体 API 模块
//!
//! 实现媒体上传下载、缓存管理等功能

use std::ffi::c_void;

use crate::callback::{invoke_result_callback, invoke_string_callback, CallbackContext, FlareResultCallback, FlareStringCallback};
use crate::error::FlareErrorCode;
use crate::handle::{get_instance, FlareImHandle};
use crate::string::parse_string;

/// 获取媒体 URL
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `media_id` - 媒体 ID
/// * `context` - 用户上下文
/// * `callback` - 字符串回调，返回媒体 URL
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_get_media_url(
    handle: FlareImHandle,
    media_id: *const i8,
    context: *mut c_void,
    callback: FlareStringCallback,
) {
    let result = get_media_url_inner(handle, media_id, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to get media url: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_string_callback(ctx, Err(e));
    }
}

fn get_media_url_inner(
    handle: FlareImHandle,
    media_id: *const i8,
    context: *mut c_void,
    callback: FlareStringCallback,
) -> Result<(), FlareErrorCode> {
    let instance = get_instance(handle)?;
    let media_id = parse_string(media_id)?;

    let ctx = CallbackContext {
        user_context: context,
        callback,
    };
    let client = instance.client.clone();

    instance.runtime.spawn(async move {
        let result = async {
            let api = client.media().map_err(|e| FlareErrorCode::from(&e))?;
            let url = api.get_url(&media_id).await.map_err(|e| FlareErrorCode::from(&e))?;
            Ok(url)
        }
        .await;
        invoke_string_callback(ctx, result);
    });

    Ok(())
}

/// 设置媒体缓存大小
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `max_bytes` - 最大字节数
/// * `context` - 用户上下文
/// * `callback` - 结果回调
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_set_media_cache_max_bytes(
    handle: FlareImHandle,
    max_bytes: u64,
    context: *mut c_void,
    callback: FlareResultCallback,
) {
    let result = set_media_cache_max_bytes_inner(handle, max_bytes, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to set media cache max bytes: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_result_callback(ctx, Err(e));
    }
}

fn set_media_cache_max_bytes_inner(
    handle: FlareImHandle,
    max_bytes: u64,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> Result<(), FlareErrorCode> {
    let instance = get_instance(handle)?;

    let ctx = CallbackContext {
        user_context: context,
        callback,
    };
    let client = instance.client.clone();

    instance.runtime.spawn(async move {
        let result = async {
            let api = client.media().map_err(|e| FlareErrorCode::from(&e))?;
            api.set_cache_max_bytes(max_bytes).await.map_err(|e| FlareErrorCode::from(&e))?;
            Ok(())
        }
        .await;
        invoke_result_callback(ctx, result);
    });

    Ok(())
}

/// 清理媒体缓存
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `context` - 用户上下文
/// * `callback` - 结果回调
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_clear_media_cache(handle: FlareImHandle, context: *mut c_void, callback: FlareResultCallback) {
    let result = clear_media_cache_inner(handle, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to clear media cache: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_result_callback(ctx, Err(e));
    }
}

fn clear_media_cache_inner(
    handle: FlareImHandle,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> Result<(), FlareErrorCode> {
    let instance = get_instance(handle)?;

    let ctx = CallbackContext {
        user_context: context,
        callback,
    };
    let client = instance.client.clone();

    instance.runtime.spawn(async move {
        let result = async {
            let api = client.media().map_err(|e| FlareErrorCode::from(&e))?;
            api.clear_cache().await.map_err(|e| FlareErrorCode::from(&e))?;
            Ok(())
        }
        .await;
        invoke_result_callback(ctx, result);
    });

    Ok(())
}
