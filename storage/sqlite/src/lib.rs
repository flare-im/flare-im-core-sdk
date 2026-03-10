mod message_store;
mod conversation_store;
mod cursor_store;

pub use message_store::SqliteMessageStore;
pub use conversation_store::SqliteConversationStore;
pub use cursor_store::SqliteSyncCursorStore;

use std::sync::Arc;
use sqlx::SqlitePool;

/// 存储提供者工厂
///
/// 创建 SQLite 连接池并初始化所有表，返回三个 store 实例，
/// 可直接传入 `StoreProvider`。
pub async fn create_stores(
    database_url: &str,
) -> anyhow::Result<(
    Arc<SqliteMessageStore>,
    Arc<SqliteConversationStore>,
    Arc<SqliteSyncCursorStore>,
)> {
    let pool = SqlitePool::connect(database_url).await?;

    let msg_store = SqliteMessageStore::new(pool.clone());
    msg_store.init().await?;

    let conv_store = SqliteConversationStore::new(pool.clone());
    conv_store.init().await?;

    let cursor_store = SqliteSyncCursorStore::new(pool.clone());
    cursor_store.init().await?;

    Ok((
        Arc::new(msg_store),
        Arc::new(conv_store),
        Arc::new(cursor_store),
    ))
}
