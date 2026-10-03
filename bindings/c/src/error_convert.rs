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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::flare_error_free;
    use flare_im_core_sdk::ErrorCode;

    fn c_code_of(err: &SdkError) -> i32 {
        let converted = make_error(err);
        let code = converted.code;
        flare_error_free(converted);
        code
    }

    /// 连接建立失败、连接被关闭、网络连接丢失都是网络类失败：经 C ABI 交给宿主时是
    /// `FLARE_ERR_NETWORK_ERROR`，宿主据此提示「网络异常、可重试」。原先它们没有映射，落到
    /// `FLARE_ERR_INTERNAL`，与内部错误混在一起。
    #[test]
    fn connection_and_network_loss_errors_map_to_network_error() {
        for err in [
            SdkError::connection_failed("dial failed"),
            SdkError::connection_closed("peer closed"),
            SdkError::localized(ErrorCode::NetworkConnectionLost, "link lost"),
            SdkError::localized(ErrorCode::NetworkError, "network error"),
        ] {
            assert_eq!(c_code_of(&err), FLARE_ERR_NETWORK_ERROR, "{err:?}");
        }
        assert_eq!(
            c_code_of(&SdkError::localized(ErrorCode::InternalError, "bug")),
            FLARE_ERR_INTERNAL
        );
        assert_eq!(c_code_of(&SdkError::system("no code")), FLARE_ERR_INTERNAL);
    }

    /// 生成的映射与 `contract/errors.json` 逐条一致：契约里每个 `coreCodes` 都是真实的错误码，
    /// 且 `error_code_to_c` 把它映射到所在条目的 C 码。改了契约却没重新生成会在这里失败。
    #[test]
    fn generated_mapping_matches_the_error_contract() {
        let contract: serde_json::Value =
            serde_json::from_str(include_str!("../../contract/errors.json"))
                .expect("errors.json parses");
        let mut checked = 0;
        for entry in contract["cAbi"]["codes"].as_array().expect("cAbi.codes") {
            let c_code = entry["code"].as_i64().expect("code") as i32;
            for core in entry["coreCodes"].as_array().into_iter().flatten() {
                let name = core.as_str().expect("core code name");
                let mut wire = String::new();
                for (i, ch) in name.chars().enumerate() {
                    if ch.is_ascii_uppercase() && i > 0 {
                        wire.push('_');
                    }
                    wire.push(ch.to_ascii_uppercase());
                }
                let code: ErrorCode = serde_json::from_value(serde_json::Value::String(wire))
                    .unwrap_or_else(|e| panic!("{name} is not a core ErrorCode: {e}"));
                assert_eq!(error_code_to_c(code), c_code, "{name}");
                checked += 1;
            }
        }
        assert!(checked >= 15, "only {checked} core codes in the contract");
    }
}
