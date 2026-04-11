//! 错误转换 - 统一错误模型
//!
//! 所有错误转换为 FlareError 结构

use flare_im_core_sdk::error::{ErrorCode, FlareError as SdkError};
use crate::types::{FlareError, FlareString};

/// 错误码定义
#[allow(dead_code)]
pub const FLARE_OK: i32 = 0;
pub const FLARE_ERR_INVALID_HANDLE: i32 = 1;
pub const FLARE_ERR_INVALID_PARAM: i32 = 2;
pub const FLARE_ERR_NOT_CONNECTED: i32 = 3;
pub const FLARE_ERR_OPERATION_TIMEOUT: i32 = 4;
pub const FLARE_ERR_NETWORK_ERROR: i32 = 5;
pub const FLARE_ERR_AUTH_FAILED: i32 = 6;
pub const FLARE_ERR_TOKEN_EXPIRED: i32 = 7;
pub const FLARE_ERR_INTERNAL: i32 = 8;
pub const FLARE_ERR_JSON_PARSE: i32 = 9;
pub const FLARE_ERR_NULL_POINTER: i32 = 10;
/// FFI 边界捕获到 Rust panic（禁止穿透到 C 调用方）
pub const FLARE_ERR_FFI_PANIC: i32 = 11;

/// 从 SDK ErrorCode 转换为 C ABI 错误码
#[inline]
pub fn error_code_to_c(code: ErrorCode) -> i32 {
    match code {
        ErrorCode::NotConnected => FLARE_ERR_NOT_CONNECTED,
        ErrorCode::OperationTimeout => FLARE_ERR_OPERATION_TIMEOUT,
        ErrorCode::NetworkError => FLARE_ERR_NETWORK_ERROR,
        ErrorCode::AuthenticationFailed => FLARE_ERR_AUTH_FAILED,
        ErrorCode::TokenExpired => FLARE_ERR_TOKEN_EXPIRED,
        ErrorCode::InternalError => FLARE_ERR_INTERNAL,
        ErrorCode::InvalidParameter => FLARE_ERR_INVALID_PARAM,
        _ => FLARE_ERR_INTERNAL,
    }
}

/// 从 SDK FlareError 创建 C ABI FlareError
#[inline]
pub fn make_error(err: &SdkError) -> FlareError {
    use std::fmt::Write;

    let code = err.code().map(error_code_to_c).unwrap_or(FLARE_ERR_INTERNAL);

    let mut message = String::new();
    let _ = write!(&mut message, "{}", err);

    FlareError {
        code,
        message: FlareString::from_rust_string(message),
        details_json: FlareString::from_rust_string(String::new()),
    }
}

/// 创建简单错误
#[inline]
pub fn make_simple_error(code: i32, message: &str) -> FlareError {
    FlareError {
        code,
        message: FlareString::from_rust_string(message.to_string()),
        details_json: FlareString::default(),
    }
}

/// 创建成功结果 (null error)
#[inline]
pub fn make_success() -> *const FlareError {
    std::ptr::null()
}
