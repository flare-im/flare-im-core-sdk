//! SDK 错误类型定义
//!
//! 基于 flare-core 的错误码，提供 SDK 层面的错误封装

use thiserror::Error;
// use chrono::{DateTime, Utc};
// use flare_core::common::error::FlareError;
use flare_core::common::error::code::ErrorCode;
// use std::fmt;
// use std::collections::HashMap;
use super::context::ErrorContext;
use super::i18n::{LocalizedError, ToLocalizedError};

/// SDK 错误类型
///
/// 封装 flare-core 的错误，并添加 SDK 特定的上下文信息
#[derive(Error, Debug, Clone)]
pub enum SDKError {
    /// 连接错误
    #[error("连接错误 [{code}]: {message}", code = .code.as_str())]
    Connection {
        code: ErrorCode,
        message: String,
        context: ErrorContext,
    },

    /// 认证错误
    #[error("认证错误 [{code}]: {message}", code = .code.as_str())]
    Authentication {
        code: ErrorCode,
        message: String,
        context: ErrorContext,
    },

    /// 消息错误
    #[error("消息错误 [{code}]: {message}", code = .code.as_str())]
    Message {
        code: ErrorCode,
        message: String,
        context: ErrorContext,
    },

    /// 同步错误
    #[error("同步错误 [{code}]: {message}", code = .code.as_str())]
    Sync {
        code: ErrorCode,
        message: String,
        context: ErrorContext,
    },

    /// 存储错误
    #[error("存储错误 [{code}]: {message}", code = .code.as_str())]
    Storage {
        code: ErrorCode,
        message: String,
        context: ErrorContext,
    },

    /// 配置错误
    #[error("配置错误: {message}")]
    Config {
        message: String,
        context: ErrorContext,
    },

    /// 内部错误（不暴露给用户）
    #[error("内部错误: {message}")]
    Internal {
        message: String,
        context: ErrorContext,
    },

    /// 包装其他错误
    #[error("包装错误: {message}")]
    Wrapped { message: String },
}

impl SDKError {
    /// 获取错误码
    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            SDKError::Connection { code, .. } => Some(*code),
            SDKError::Authentication { code, .. } => Some(*code),
            SDKError::Message { code, .. } => Some(*code),
            SDKError::Sync { code, .. } => Some(*code),
            SDKError::Storage { code, .. } => Some(*code),
            SDKError::Config { .. } => None,
            SDKError::Internal { .. } => None,
            SDKError::Wrapped { .. } => None,
        }
    }

    /// 获取错误消息
    pub fn message(&self) -> String {
        match self {
            SDKError::Connection { message, .. } => message.clone(),
            SDKError::Authentication { message, .. } => message.clone(),
            SDKError::Message { message, .. } => message.clone(),
            SDKError::Sync { message, .. } => message.clone(),
            SDKError::Storage { message, .. } => message.clone(),
            SDKError::Config { message, .. } => message.clone(),
            SDKError::Internal { message, .. } => message.clone(),
            SDKError::Wrapped { message } => message.clone(),
        }
    }

    /// 获取错误上下文
    pub fn context(&self) -> ErrorContext {
        match self {
            SDKError::Connection { context, .. } => context.clone(),
            SDKError::Authentication { context, .. } => context.clone(),
            SDKError::Message { context, .. } => context.clone(),
            SDKError::Sync { context, .. } => context.clone(),
            SDKError::Storage { context, .. } => context.clone(),
            SDKError::Config { context, .. } => context.clone(),
            SDKError::Internal { context, .. } => context.clone(),
            SDKError::Wrapped { .. } => ErrorContext::default(),
        }
    }

    /// 判断是否可重试
    pub fn is_retryable(&self) -> bool {
        match self.code() {
            Some(ErrorCode::ConnectionTimeout) => true,
            Some(ErrorCode::NetworkTimeout) => true,
            Some(ErrorCode::NetworkError) => true,
            Some(ErrorCode::NetworkConnectionLost) => true,
            Some(ErrorCode::ServiceUnavailable) => true,
            Some(ErrorCode::MessageSendFailed) => true,
            _ => false,
        }
    }

    /// 获取重试延迟（毫秒）
    pub fn retry_after_ms(&self) -> Option<u64> {
        match self.code() {
            Some(ErrorCode::ConnectionTimeout) => Some(1000), // 1秒
            Some(ErrorCode::NetworkTimeout) => Some(2000),    // 2秒
            Some(ErrorCode::NetworkError) => Some(1000),      // 1秒
            Some(ErrorCode::NetworkConnectionLost) => Some(2000), // 2秒
            Some(ErrorCode::ServiceUnavailable) => Some(5000), // 5秒
            Some(ErrorCode::MessageSendFailed) => Some(500),  // 0.5秒
            _ => None,
        }
    }

    /// 获取最大重试次数
    pub fn max_retries(&self) -> u32 {
        match self.code() {
            Some(ErrorCode::ConnectionTimeout) => 3,
            Some(ErrorCode::NetworkTimeout) => 3,
            Some(ErrorCode::NetworkError) => 3,
            Some(ErrorCode::NetworkConnectionLost) => 3,
            Some(ErrorCode::ServiceUnavailable) => 2,
            Some(ErrorCode::MessageSendFailed) => 5,
            _ => 0,
        }
    }

    /// 判断是否需要重新认证
    pub fn requires_reauth(&self) -> bool {
        match self.code() {
            Some(ErrorCode::AuthenticationExpired) => true,
            Some(ErrorCode::AuthenticationInvalid) => true,
            Some(ErrorCode::TokenExpired) => true,
            Some(ErrorCode::TokenInvalid) => true,
            _ => false,
        }
    }

    // ============================================================
    // 便捷构造方法
    // ============================================================

    /// 创建连接错误
    pub fn connection(code: ErrorCode, message: impl Into<String>) -> Self {
        let msg = message.into();
        Self::Connection {
            code,
            message: msg.clone(),
            context: ErrorContext::new(msg),
        }
    }

    /// 创建认证错误
    pub fn authentication(code: ErrorCode, message: impl Into<String>) -> Self {
        let msg = message.into();
        Self::Authentication {
            code,
            message: msg.clone(),
            context: ErrorContext::new(msg),
        }
    }

    /// 创建消息错误
    pub fn message_error(code: ErrorCode, message: impl Into<String>) -> Self {
        let msg = message.into();
        Self::Message {
            code,
            message: msg.clone(),
            context: ErrorContext::new(msg),
        }
    }
}

impl ToLocalizedError for SDKError {
    fn to_localized_error(&self) -> LocalizedError {
        let code_str = self.code()
            .map(|c| c.as_str().to_string())
            .unwrap_or_else(|| "INTERNAL_ERROR".to_string());
            
        let message = self.message();
        let context = self.context();
        
        // 生成国际化键值，格式: error.{CODE}
        // 例如: error.AUTH_001
        let key = format!("error.{}", code_str);
        
        LocalizedError {
            code: code_str,
            message,
            key,
            params: context.params,
            debug_info: context.debug_info,
        }
    }
}

impl From<anyhow::Error> for SDKError {
    fn from(err: anyhow::Error) -> Self {
        // 尝试向下转型为 SDKError
        if let Some(sdk_error) = err.downcast_ref::<SDKError>() {
            return sdk_error.clone();
        }
        
        // 默认为内部错误
        SDKError::Internal {
            message: err.to_string(),
            context: ErrorContext::new(err.to_string()),
        }
    }
}

/// SDK 结果类型
pub type SDKResult<T> = Result<T, SDKError>;
