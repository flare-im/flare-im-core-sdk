//! 错误转换 - 统一错误模型
//!
//! 稳定错误码由 `contract/errors.json` 生成；本模块仅保留 FlareError 构造辅助。

pub use crate::generated::errors::*;

use crate::types::{FlareError, FlareString};
use flare_im_core_sdk::FlareError as SdkError;

/// 从 SDK FlareError 创建 C ABI FlareError
#[inline]
pub fn make_error(err: &SdkError) -> FlareError {
    use std::fmt::Write;

    let code = err
        .code()
        .map(error_code_to_c)
        .unwrap_or(FLARE_ERR_INTERNAL);

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
