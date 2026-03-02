use serde::Serialize;
use flare_im_core_sdk::shared::error::{SDKError, LocalizedError, ToLocalizedError};
use std::collections::HashMap;

/// Tauri 命令统一错误返回类型
///
/// 所有的 Tauri 命令应该返回 `Result<T, CommandError>` 而不是 `Result<T, String>`。
/// 前端接收到的将是一个结构化的 JSON 对象，包含错误码、Key 和参数，便于国际化。
#[derive(Debug, Serialize)]
pub struct CommandError(LocalizedError);

impl From<SDKError> for CommandError {
    fn from(error: SDKError) -> Self {
        CommandError(error.to_localized_error())
    }
}

impl From<anyhow::Error> for CommandError {
    fn from(error: anyhow::Error) -> Self {
        let sdk_error: SDKError = error.into();
        CommandError(sdk_error.to_localized_error())
    }
}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        CommandError(LocalizedError {
            code: "UNKNOWN".to_string(),
            message: message.clone(),
            key: "error.unknown".to_string(),
            params: HashMap::new(),
            debug_info: None,
        })
    }
}

impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        Self::from(message.to_string())
    }
}
