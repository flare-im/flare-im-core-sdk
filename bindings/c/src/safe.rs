//! 安全的 FFI 包装层
//!
//! 将所有 unsafe 操作封装在内部，对外提供安全的 API

#![allow(unsafe_code)] // FFI 需要 unsafe，但已封装在安全包装层中

use flare_im_core_sdk::config::SdkConfig;
use flare_im_core_sdk::shared::error::SDKError;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};

/// 安全地从 C 字符串读取 Rust 字符串
///
/// # 安全性
/// 此函数内部使用 unsafe，但对外提供安全的 API
pub fn c_str_to_string(ptr: *const c_char) -> Result<String, String> {
    if ptr.is_null() {
        return Err("Pointer is null".to_string());
    }

    // 内部使用 unsafe，但通过错误处理保证安全
    let c_str = unsafe { CStr::from_ptr(ptr) };

    c_str
        .to_str()
        .map(|s| s.to_string())
        .map_err(|e| format!("Invalid UTF-8: {}", e))
}

/// 安全地将 Rust 字符串转换为 C 字符串
pub fn string_to_c_string(s: &str) -> Result<CString, String> {
    CString::new(s).map_err(|e| format!("Failed to create CString: {}", e))
}

/// 安全地从 JSON 字符串创建配置（已废弃，使用 client.rs 中的 parse_sdk_config_from_json）
#[deprecated(note = "Use client::parse_sdk_config_from_json instead")]
pub fn safe_config_from_json(json: *const c_char) -> Result<SdkConfig, String> {
    let json_str = c_str_to_string(json)?;
    serde_json::from_str(&json_str).map_err(|e| format!("Invalid config JSON: {}", e))
}

/// 安全地调用错误回调
pub fn safe_call_error_callback(
    callback: extern "C" fn(*mut c_void, *const c_char),
    user_data: *mut c_void,
    error: SDKError,
) {
    let error_msg = format!("{}", error);
    if let Ok(error_str) = string_to_c_string(&error_msg) {
        // 回调函数本身是 extern "C"，调用它不需要 unsafe
        callback(user_data, error_str.as_ptr());
    }
}

/// 安全地调用结果回调
pub fn safe_call_result_callback(
    callback: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    user_data: *mut c_void,
    result: Option<&str>,
    error: Option<SDKError>,
) {
    let result_str = result
        .and_then(|s| string_to_c_string(s).ok())
        .map(|s| s.as_ptr())
        .unwrap_or(std::ptr::null());

    let error_str = error
        .map(|e| format!("{}", e))
        .and_then(|s| string_to_c_string(&s).ok())
        .map(|s| s.as_ptr())
        .unwrap_or(std::ptr::null());

    // 回调函数本身是 extern "C"，调用它不需要 unsafe
    callback(user_data, result_str, error_str);
}

/// 安全地调用回调（接受字符串错误）
pub fn call_callback(
    callback: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    user_data: *mut c_void,
    result: Option<&str>,
    error: Option<&str>,
) {
    let result_str = result
        .and_then(|s| string_to_c_string(s).ok())
        .map(|s| s.as_ptr())
        .unwrap_or(std::ptr::null());

    let error_str = error
        .and_then(|s| string_to_c_string(s).ok())
        .map(|s| s.as_ptr())
        .unwrap_or(std::ptr::null());

    callback(user_data, result_str, error_str);
}

/// Send 安全的回调包装
///
/// 用于在异步闭包中安全地传递 C 回调函数和用户数据
///
/// # 安全性
/// 原始指针在 FFI 上下文中通常是 Send 的（它们只是地址），
/// 但 Rust 的默认实现不认为它们是 Send。我们使用 unsafe impl Send
/// 来标记它们是 Send 的，这在 FFI 场景中是安全的。
pub struct SendCallback {
    callback: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    user_data: *mut c_void,
}

// 对于 FFI 回调，原始指针是 Send 的（它们只是地址）
// 这是安全的，因为回调函数本身是线程安全的（extern "C" 函数）
unsafe impl Send for SendCallback {}

impl SendCallback {
    pub fn new(
        callback: extern "C" fn(*mut c_void, *const c_char, *const c_char),
        user_data: *mut c_void,
    ) -> Self {
        Self {
            callback,
            user_data,
        }
    }

    /// 调用回调函数
    pub fn call(&self, result: Option<&str>, error: Option<SDKError>) {
        safe_call_result_callback(self.callback, self.user_data, result, error);
    }
}
