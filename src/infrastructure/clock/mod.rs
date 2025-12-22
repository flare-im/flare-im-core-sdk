//! 时钟模块
//!
//! 提供时间服务

use chrono::{DateTime, Utc};

/// 时钟服务
pub struct Clock;

impl Clock {
    /// 获取当前时间
    pub fn now() -> DateTime<Utc> {
        Utc::now()
    }
}
