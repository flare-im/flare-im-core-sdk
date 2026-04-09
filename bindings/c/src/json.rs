//! JSON 辅助工具模块
//!
//! 提供 JSON 解析和序列化的辅助函数

use std::ffi::c_void;

use crate::error::FlareErrorCode;
use crate::string::parse_string;

/// 解析 JSON 字符串为指定类型
///
/// # Safety
/// `json_ptr` 必须是有效的 UTF-8 JSON 字符串指针
pub fn parse_json<T: serde::de::DeserializeOwned>(json_ptr: *const i8) -> Result<T, FlareErrorCode> {
    let json_str = parse_string(json_ptr)?;
    serde_json::from_str(&json_str).map_err(|e| {
        tracing::error!("JSON parse error: {}", e);
        FlareErrorCode::InvalidJson
    })
}

/// 解析可选 JSON 字符串
///
/// # Safety
/// `json_ptr` 如果不为 null，必须是有效的 UTF-8 JSON 字符串指针
pub fn parse_optional_json<T: serde::de::DeserializeOwned>(
    json_ptr: *const i8,
) -> Result<Option<T>, FlareErrorCode> {
    if json_ptr.is_null() {
        return Ok(None);
    }
    parse_json(json_ptr).map(Some)
}

/// 序列化值为 JSON 字符串
pub fn to_json<T: serde::Serialize>(value: &T) -> Result<String, FlareErrorCode> {
    serde_json::to_string(value).map_err(|e| {
        tracing::error!("JSON serialize error: {}", e);
        FlareErrorCode::InternalError
    })
}

/// 序列化值为 JSON 字符串（美化格式）
pub fn to_json_pretty<T: serde::Serialize>(value: &T) -> Result<String, FlareErrorCode> {
    serde_json::to_string_pretty(value).map_err(|e| {
        tracing::error!("JSON serialize error: {}", e);
        FlareErrorCode::InternalError
    })
}
