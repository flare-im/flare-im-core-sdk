use flare_im_core_sdk::serde_json::{Value, json};
use flare_im_core_sdk::{ErrorCode, FlareError};

pub fn binding_invalid_parameter(message: impl Into<String>) -> FlareError {
    FlareError::localized(ErrorCode::InvalidParameter, message.into())
}

/// The structured error every binding hands its host next to the human message
/// (`bindings/contract/errors.json`): `{code, reason, params?, retryable}`. `code` is
/// the core `ErrorCode` wire name, `INTERNAL_ERROR` for errors without one (system,
/// IO); `reason` is the localized key or the reason text; `params` carries the
/// metadata Core and the SDK attached, e.g. a plugin's `machineReason`.
pub fn sdk_error_details(err: &FlareError) -> Value {
    let (code, params) = match err {
        FlareError::Localized { code, params, .. } => (code.as_str(), params.as_ref()),
        _ => (ErrorCode::InternalError.as_str(), None),
    };
    let mut details = json!({
        "code": code,
        "reason": err.reason(),
        "retryable": err.is_retryable(),
    });
    if let Some(params) = params.filter(|params| !params.is_empty()) {
        details["params"] = json!(params);
    }
    details
}

/// [`sdk_error_details`] plus the human `message` and, when known, the binding
/// `operation`: the rejection value of the Tauri commands and the properties of
/// the wasm runtime's thrown `Error`.
pub fn sdk_error_value(err: &FlareError, operation: Option<&str>) -> Value {
    let mut value = sdk_error_details(err);
    value["message"] = json!(err.to_string());
    if let Some(operation) = operation {
        value["operation"] = json!(operation);
    }
    value
}

pub fn binding_operation_not_supported(operation: impl AsRef<str>) -> FlareError {
    FlareError::localized(
        ErrorCode::OperationNotSupported,
        format!(
            "binding operation is not implemented: {}",
            operation.as_ref()
        ),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingBoundaryError {
    pub code: &'static str,
    pub operation: String,
    pub message: String,
}

pub type BindingBoundaryResult<T> = Result<T, BindingBoundaryError>;

impl BindingBoundaryError {
    pub fn new(
        code: &'static str,
        operation: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            operation: operation.into(),
            message: message.into(),
        }
    }

    pub fn invalid_parameter(operation: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new("invalidParameter", operation, message)
    }

    pub fn capability_unavailable(
        operation: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new("capabilityUnavailable", operation, message)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "code": self.code,
            "operation": self.operation,
            "message": self.message,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn plugin_rejections_keep_their_machine_reason() {
        let err = FlareError::Localized {
            code: ErrorCode::OperationFailed,
            reason: "sdk.plugin.rejected".into(),
            details: None,
            params: Some(HashMap::from([
                ("machineReason".to_string(), "CALL_FULL".to_string()),
                ("httpStatus".to_string(), "422".to_string()),
            ])),
            timestamp: Default::default(),
        };
        assert_eq!(
            sdk_error_details(&err),
            json!({
                "code": "OPERATION_FAILED",
                "reason": "sdk.plugin.rejected",
                "retryable": false,
                "params": {"machineReason": "CALL_FULL", "httpStatus": "422"},
            })
        );
        let value = sdk_error_value(&err, Some("capability.dispatch"));
        assert_eq!(value["operation"], "capability.dispatch");
        assert_eq!(value["message"], err.to_string());
    }

    #[test]
    fn errors_without_a_code_read_as_internal_and_retryability_is_cores() {
        let system = sdk_error_details(&FlareError::system("boom"));
        assert_eq!(system["code"], "INTERNAL_ERROR");
        assert_eq!(system["reason"], "boom");
        assert!(system.get("params").is_none());
        let timeout =
            sdk_error_details(&FlareError::localized(ErrorCode::OperationTimeout, "late"));
        assert_eq!(timeout["code"], "OPERATION_TIMEOUT");
        assert_eq!(
            timeout["retryable"],
            ErrorCode::OperationTimeout.is_retryable()
        );
    }
}
