//! SDK 统一错误处理体系
//!
//! 基于 flare-core 的错误码，提供 SDK 层面的错误封装和恢复策略

mod context;
mod recovery;
mod sdk_error;
pub mod i18n;

pub use recovery::{CircuitBreaker, ErrorRecovery, RetryStrategy};
pub use context::ErrorContext;
pub use sdk_error::{SDKError, SDKResult};
pub use i18n::{LocalizedError, ToLocalizedError};
pub use flare_core::common::error::code::ErrorCode;
