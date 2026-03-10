use async_trait::async_trait;
use flare_im_core_sdk::error::{Result, SdkError};
use flare_im_core_sdk::store::SyncCursorStore;
use sqlx::SqlitePool;

pub struct SqliteSyncCursorStore {
    pool: SqlitePool,
}

impl SqliteSyncCursorStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn init(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS sync_cursors (
                key TEXT PRIMARY KEY,
                cursor TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )"#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[async_trait]
impl SyncCursorStore for SqliteSyncCursorStore {
    async fn get(&self, key: &str) -> Result<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT cursor FROM sync_cursors WHERE key = ?",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SdkError::Store(e.to_string()))?;

        Ok(row.map(|(c,)| c))
    }

    async fn save(&self, key: &str, cursor: &str) -> Result<()> {
        sqlx::query(
            r#"INSERT OR REPLACE INTO sync_cursors (key, cursor, updated_at)
               VALUES (?, ?, datetime('now'))"#,
        )
        .bind(key)
        .bind(cursor)
        .execute(&self.pool)
        .await
        .map_err(|e| SdkError::Store(e.to_string()))?;
        Ok(())
    }
}
