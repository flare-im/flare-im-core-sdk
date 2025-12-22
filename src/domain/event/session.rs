//! Session 领域事件
//!
//! 定义所有 Session 聚合根相关的领域事件

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Session 已登录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLoggedIn {
    pub user_id: String,
    pub token: String,
    pub device_id: String,
}

/// Session 已登出
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLoggedOut {
    pub reason: String,
}

/// Session 已过期
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionExpired {
    pub expired_at: DateTime<Utc>,
}

/// Session Token 已刷新
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTokenRefreshed {
    pub new_token: String,
    pub refreshed_at: DateTime<Utc>,
}
