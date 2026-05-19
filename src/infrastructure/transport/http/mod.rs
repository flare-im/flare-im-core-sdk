pub mod http_client;
pub mod media_dto;
pub use http_client::{HttpClient, HttpRequestContext};
pub use media_dto::{
    AbortDirectUploadHttpRequest, CommitDirectUploadPartsHttpRequest,
    CommitDirectUploadPartsHttpResponse, CompleteDirectUploadHttpRequest, DeleteFileHttpRequest,
    DeleteFileHttpResponse, DirectUploadTransportKindHttp, FileInfoHttpResponse,
    GetDirectUploadStatusHttpResponse, GetFileUrlHttpRequest, GetFileUrlHttpResponse,
    InitiateDirectUploadHttpRequest, InitiateDirectUploadHttpResponse,
    PresignDirectUploadPartsHttpRequest, PresignDirectUploadPartsHttpResponse,
    PresignedUploadPartHttp, UploadFileHttpResponse, UploadFileMetadataHttp, UploadedPartInfoHttp,
};

use crate::error::{ErrorCode, FlareError, Result};

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

/// 非 2xx 时从 gateway JSON body 提取 reason/message，便于定位 400/401 等。
pub fn http_error_from_response_status(status: u16, body: &str) -> FlareError {
    if let Ok(parsed) = serde_json::from_str::<HttpApiResponse<serde_json::Value>>(body) {
        let reason = parsed.reason.unwrap_or_default();
        let message = parsed.message.unwrap_or_default();
        let detail = match (reason.is_empty(), message.is_empty()) {
            (true, true) => String::new(),
            (false, true) => reason,
            (true, false) => message,
            (false, false) => format!("{reason}: {message}"),
        };
        if !detail.is_empty() {
            return FlareError::general_error(format!(
                "http status not success: {status} ({detail})"
            ));
        }
    }
    FlareError::general_error(format!("http status not success: {status}"))
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
