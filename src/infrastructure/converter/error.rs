//! 转换错误类型

use thiserror::Error;

/// 转换错误类型
#[derive(Debug, Error)]
pub enum ConversionError {
    #[error("Serialization error: {0}")]
    Serialization(String),
    
    #[error("Deserialization error: {0}")]
    Deserialization(String),
    
    #[error("Field mapping error: {0}")]
    FieldMapping(String),
    
    #[error("Version mismatch: {0}")]
    VersionMismatch(String),
    
    #[error("Invalid data: {0}")]
    InvalidData(String),
    
    #[error("Converter not found: {0}")]
    ConverterNotFound(String),
}

impl From<serde_json::Error> for ConversionError {
    fn from(err: serde_json::Error) -> Self {
        ConversionError::Serialization(err.to_string())
    }
}

impl From<anyhow::Error> for ConversionError {
    fn from(err: anyhow::Error) -> Self {
        ConversionError::FieldMapping(err.to_string())
    }
}
