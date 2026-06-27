//! 同步引擎专用错误类型
//!
//! 任务执行失败时使用 [SyncError]；与 [crate::shared::error::FlareError] 互转，便于统一处理。

use crate::shared::error::FlareError;

/// 同步任务执行错误
pub type SyncError = FlareError;

/// 同步任务执行结果类型
pub type SyncResult<T> = std::result::Result<T, SyncError>;
