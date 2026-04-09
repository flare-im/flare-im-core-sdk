//! C ABI 错误码定义与映射
//!
//! 提供从 Rust SDK 错误到 C ABI 错误码的映射

use flare_im_core_sdk::error::ErrorCode;

/// C ABI 错误码
#[repr(C, i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlareErrorCode {
    /// 成功
    Ok = 0,

    // 连接错误 (1xxx)
    /// 未连接
    NotConnected = 1001,
    /// 连接失败
    ConnectionFailed = 1002,
    /// 连接超时
    ConnectionTimeout = 1003,

    // 参数错误 (2xxx)
    /// 无效参数
    InvalidParam = 2001,
    /// 无效句柄
    InvalidHandle = 2002,
    /// 无效 JSON
    InvalidJson = 2003,

    // 网络错误 (3xxx)
    /// 网络错误
    Network = 3001,
    /// 超时
    Timeout = 3002,

    // 认证错误 (4xxx)
    /// 未授权
    Unauthorized = 4001,
    /// Token 过期
    TokenExpired = 4002,
    /// 被踢下线
    KickedOff = 4003,

    // 存储错误 (5xxx)
    /// 存储错误
    Storage = 5001,
    /// 未找到
    NotFound = 5002,

    // 内部错误 (9xxx)
    /// 内部错误
    InternalError = 9001,
    /// 未知错误
    Unknown = 9999,
}

impl Default for FlareErrorCode {
    fn default() -> Self {
        Self::Unknown
    }
}

impl std::fmt::Display for FlareErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok => write!(f, "Success"),
            Self::NotConnected => write!(f, "Not connected"),
            Self::ConnectionFailed => write!(f, "Connection failed"),
            Self::ConnectionTimeout => write!(f, "Connection timeout"),
            Self::InvalidParam => write!(f, "Invalid parameter"),
            Self::InvalidHandle => write!(f, "Invalid handle"),
            Self::InvalidJson => write!(f, "Invalid JSON"),
            Self::Network => write!(f, "Network error"),
            Self::Timeout => write!(f, "Timeout"),
            Self::Unauthorized => write!(f, "Unauthorized"),
            Self::TokenExpired => write!(f, "Token expired"),
            Self::KickedOff => write!(f, "Kicked off"),
            Self::Storage => write!(f, "Storage error"),
            Self::NotFound => write!(f, "Not found"),
            Self::InternalError => write!(f, "Internal error"),
            Self::Unknown => write!(f, "Unknown error"),
        }
    }
}

/// 从 SDK ErrorCode 映射到 C ABI 错误码
impl From<ErrorCode> for FlareErrorCode {
    fn from(code: ErrorCode) -> Self {
        match code {
            ErrorCode::NotConnected => Self::NotConnected,
            ErrorCode::InvalidParameter => Self::InvalidParam,
            ErrorCode::AuthenticationFailed => Self::Unauthorized,
            ErrorCode::TokenExpired => Self::TokenExpired,
            ErrorCode::NetworkError => Self::Network,
            ErrorCode::OperationTimeout => Self::Timeout,
            ErrorCode::InternalError => Self::InternalError,
            ErrorCode::UserNotFound => Self::NotFound,
            ErrorCode::ResourceExhausted => Self::Storage,
            ErrorCode::ServiceUnavailable => Self::ConnectionFailed,
            ErrorCode::PermissionDenied => Self::Unauthorized,
            ErrorCode::OperationNotSupported => Self::InvalidParam,
            ErrorCode::MessageRateLimitExceeded => Self::Network,
            ErrorCode::GeneralError => Self::InternalError,
            _ => Self::Unknown,
        }
    }
}

/// 从 FlareError 映射
impl From<&flare_im_core_sdk::error::FlareError> for FlareErrorCode {
    fn from(err: &flare_im_core_sdk::error::FlareError) -> Self {
        err.code().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_code_default() {
        let code = FlareErrorCode::default();
        assert_eq!(code, FlareErrorCode::Unknown);
    }

    #[test]
    fn test_error_code_display() {
        assert_eq!(format!("{}", FlareErrorCode::Ok), "Success");
        assert_eq!(format!("{}", FlareErrorCode::NotConnected), "Not connected");
        assert_eq!(format!("{}", FlareErrorCode::InvalidParam), "Invalid parameter");
    }

    #[test]
    fn test_error_code_from_sdk_error() {
        let sdk_code = ErrorCode::NotConnected;
        let c_code: FlareErrorCode = sdk_code.into();
        assert_eq!(c_code, FlareErrorCode::NotConnected);

        let sdk_code = ErrorCode::InvalidParameter;
        let c_code: FlareErrorCode = sdk_code.into();
        assert_eq!(c_code, FlareErrorCode::InvalidParam);
    }
}
