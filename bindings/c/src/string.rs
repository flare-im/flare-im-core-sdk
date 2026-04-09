//! 字符串内存管理模块
//!
//! 实现跨语言字符串传递的内存管理

use std::ffi::{CStr, CString};

use crate::error::FlareErrorCode;

/// 释放字符串内存
///
/// # Safety
/// `ptr` 必须是由 Rust 分配的字符串指针（通过 CString::into_raw）
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flare_im_string_free(ptr: *const i8) {
    if !ptr.is_null() {
        // SAFETY: ptr is guaranteed to be from CString::into_raw
        unsafe {
            drop(CString::from_raw(ptr as *mut i8));
        }
    }
}

/// 释放字节数组内存
///
/// # Safety
/// `ptr` 必须是由 Rust 分配的字节数组指针
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flare_im_bytes_free(ptr: *const u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        // SAFETY: ptr is guaranteed to be from Vec::leak
        unsafe {
            drop(Vec::from_raw_parts(ptr as *mut u8, len, len));
        }
    }
}

/// 辅助：将 Rust String 转为 C 字符串（调用方需释放）
///
/// # Safety
/// 返回的指针必须通过 flare_im_string_free 释放
pub fn string_to_c(s: String) -> *const i8 {
    match CString::new(s) {
        Ok(cstr) => cstr.into_raw(),
        Err(_) => std::ptr::null(),
    }
}

/// 辅助：解析 C 字符串为 Rust String
///
/// # Safety
/// `ptr` 必须是有效的 UTF-8 字符串指针
pub fn parse_string(ptr: *const i8) -> Result<String, FlareErrorCode> {
    if ptr.is_null() {
        return Err(FlareErrorCode::InvalidParam);
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map(|s| s.to_string())
        .map_err(|_| FlareErrorCode::InvalidParam)
}

/// 辅助：解析可选 C 字符串为 Rust Option<String>
///
/// # Safety
/// `ptr` 如果不为 null，必须是有效的 UTF-8 字符串指针
pub fn parse_optional_string(ptr: *const i8) -> Result<Option<String>, FlareErrorCode> {
    if ptr.is_null() {
        return Ok(None);
    }
    parse_string(ptr).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_string_to_c_and_free() {
        let s = "Hello, World!".to_string();
        let ptr = string_to_c(s);

        assert!(!ptr.is_null());

        // 验证可以读取
        let c_str = unsafe { CStr::from_ptr(ptr) };
        assert_eq!(c_str.to_str().unwrap(), "Hello, World!");

        // 释放内存
        unsafe { flare_im_string_free(ptr) };
    }

    #[test]
    fn test_parse_string() {
        let s = "Test String";
        let c_str = CString::new(s).unwrap();
        let ptr = c_str.as_ptr();

        let result = parse_string(ptr);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), s);
    }

    #[test]
    fn test_parse_string_null() {
        let result = parse_string(std::ptr::null());
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), FlareErrorCode::InvalidParam);
    }

    #[test]
    fn test_parse_optional_string_null() {
        let result = parse_optional_string(std::ptr::null());
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_parse_optional_string_some() {
        let s = "Test";
        let c_str = CString::new(s).unwrap();
        let ptr = c_str.as_ptr();

        let result = parse_optional_string(ptr);
        assert!(result.is_ok());
        let opt = result.unwrap();
        assert!(opt.is_some());
        assert_eq!(opt.unwrap(), s);
    }
}
