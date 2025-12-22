//! 领域事件定义
//!
//! 所有状态变化通过领域事件表达，事件可回放
//!
//! ## 模块结构
//!
//! - `domain_event.rs`: 领域事件基类
//! - `constants.rs`: 事件名称常量
//! - `session.rs`: Session 相关事件
//! - `connection.rs`: Connection 相关事件
//! - `message.rs`: Message 相关事件
//! - `conversation.rs`: Conversation 相关事件
//! - `sync.rs`: Sync 相关事件

mod constants;
mod session;
mod connection;
mod message;
mod conversation;
mod sync;
pub mod subscribers;  // 公开模块，允许其他层访问

// 导出领域事件基类
pub use domain_event::DomainEvent;

// 导出事件名称常量
pub use constants::{
    session_events,
    connection_events,
    message_events,
    conversation_events,
    sync_events,
};

// 导出所有事件结构体
pub use session::*;
pub use connection::*;
pub use message::*;
pub use conversation::*;
pub use sync::*;

// 导出事件订阅器 trait（领域接口）
pub use subscribers::*;

/// 领域事件基类
mod domain_event {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};

    /// 领域事件基类
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct DomainEvent {
        /// 事件ID
        pub event_id: String,
        
        /// 事件类型
        pub event_type: String,
        
        /// 聚合根ID
        pub aggregate_id: String,
        
        /// 事件版本（用于乐观锁）
        pub version: u64,
        
        /// 事件时间戳
        pub timestamp: DateTime<Utc>,
        
        /// 事件数据（JSON）
        pub data: serde_json::Value,
    }

    impl DomainEvent {
        /// 创建新的领域事件
        pub fn new(
            event_type: impl Into<String>,
            aggregate_id: impl Into<String>,
            version: u64,
            data: serde_json::Value,
        ) -> Self {
            Self {
                event_id: uuid::Uuid::new_v4().to_string(),
                event_type: event_type.into(),
                aggregate_id: aggregate_id.into(),
                version,
                timestamp: Utc::now(),
                data,
            }
        }
    }
}
