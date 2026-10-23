pub mod http_client;
#[cfg(target_arch = "wasm32")]
pub mod http_client_wasm;
pub mod media_dto;
pub use http_client::{CoreBearerRenewal, HttpClient, HttpRequestContext, SentBytesCallback};
pub use media_dto::{
    AbortDirectUploadHttpRequest, CommitDirectUploadPartsHttpRequest,
    CommitDirectUploadPartsHttpResponse, CompleteDirectUploadHttpRequest, DeleteFileHttpRequest,
    DeleteFileHttpResponse, DirectUploadTransportKindHttp, FileInfoHttpResponse,
    GetDirectUploadStatusHttpResponse, GetFileUrlHttpRequest, GetFileUrlHttpResponse,
    InitiateDirectUploadHttpRequest, InitiateDirectUploadHttpResponse,
    PresignDirectUploadPartsHttpRequest, PresignDirectUploadPartsHttpResponse,
    PresignedUploadPartHttp, UploadFileHttpResponse, UploadFileMetadataHttp, UploadedPartInfoHttp,
};

use crate::shared::error::{ErrorCode, FlareError, Result};

/// SDK 统一 HTTP 包装响应（与 gateway ApiResponse 对齐）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HttpApiResponse<T> {
    pub code: i32,
    pub data: Option<T>,
    pub reason: Option<String>,
    pub message: Option<String>,
}

impl<T> HttpApiResponse<T> {
    #[inline]
    pub fn is_success(&self) -> bool {
        self.code == 0
    }
}

/// 非 2xx 时保留兼容的诊断文案，并提供与文案无关的 HTTP 错误元数据。
///
/// 机器元数据仅包含状态码、远端数字码和有界的 reason 标识，不携带响应正文、
/// message 或 HTTP headers。跨语言调用方不能凭“发生了异常”推断权限拒绝。
pub fn http_error_from_response_status(status: u16, body: &str) -> FlareError {
    let parsed = serde_json::from_str::<HttpApiResponse<serde_json::Value>>(body).ok();
    let mut metadata = std::collections::HashMap::new();
    metadata.insert("httpStatus".to_string(), status.to_string());
    let detail = if let Some(parsed) = parsed {
        metadata.insert("remoteCode".to_string(), parsed.code.to_string());
        let reason = parsed.reason.unwrap_or_default();
        if is_machine_reason(&reason) {
            metadata.insert("machineReason".to_string(), reason.clone());
        }
        let message = parsed.message.unwrap_or_default();
        match (reason.is_empty(), message.is_empty()) {
            (true, true) => String::new(),
            (false, true) => reason,
            (true, false) => message,
            (false, false) => format!("{reason}: {message}"),
        }
    } else {
        String::new()
    };
    let mut error = FlareError::general_error(if detail.is_empty() {
        format!("http status not success: {status}")
    } else {
        format!("http status not success: {status} ({detail})")
    });
    if let FlareError::Localized { params, .. } = &mut error {
        *params = Some(metadata);
    }
    error
}

fn is_machine_reason(reason: &str) -> bool {
    !reason.is_empty()
        && reason.len() <= 64
        && reason.as_bytes()[0].is_ascii_uppercase()
        && reason
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

/// 解析标准 HttpApiResponse：非 0 code 返回 FlareError，成功返回 data。
pub fn unwrap_api_response<T>(body: HttpApiResponse<T>, action: &str) -> Result<T> {
    if !body.is_success() {
        return Err(FlareError::localized(
            ErrorCode::GeneralError,
            format!(
                "{action} failed: {} {}",
                body.reason.unwrap_or_default(),
                body.message.unwrap_or_default()
            )
            .trim()
            .to_string(),
        ));
    }
    body.data
        .ok_or_else(|| FlareError::general_error(format!("{action} response missing data")))
}

/// 解析“成功但业务对象可以不存在”的标准响应。
///
/// JSON 的 `"data": null` 会反序列化为 [`HttpApiResponse::data`] 的 `None`；
/// 这对查询当前有效邀请链接一类接口是正常结果，不能误报为协议缺字段。
pub fn unwrap_optional_api_response<T>(
    body: HttpApiResponse<T>,
    action: &str,
) -> Result<Option<T>> {
    if !body.is_success() {
        return Err(FlareError::localized(
            ErrorCode::GeneralError,
            format!(
                "{action} failed: {} {}",
                body.reason.unwrap_or_default(),
                body.message.unwrap_or_default()
            )
            .trim()
            .to_string(),
        ));
    }
    Ok(body.data)
}

/// 解析无业务 body 的成功响应（DELETE / logout 等）。
///
/// Gateway 对 `()` 成功响应通常序列化为 `"data": null`，不能走 [`unwrap_api_response`]。
pub fn unwrap_void_api_response(body: HttpApiResponse<()>, action: &str) -> Result<()> {
    if !body.is_success() {
        return Err(FlareError::localized(
            ErrorCode::GeneralError,
            format!(
                "{action} failed: {} {}",
                body.reason.unwrap_or_default(),
                body.message.unwrap_or_default()
            )
            .trim()
            .to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{HttpApiResponse, http_error_from_response_status, unwrap_optional_api_response};
    use crate::shared::error::ErrorCode;

    #[test]
    fn http_error_preserves_typed_status_code_and_machine_reason() {
        for (status, reason) in [(403, "FORBIDDEN"), (404, "NOT_FOUND"), (400, "VALIDATION")] {
            let error = http_error_from_response_status(
                status,
                &format!(r#"{{"code":4003,"reason":"{reason}","message":"diagnostic only"}}"#),
            );
            assert_eq!(error.code(), Some(ErrorCode::GeneralError));
            let params = error.as_localized().unwrap().params.unwrap();
            assert_eq!(params.len(), 3);
            assert_eq!(params["httpStatus"], status.to_string());
            assert_eq!(params["remoteCode"], "4003");
            assert_eq!(params["machineReason"], reason);
            assert!(!params.values().any(|value| value.contains("diagnostic")));
        }
    }

    #[test]
    fn malformed_http_error_has_status_but_no_invented_machine_reason() {
        for body in [
            "not JSON",
            r#"{"reason":"FORBIDDEN"}"#,
            r#"{"code":"4003"}"#,
        ] {
            let error = http_error_from_response_status(503, body);
            let params = error.as_localized().unwrap().params.unwrap();
            assert_eq!(params.len(), 1);
            assert_eq!(params["httpStatus"], "503");
        }
    }

    #[test]
    fn untrusted_reason_and_extra_fields_are_not_machine_metadata() {
        for reason in ["Bearer secret", "forbidden", "A=B", "", &"A".repeat(65)] {
            let body = serde_json::json!({
                "code": 4003,
                "reason": reason,
                "message": "password=private",
                "Authorization": "Bearer private",
                "token": "private",
                "data": {"credential": "private"}
            });
            let error = http_error_from_response_status(403, &body.to_string());
            let params = error.as_localized().unwrap().params.unwrap();
            assert_eq!(params.len(), 2);
            assert!(!params.contains_key("machineReason"));
            assert!(!params.values().any(|value| value.contains("private")));
        }
    }

    #[test]
    fn optional_response_accepts_successful_null_data() {
        let response: HttpApiResponse<String> =
            serde_json::from_str(r#"{"code":0,"data":null}"#).unwrap();

        assert_eq!(
            unwrap_optional_api_response(response, "get optional").unwrap(),
            None
        );
    }

    #[test]
    fn optional_response_preserves_present_data() {
        let response: HttpApiResponse<String> =
            serde_json::from_str(r#"{"code":0,"data":"present"}"#).unwrap();

        assert_eq!(
            unwrap_optional_api_response(response, "get optional").unwrap(),
            Some("present".to_string())
        );
    }
}
