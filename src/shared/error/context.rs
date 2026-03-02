//! 错误上下文
//!
//! 包含错误的附加信息，如参数、堆栈等

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 错误上下文
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ErrorContext {
    /// 原始错误信息
    pub message: String,
    
    /// 国际化参数
    #[serde(default)]
    pub params: HashMap<String, String>,
    
    /// 调试信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug_info: Option<String>,
}

impl ErrorContext {
    pub fn new(message: String) -> Self {
        Self { 
            message,
            params: HashMap::new(),
            debug_info: None,
        }
    }
    
    pub fn with_params(mut self, params: HashMap<String, String>) -> Self {
        self.params = params;
        self
    }
    
    pub fn with_param(mut self, key: &str, value: &str) -> Self {
        self.params.insert(key.to_string(), value.to_string());
        self
    }
    
    pub fn empty() -> Self {
        Self::default()
    }
}
