//! 错误上下文（占位）

use serde::{Deserialize, Serialize};

/// 错误上下文
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorContext {
    pub message: String,
}

impl ErrorContext {
    pub fn new(message: String) -> Self {
        Self { message }
    }
}
