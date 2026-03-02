//! SQLite 存储实现
//!
//! 提供基于 SQLite 的存储实现，包括：
//! - EventStore: 事件存储
//! - MessageRepository: 消息仓储
//! - ConversationRepository: 会话仓储
//! - SnapshotStore: 快照存储（可选）

pub mod event_store;
pub mod message_repository;
pub mod conversation_repository;
pub mod snapshot_store;

pub use event_store::SqliteEventStore;
pub use message_repository::SqliteMessageRepository;
pub use conversation_repository::SqliteConversationRepository;
pub use snapshot_store::SqliteSnapshotStore;

/// 创建 SQLite 数据库连接池并初始化所有表
///
/// # 参数
/// * `database_url` - 数据库 URL（例如: "sqlite:./flare_im.db"）
///
/// # 返回
/// * `Ok((EventStore, MessageRepository, ConversationRepository))` - 存储实例
/// * `Err` - 创建失败
pub async fn create_storage(
    database_url: &str,
) -> anyhow::Result<(
    std::sync::Arc<SqliteEventStore>,
    std::sync::Arc<SqliteMessageRepository>,
    std::sync::Arc<SqliteConversationRepository>,
)> {
    use std::sync::Arc;
    
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;
    
    let pool = Arc::new(pool);
    
    // 初始化所有表
    let event_store = Arc::new(SqliteEventStore::new(pool.clone()));
    event_store.init().await?;
    
    let message_repository = Arc::new(SqliteMessageRepository::new(pool.clone()));
    message_repository.init().await?;
    
    let conversation_repository = Arc::new(SqliteConversationRepository::new(pool.clone()));
    conversation_repository.init().await?;
    
    Ok((event_store, message_repository, conversation_repository))
}
