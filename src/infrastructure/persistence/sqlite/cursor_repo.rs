use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::domain::{SyncCursorReader, SyncCursorVo, SyncCursorWriter};
use crate::shared::error::{ErrorCode, FlareError, Result};

pub struct SqliteSyncCursorRepo {
    pool: SqlitePool,
}

impl SqliteSyncCursorRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SyncCursorReader for SqliteSyncCursorRepo {
    async fn get_raw(&self, key: &str) -> Result<Option<String>> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT cursor FROM sync_cursors WHERE key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(row.map(|(c,)| c))
    }

    async fn get_conversation_cursor(
        &self,
        user_id: &str,
        conversation_id: &str,
    ) -> Result<Option<SyncCursorVo>> {
        let row: Option<(String, String, i64, i64)> = sqlx::query_as(
            r#"SELECT user_id, conversation_id, last_seq, synced_at
               FROM sync_conversation_cursors
               WHERE user_id = ? AND conversation_id = ?"#,
        )
        .bind(user_id)
        .bind(conversation_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(row.map(|(uid, cid, seq, synced_at)| SyncCursorVo {
            user_id: uid,
            conversation_id: cid,
            last_seq: seq.max(0) as u64,
            synced_at: synced_at.max(0) as u64,
        }))
    }
}

#[async_trait]
impl SyncCursorWriter for SqliteSyncCursorRepo {
    async fn save_raw(&self, key: &str, cursor: &str) -> Result<()> {
        sqlx::query(
            r#"INSERT OR REPLACE INTO sync_cursors (key, cursor, updated_at)
               VALUES (?, ?, datetime('now'))"#,
        )
        .bind(key)
        .bind(cursor)
        .execute(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn save_conversation_cursor(&self, cursor: &SyncCursorVo) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO sync_conversation_cursors
               (user_id, conversation_id, last_seq, synced_at)
               VALUES (?, ?, ?, ?)
               ON CONFLICT(user_id, conversation_id) DO UPDATE SET
                   last_seq = MAX(sync_conversation_cursors.last_seq, excluded.last_seq),
                   synced_at = MAX(sync_conversation_cursors.synced_at, excluded.synced_at)"#,
        )
        .bind(&cursor.user_id)
        .bind(&cursor.conversation_id)
        .bind(cursor.last_seq as i64)
        .bind(cursor.synced_at as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }
}
