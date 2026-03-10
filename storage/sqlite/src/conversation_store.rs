use async_trait::async_trait;
use flare_im_core_sdk::store::ConversationStore;
use flare_im_core_sdk::error::{Result, SdkError};
use flare_proto::common::ConversationSummary;
use prost::Message as ProstMessage;
use sqlx::SqlitePool;
use tracing::debug;

pub struct SqliteConversationStore {
    pool: SqlitePool,
}

impl SqliteConversationStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn init(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS conversations (
                conversation_id TEXT PRIMARY KEY,
                unread_count INTEGER NOT NULL DEFAULT 0,
                last_read_seq INTEGER NOT NULL DEFAULT 0,
                max_seq INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                data BLOB NOT NULL
            )"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_conversations_updated
               ON conversations(updated_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[async_trait]
impl ConversationStore for SqliteConversationStore {
    async fn save_batch(&self, conversations: &[ConversationSummary]) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(|e| SdkError::Store(e.to_string()))?;
        for conv in conversations {
            let data = conv.encode_to_vec();
            sqlx::query(
                r#"INSERT OR REPLACE INTO conversations
                   (conversation_id, unread_count, max_seq, data, updated_at)
                   VALUES (?, ?, ?, ?, datetime('now'))"#,
            )
            .bind(&conv.conversation_id)
            .bind(conv.unread_count as i32)
            .bind(conv.max_seq as i64)
            .bind(&data)
            .execute(&mut *tx)
            .await
            .map_err(|e| SdkError::Store(e.to_string()))?;
        }
        tx.commit().await.map_err(|e| SdkError::Store(e.to_string()))?;
        debug!(count = conversations.len(), "saved conversation batch");
        Ok(())
    }

    async fn get(&self, conversation_id: &str) -> Result<Option<ConversationSummary>> {
        let row: Option<(Vec<u8>,)> = sqlx::query_as(
            "SELECT data FROM conversations WHERE conversation_id = ?",
        )
        .bind(conversation_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SdkError::Store(e.to_string()))?;

        match row {
            Some((data,)) => {
                let conv = ConversationSummary::decode(data.as_slice())
                    .map_err(|e| SdkError::Store(format!("decode ConversationSummary: {e}")))?;
                Ok(Some(conv))
            }
            None => Ok(None),
        }
    }

    async fn list(&self) -> Result<Vec<ConversationSummary>> {
        let rows: Vec<(Vec<u8>,)> = sqlx::query_as(
            "SELECT data FROM conversations ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SdkError::Store(e.to_string()))?;

        let mut conversations = Vec::with_capacity(rows.len());
        for (data,) in rows {
            if let Ok(conv) = ConversationSummary::decode(data.as_slice()) {
                conversations.push(conv);
            }
        }
        Ok(conversations)
    }

    async fn update_unread(
        &self,
        conversation_id: &str,
        unread_count: u32,
        last_read_seq: u64,
    ) -> Result<()> {
        sqlx::query(
            r#"UPDATE conversations
               SET unread_count = ?, last_read_seq = ?, updated_at = datetime('now')
               WHERE conversation_id = ?"#,
        )
        .bind(unread_count as i32)
        .bind(last_read_seq as i64)
        .bind(conversation_id)
        .execute(&self.pool)
        .await
        .map_err(|e| SdkError::Store(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, conversation_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM conversations WHERE conversation_id = ?")
            .bind(conversation_id)
            .execute(&self.pool)
            .await
            .map_err(|e| SdkError::Store(e.to_string()))?;
        Ok(())
    }
}
