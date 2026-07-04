use flare_im_core_sdk::serde_json::Value;

#[derive(Debug, Clone)]
pub struct BindingRequest {
    pub operation: String,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct BindingResponse {
    pub payload: Value,
    pub is_unit: bool,
}

impl BindingRequest {
    pub fn new(operation: impl Into<String>, payload: Value) -> Self {
        Self {
            operation: operation.into(),
            payload,
        }
    }
}

impl BindingResponse {
    pub fn json(payload: Value) -> Self {
        Self {
            payload,
            is_unit: false,
        }
    }

    pub fn unit() -> Self {
        Self {
            payload: Value::Null,
            is_unit: true,
        }
    }
}
