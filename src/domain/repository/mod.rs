//! 仓储接口定义
//!
//! 本模块定义了所有存储相关的 trait，采用 **分离接口设计**，符合 DDD 原则：
//!
//! - `EventStore`: 事件存储（用于事件溯源，通用接口）
//! - `MessageRepository`: 消息仓储（消息聚合根的专用接口）
//! - `ConversationRepository`: 会话仓储（会话聚合根的专用接口）
//! - `SnapshotStore`: 快照存储（用于聚合根快照，可选）
//!
//! ## 设计原则
//!
//! 1. **依赖倒置**: SDK 只依赖 trait，不依赖具体实现
//! 2. **接口隔离**: 每个聚合根有独立的仓储接口，用户只需实现需要的部分
//! 3. **DDD 符合**: 每个聚合根对应一个仓储，符合领域驱动设计
//! 4. **存储策略灵活**: 消息和会话可以使用不同的存储策略（时序DB vs 关系DB）
//! 5. **平台无关**: SDK 核心代码不关心存储的具体实现方式
//!
//! ## 实现示例
//!
//! 用户需要实现这些 trait，例如：
//!
//! ```no_run
//! use async_trait::async_trait;
//! use flare_im_core_sdk::domain::repository::{
//!     EventStore,
//!     MessageRepository,
//!     ConversationRepository,
//! };
//! use std::sync::Arc;
//!
//! // 用户实现 SQLite EventStore
//! struct MyEventStore { /* ... */ }
//!
//! #[async_trait]
//! impl EventStore for MyEventStore {
//!     async fn append(&self, event: DomainEvent) -> anyhow::Result<()> {
//!         // 实现存储逻辑
//!         Ok(())
//!     }
//!     // ... 其他方法
//! }
//!
//! // 用户实现 MessageRepository
//! struct MyMessageRepository { /* ... */ }
//!
//! #[async_trait]
//! impl MessageRepository for MyMessageRepository {
//!     async fn save(&self, message: &Message) -> anyhow::Result<()> {
//!         // 实现存储逻辑
//!         Ok(())
//!     }
//!     // ... 其他方法
//! }
//!
//! // 用户实现 ConversationRepository
//! struct MyConversationRepository { /* ... */ }
//!
//! #[async_trait]
//! impl ConversationRepository for MyConversationRepository {
//!     async fn save(&self, conversation: &Conversation) -> anyhow::Result<()> {
//!         // 实现存储逻辑
//!         Ok(())
//!     }
//!     // ... 其他方法
//! }
//! ```

pub mod event_store;
pub mod message_repository;
pub mod conversation_repository;
pub mod snapshot_store;

// 导出所有 trait 和类型
pub use event_store::EventStore;
pub use message_repository::{MessageRepository, MessageListResult};
pub use conversation_repository::{ConversationRepository, ConversationListResult};
pub use snapshot_store::SnapshotStore;
