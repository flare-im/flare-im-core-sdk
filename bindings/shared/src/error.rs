use flare_im_core_sdk::serde_json::{Value, json};
use flare_im_core_sdk::{ErrorCode, FlareError};

pub fn binding_invalid_parameter(message: impl Into<String>) -> FlareError {
    FlareError::localized(ErrorCode::InvalidParameter, message.into())
}

/// The structured error every binding hands its host next to the human message
/// (`bindings/contract/errors.json`): `{code, reason, retryable, httpStatus?,
/// remoteCode?, machineReason?}`. `code` is the core `ErrorCode` wire name,
/// `INTERNAL_ERROR` for errors without one (system, IO); `reason` is the localized
/// key or the reason text. Of the error's params only these typed, validated facts
/// travel, never the rest (headers, bodies, arbitrary values):
/// - `httpStatus`: the HTTP status Core answered, 100..=599;
/// - `remoteCode`: Core's numeric error code;
/// - `machineReason`: Core's or the plugin's code (`PLUGIN_DISABLED`, `CALL_FULL`).
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
    let param = |name: &str| params.and_then(|params| params.get(name));
    if let Some(status) = param("httpStatus")
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|status| (100..=599).contains(status))
    {
        details["httpStatus"] = json!(status);
    }
    if let Some(remote) = param("remoteCode").and_then(|value| value.parse::<i32>().ok()) {
        details["remoteCode"] = json!(remote);
    }
    if let Some(reason) = param("machineReason").filter(|reason| is_machine_reason(reason)) {
        details["machineReason"] = json!(reason);
    }
    details
}

/// `A`–`Z` first, then `A`–`Z`, `0`–`9` or `_`, at most 64 bytes.
fn is_machine_reason(value: &str) -> bool {
    value.len() <= 64
        && value.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
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

    fn localized(code: ErrorCode, reason: &str, params: &[(&str, &str)]) -> FlareError {
        FlareError::Localized {
            code,
            reason: reason.into(),
            details: None,
            params: Some(
                params
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect::<HashMap<_, _>>(),
            ),
            timestamp: Default::default(),
        }
    }

    #[test]
    fn plugin_rejections_keep_their_machine_reason_as_typed_facts() {
        let err = localized(
            ErrorCode::OperationFailed,
            "sdk.plugin.rejected",
            &[
                ("machineReason", "CALL_FULL"),
                ("httpStatus", "422"),
                ("remoteCode", "3"),
            ],
        );
        assert_eq!(
            sdk_error_details(&err),
            json!({
                "code": "OPERATION_FAILED",
                "reason": "sdk.plugin.rejected",
                "retryable": false,
                "httpStatus": 422,
                "remoteCode": 3,
                "machineReason": "CALL_FULL",
            })
        );
        let value = sdk_error_value(&err, Some("capability.dispatch"));
        assert_eq!(value["operation"], "capability.dispatch");
        assert_eq!(value["message"], err.to_string());
    }

    #[test]
    fn only_validated_facts_leave_the_sdk() {
        let err = localized(
            ErrorCode::NetworkError,
            "sdk.http.failed",
            &[
                ("httpStatus", "999"),
                ("remoteCode", "not a number"),
                ("machineReason", "lower_case"),
                ("authorization", "Bearer secret"),
                ("body", "{\"token\":\"t\"}"),
            ],
        );
        let details = sdk_error_details(&err);
        let keys: Vec<_> = details
            .as_object()
            .expect("object")
            .keys()
            .cloned()
            .collect();
        assert_eq!(keys, ["code", "reason", "retryable"]);
        assert!(!is_machine_reason(&"A".repeat(65)));
        assert!(is_machine_reason("PLUGIN_DISABLED"));
    }

    #[test]
    fn errors_without_a_code_read_as_internal_and_retryability_is_cores() {
        let system = sdk_error_details(&FlareError::system("boom"));
        assert_eq!(system["code"], "INTERNAL_ERROR");
        assert_eq!(system["reason"], "boom");
        assert!(system.get("machineReason").is_none());
        let timeout =
            sdk_error_details(&FlareError::localized(ErrorCode::OperationTimeout, "late"));
        assert_eq!(timeout["code"], "OPERATION_TIMEOUT");
        assert_eq!(
            timeout["retryable"],
            ErrorCode::OperationTimeout.is_retryable()
        );
    }
}
