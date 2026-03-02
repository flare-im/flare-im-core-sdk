//! 基础设施层 - 存储相关
//!
//! 注意：本模块不提供存储的具体实现（如 SQLite、IndexedDB）。
//! 存储实现应由用户根据平台自行实现，并实现 `domain::repository` 中定义的 trait。
//!
//! ## 本模块提供的功能
//!
//! - `media_cache`: 媒体文件缓存管理（业务逻辑，非存储抽象）
//! - `event_projection`: 事件投影器（用于将事件转换为读模型）
//!
//! ## 存储实现指南
//!
//! 用户需要实现以下 trait：
//! - `domain::repository::EventStore` - 事件存储
//! - `domain::repository::MessageRepository` - 消息仓储
//! - `domain::repository::ConversationRepository` - 会话仓储
//! - `domain::repository::SnapshotStore` - 快照存储（可选）
//!
//! 实现示例请参考 `examples/` 目录。

pub mod event_projection;
pub mod media_cache;

// 导出事件投影器
pub use event_projection::{EventProjector, EventProjectorBuilder};

// 注意：不再导出具体的存储实现
// 用户需要自行实现 domain::repository 中定义的 trait
