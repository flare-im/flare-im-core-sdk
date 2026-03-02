//! C ABI 类型定义
//!
//! 提供 C 兼容的类型转换

#![allow(unsafe_code)] // FFI 需要 unsafe

use flare_im_core_sdk::interface::facade::ImCoreSdk;
use std::ffi::CString;
use std::os::raw::c_char;

/// C ABI: SDK 句柄（不透明指针）
pub type FlareIMSDKHandle = *mut ImCoreSdk;


// 注意：此函数已废弃，直接在各处手动序列化

/// C ABI: 释放字符串（由调用者分配的内存）
///
/// # 安全性
/// 此函数内部使用 unsafe，但通过空指针检查保证安全
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        // 内部使用 unsafe 释放内存，但通过空指针检查保证安全
        let _ = unsafe { CString::from_raw(ptr) };
    }
}
