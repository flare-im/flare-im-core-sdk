//! SDK 统一错误处理体系
//!
//! 基于 flare-core 的错误码，提供 SDK 层面的错误封装和恢复策略

mod sdk_error;
mod recovery;
mod context;

pub use sdk_error::{SDKError, SDKResult, ErrorContext};
pub use recovery::{RetryStrategy, ErrorRecovery, CircuitBreaker};

