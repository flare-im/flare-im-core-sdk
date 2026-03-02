use std::collections::HashMap;
use serde::{Serialize, Deserialize};

/// 国际化错误信息载体
/// 
/// 这个结构体将被序列化并返回给前端/客户端，
/// 客户端根据 `key` 和 `params` 进行本地化翻译。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalizedError {
    /// 错误码 (e.g., "AUTH_001")
    pub code: String,
    
    /// 默认错误信息 (英文 fallback)
    pub message: String,
    
    /// 国际化键值 (e.g., "error.auth.login_failed")
    pub key: String,
    
    /// 动态参数 (e.g., { "retry_after": "30" })
    #[serde(default)]
    pub params: HashMap<String, String>,
    
    /// 错误发生的上下文/堆栈信息（可选，调试用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug_info: Option<String>,
}

/// 支持转换为国际化错误的 Trait
pub trait ToLocalizedError {
    fn to_localized_error(&self) -> LocalizedError;
}
