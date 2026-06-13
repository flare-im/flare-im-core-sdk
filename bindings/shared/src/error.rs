use flare_im_core_sdk::{ErrorCode, FlareError};
use serde_json::{Value, json};

pub fn binding_invalid_parameter(message: impl Into<String>) -> FlareError {
    FlareError::localized(ErrorCode::InvalidParameter, message.into())
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
