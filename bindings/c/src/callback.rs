//! 回调管理模块
//!
//! 定义回调类型、实现回调上下文包装和调度

use std::ffi::{c_void, CString};

use crate::error::FlareErrorCode;

/// 结果回调（无返回值）
///
/// # Arguments
/// * `context` - 用户上下文指针
/// * `code` - 错误码，FLARE_OK 表示成功
/// * `message` - 错误消息，成功时为 NULL
pub type FlareResultCallback = extern "C" fn(*mut c_void, FlareErrorCode, *const i8);

/// 字符串回调（返回单个字符串）
///
/// # Arguments
/// * `context` - 用户上下文指针
/// * `code` - 错误码
/// * `result` - 结果字符串，需调用 flare_im_string_free 释放
pub type FlareStringCallback = extern "C" fn(*mut c_void, FlareErrorCode, *const i8);

/// JSON 回调（返回 JSON 字符串）
///
/// # Arguments
/// * `context` - 用户上下文指针
/// * `code` - 错误码
/// * `json` - JSON 结果字符串，需调用 flare_im_string_free 释放
pub type FlareJsonCallback = extern "C" fn(*mut c_void, FlareErrorCode, *const i8);

/// 字节数组回调（返回二进制数据）
///
/// # Arguments
/// * `context` - 用户上下文指针
/// * `code` - 错误码
/// * `data` - 数据指针，需调用 flare_im_bytes_free 释放
/// * `len` - 数据长度
pub type FlareBytesCallback = extern "C" fn(*mut c_void, FlareErrorCode, *const u8, usize);

/// 事件回调（推送 SDK 事件）
///
/// # Arguments
/// * `context` - 用户上下文指针
/// * `event_type` - 事件类型，如 "message.received"、"connection.connected"
/// * `event_json` - 事件 JSON 数据，需调用 flare_im_string_free 释放
pub type FlareEventCallback = extern "C" fn(*mut c_void, *const i8, *const i8);

/// 上传进度回调
///
/// # Arguments
/// * `context` - 用户上下文指针
/// * `uploaded_bytes` - 已上传字节数
/// * `total_bytes` - 总字节数
pub type FlareUploadProgressCallback = extern "C" fn(*mut c_void, u64, u64);

/// 下载进度回调
///
/// # Arguments
/// * `context` - 用户上下文指针
/// * `downloaded_bytes` - 已下载字节数
/// * `total_bytes` - 总字节数
pub type FlareDownloadProgressCallback = extern "C" fn(*mut c_void, u64, u64);

/// 回调上下文（包装用户上下文和回调函数）
pub struct CallbackContext<C> {
    pub user_context: *mut c_void,
    pub callback: C,
}

// 安全：回调上下文跨线程传递
unsafe impl<C> Send for CallbackContext<C> {}
unsafe impl<C> Sync for CallbackContext<C> {}

/// 调用结果回调
pub fn invoke_result_callback(ctx: CallbackContext<FlareResultCallback>, result: Result<(), FlareErrorCode>) {
    let (code, msg) = match result {
        Ok(()) => (FlareErrorCode::Ok, std::ptr::null()),
        Err(e) => {
            let msg = CString::new(e.to_string()).unwrap().into_raw();
            (e, msg)
        }
    };
    (ctx.callback)(ctx.user_context, code, msg);
}

/// 调用字符串回调
pub fn invoke_string_callback(ctx: CallbackContext<FlareStringCallback>, result: Result<String, FlareErrorCode>) {
    let (code, value) = match result {
        Ok(s) => {
            let ptr = CString::new(s).unwrap().into_raw();
            (FlareErrorCode::Ok, ptr)
        }
        Err(e) => (e, std::ptr::null()),
    };
    (ctx.callback)(ctx.user_context, code, value);
}

/// 调用 JSON 回调
pub fn invoke_json_callback(ctx: CallbackContext<FlareJsonCallback>, result: Result<String, FlareErrorCode>) {
    let (code, json) = match result {
        Ok(s) => {
            let ptr = CString::new(s).unwrap().into_raw();
            (FlareErrorCode::Ok, ptr)
        }
        Err(e) => (e, std::ptr::null()),
    };
    (ctx.callback)(ctx.user_context, code, json);
}

/// 调用字节数组回调
pub fn invoke_bytes_callback(
    ctx: CallbackContext<FlareBytesCallback>,
    result: Result<(Vec<u8>, usize), FlareErrorCode>,
) {
    let (code, data, len) = match result {
        Ok((bytes, len)) => {
            let ptr = bytes.leak().as_ptr();
            (FlareErrorCode::Ok, ptr, len)
        }
        Err(e) => (e, std::ptr::null(), 0),
    };
    (ctx.callback)(ctx.user_context, code, data, len);
}
