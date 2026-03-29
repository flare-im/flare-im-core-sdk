//! 统一错误类型 — 基于 flare-core `common::error`，与框架一致
//!
//! 本 crate 直接使用 `FlareError` 与 `Result`，不再定义独立 `SdkError`。
//! 支持从 proto `RpcStatus`（code + message）映射为 `FlareError`。

pub use flare_core::common::error::LocalizedError;
pub use flare_core::common::error::{
    ClientError, ErrorBuilder, ErrorCode, FlareError, Result, ServerError,
};

/// 从 proto RpcStatus 的 code + message 映射为 FlareError
///
/// 与 `errors.proto` 中 ErrorCode 对齐；未命中时使用 `message` 构造 GeneralError。
///
/// # Example
///
/// ```ignore
/// if let Some(status) = response.status {
///     if status.code != ERROR_CODE_OK {
///         return Err(from_rpc_status(status.code, status.message));
///     }
/// }
/// ```
#[inline]
pub fn from_rpc_status(code: i32, message: impl Into<String>) -> FlareError {
    use flare_core::common::error::ErrorCode as EC;
    let msg = message.into();
    let code = match code {
        2 => return FlareError::general_error("操作已取消"),
        10 | 12 => EC::InvalidParameter,
        11 => EC::InvalidParameter,
        13 => EC::OperationNotSupported,
        20 => EC::AuthenticationFailed,
        21 => EC::PermissionDenied,
        22 | 23 => EC::TokenExpired,
        30 => EC::UserNotFound,
        31 | 32 => EC::GeneralError,
        33 | 34 => EC::ResourceExhausted,
        40 | 41 => EC::MessageRateLimitExceeded,
        42 | 43 => EC::OperationTimeout,
        50 => EC::OperationTimeout,
        51 | 52 => EC::ServiceUnavailable,
        60 => EC::InternalError,
        61 | 62 => EC::OperationNotSupported,
        _ => return FlareError::localized(EC::GeneralError, msg),
    };
    FlareError::localized(code, msg)
}
