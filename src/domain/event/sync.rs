//! Sync 领域事件
//!
//! 定义所有 Sync 聚合根相关的领域事件

use serde::{Deserialize, Serialize};

/// Sync Bootstrap 已开始
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncBootstrapStarted;

/// Sync Bootstrap 已完成
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncBootstrapCompleted {
    pub cursor: String,
}

/// Sync Bootstrap 已失败
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncBootstrapFailed {
    pub error: String,
}

/// Sync Async 已开始
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncAsyncStarted {
    pub sync_type: String,
}

/// Sync Async 已完成
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncAsyncCompleted {
    pub sync_type: String,
    pub cursor: String,
}

/// Sync Async 已失败
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncAsyncFailed {
    pub sync_type: String,
    pub error: String,
}

/// Sync 进度已更新
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncProgressUpdated {
    pub sync_type: String,
    pub progress: f64, // 0.0 - 1.0
    pub current: u64,
    pub total: u64,
}
