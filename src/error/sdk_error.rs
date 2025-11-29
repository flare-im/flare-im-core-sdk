//! SDK 错误类型定义
//!
//! 基于 flare-core 的错误码，提供 SDK 层面的错误封装

use flare_core::common::error::code::ErrorCode;
use flare_core::common::error::FlareError;
use thiserror::Error;
use std::collections::HashMap;
use chrono::{DateTime, Utc};

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
    Wrapped {
        message: String,
    },
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
            _ => false,
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
        Self::Connection {
            code,
            message: message.into(),
            context: ErrorContext::new(),
        }
    }
    
    /// 创建认证错误
    pub fn authentication(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Authentication {
            code,
            message: message.into(),
            context: ErrorContext::new(),
        }
    }
    
    /// 创建消息错误
    pub fn message_error(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Message {
            code,
            message: message.into(),
            context: ErrorContext::new(),
        }
    }
    
    /// 创建同步错误
    pub fn sync(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Sync {
            code,
            message: message.into(),
            context: ErrorContext::new(),
        }
    }
    
    /// 创建存储错误
    pub fn storage(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Storage {
            code,
            message: message.into(),
            context: ErrorContext::new(),
        }
    }
    
    /// 创建配置错误
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config {
            message: message.into(),
            context: ErrorContext::new(),
        }
    }
    
    /// 创建内部错误
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
            context: ErrorContext::new(),
        }
    }
}

impl From<FlareError> for SDKError {
    fn from(err: FlareError) -> Self {
        match err {
            FlareError::Localized { code, reason, .. } => {
                // 根据错误码分类
                match code {
                    ErrorCode::ConnectionFailed
                    | ErrorCode::ConnectionTimeout
                    | ErrorCode::ConnectionClosed
                    | ErrorCode::ConnectionRefused
                    | ErrorCode::NotConnected
                    | ErrorCode::ConnectionReconnecting => {
                        SDKError::connection(code, reason)
                    }
                    ErrorCode::AuthenticationFailed
                    | ErrorCode::AuthenticationExpired
                    | ErrorCode::AuthenticationInvalid
                    | ErrorCode::TokenInvalid
                    | ErrorCode::TokenExpired => {
                        SDKError::authentication(code, reason)
                    }
                    ErrorCode::MessageSendFailed
                    | ErrorCode::MessageDeliveryFailed
                    | ErrorCode::MessageNotFound
                    | ErrorCode::MessageExpired => {
                        SDKError::message_error(code, reason)
                    }
                    _ => SDKError::internal(reason),
                }
            }
            FlareError::System(msg) => SDKError::internal(msg),
            FlareError::Io(msg) => SDKError::storage(ErrorCode::DatabaseError, msg),
        }
    }
}

impl From<anyhow::Error> for SDKError {
    fn from(err: anyhow::Error) -> Self {
        SDKError::Wrapped {
            message: err.to_string(),
        }
    }
}

/// 错误上下文
/// 
/// 提供错误的额外上下文信息，用于调试和问题追踪
#[derive(Debug, Clone, Default)]
pub struct ErrorContext {
    /// 操作类型（如 "send_message", "sync_messages"）
    pub operation: Option<String>,
    
    /// 相关资源 ID（如 session_id, message_id）
    pub resource_id: Option<String>,
    
    /// 额外参数
    pub params: HashMap<String, String>,
    
    /// 错误发生时间
    pub timestamp: DateTime<Utc>,
    
    /// 错误链路（用于追踪错误传播路径）
    pub trace: Vec<String>,
}

impl ErrorContext {
    pub fn new() -> Self {
        Self {
            operation: None,
            resource_id: None,
            params: HashMap::new(),
            timestamp: Utc::now(),
            trace: Vec::new(),
        }
    }
    
    pub fn with_operation(mut self, operation: impl Into<String>) -> Self {
        self.operation = Some(operation.into());
        self
    }
    
    pub fn with_resource_id(mut self, resource_id: impl Into<String>) -> Self {
        self.resource_id = Some(resource_id.into());
        self
    }
    
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }
    
    pub fn add_trace(mut self, trace: impl Into<String>) -> Self {
        self.trace.push(trace.into());
        self
    }
}

/// Result 类型别名
pub type SDKResult<T> = Result<T, SDKError>;

