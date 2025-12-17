//! C ABI 类型定义
//!
//! 提供 C 兼容的类型转换

#![allow(unsafe_code)] // FFI 需要 unsafe

use crate::api::FlareIMClient;
use crate::shared::config::ClientConfig;
use std::ffi::CString;
use std::os::raw::c_char;

/// C ABI: 客户端句柄（不透明指针）
pub type FlareIMClientHandle = *mut FlareIMClient;

/// C ABI: 从 JSON 字符串创建配置（已废弃，使用 safe::safe_config_from_json）
#[deprecated(note = "Use safe::safe_config_from_json instead")]
pub unsafe fn config_from_json(json: *const c_char) -> Result<ClientConfig, String> {
    crate::ffi::safe::safe_config_from_json(json)
}

// 注意：此函数已废弃，直接在各处手动序列化

/// C ABI: 释放字符串（由调用者分配的内存）
///
/// # 安全性
/// 此函数内部使用 unsafe，但通过空指针检查保证安全
#[no_mangle]
pub extern "C" fn flare_im_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        // 内部使用 unsafe 释放内存，但通过空指针检查保证安全
        let _ = unsafe { CString::from_raw(ptr) };
    }
}
