//! 统一错误类型 — 基于 flare-core `common::error`，与框架一致
//!
//! 本 crate 直接使用 `FlareError` 与 `Result`，不再定义独立 `SdkError`。
//! 支持从 proto `RpcStatus`（code + message）映射为 `FlareError`。

pub use flare_core::common::error::LocalizedError;
pub use flare_core::common::error::{
    ClientError, ErrorBuilder, ErrorCode, FlareError, Result, ServerError,
};

/// Core 因租户不可用（停用、删除中）拒绝建连、踢线或拒绝请求时给出的类型化原因。
pub const TENANT_UNAVAILABLE: &str = "TENANT_UNAVAILABLE";

/// 同一账号在同一平台的新设备登录，本设备被踢下线时的类型化原因（「已在别处登录」）。
///
/// SDK 收到它就把 `KickedOff { reason: "DEVICE_CONFLICT" }` 交给应用并终止会话：不自动重连
/// （连回去会把新设备再顶下线），恢复要由应用重新登录。
pub const DEVICE_CONFLICT: &str = flare_core::common::kick::DEVICE_CONFLICT_REASON;

/// 建连 / 重连错误是否表示租户不可用。
///
/// 按错误码识别：网关以 `TENANT_UNAVAILABLE` 拒绝 CONNECT 时，flare-core 把它映射为
/// `PERMISSION_DENIED`（`connect_rejection_error`），socket 层原样保留这个错误码；网关拒绝 CONNECT
/// 不会因为别的原因用 `PERMISSION_DENIED`。以前在错误文本里找 `TENANT_UNAVAILABLE`，地址、服务端
/// 文案里碰巧带上这几个字就会误判，换了措辞又会漏判。
///
/// 它不是鉴权失败：换令牌、自动重连都无济于事，只会反复打到网关。SDK 据此停止重连并把
/// `KickedOff { reason: "TENANT_UNAVAILABLE" }` 交给应用。
pub fn is_tenant_unavailable(err: &FlareError) -> bool {
    err.code() == Some(ErrorCode::PermissionDenied)
}

/// 从 proto RpcStatus 的 code + message 映射为 FlareError
///
/// 与 `errors.proto` 中 ErrorCode 对齐；未命中时使用 `message` 构造 GeneralError。
///
/// # Example
///
/// ```ignore
/// return Err(from_rpc_status(rpc_status.code, rpc_status.message));
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
