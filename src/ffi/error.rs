//! C ABI 错误处理
//!
//! 提供 C 兼容的错误处理

#![allow(unsafe_code)] // FFI 需要 unsafe

use crate::shared::error::SDKError;
use std::ffi::CString;
use std::os::raw::c_char;

/// C ABI: 错误回调函数类型
pub type ErrorCallback = extern "C" fn(*mut std::ffi::c_void, *const c_char);

// 注意：结果回调函数类型直接在使用处定义

/// 将 SDKError 转换为 C 字符串
pub fn error_to_c_string(error: SDKError) -> CString {
    let error_msg = format!("{}", error);
    CString::new(error_msg).unwrap_or_else(|_| CString::new("Unknown error").unwrap())
}

/// 调用错误回调（已废弃，使用 safe::safe_call_error_callback）
#[deprecated(note = "Use safe::safe_call_error_callback instead")]
pub unsafe fn call_error_callback(
    callback: ErrorCallback,
    user_data: *mut std::ffi::c_void,
    error: SDKError,
) {
    crate::ffi::safe::safe_call_error_callback(callback, user_data, error);
}
