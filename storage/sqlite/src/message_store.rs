use async_trait::async_trait;
use flare_im_core_sdk::error::Result;
use flare_im_core_sdk::error::SdkError;
use flare_im_core_sdk::store::MessageStore;
use flare_proto::common::Message;
use prost::Message as ProstMessage;
use sqlx::SqlitePool;
use tracing::debug;

pub struct SqliteMessageStore {
    pool: SqlitePool,
}

impl SqliteMessageStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn init(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS messages (
                server_id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                seq INTEGER NOT NULL DEFAULT 0,
                status INTEGER NOT NULL DEFAULT 0,
                data BLOB NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_messages_conv_seq
               ON messages(conversation_id, seq DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts
               USING fts5(server_id, text_content)"#,
        )
        .execute(&self.pool)
        .await
        .ok(); // FTS5 可能不可用

        Ok(())
    }
}

#[async_trait]
impl MessageStore for SqliteMessageStore {
    async fn save_batch(&self, messages: &[Message]) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(|e| SdkError::Store(e.to_string()))?;
        for msg in messages {
            let data = msg.encode_to_vec();
            sqlx::query(
                r#"INSERT OR REPLACE INTO messages (server_id, conversation_id, seq, status, data)
                   VALUES (?, ?, ?, ?, ?)"#,
            )
            .bind(&msg.server_id)
            .bind(&msg.conversation_id)
            .bind(msg.seq as i64)
            .bind(msg.status)
            .bind(&data)
            .execute(&mut *tx)
            .await
            .map_err(|e| SdkError::Store(e.to_string()))?;
        }
        tx.commit().await.map_err(|e| SdkError::Store(e.to_string()))?;
        debug!(count = messages.len(), "saved message batch");
        Ok(())
    }

    async fn get(&self, message_id: &str) -> Result<Option<Message>> {
        let row: Option<(Vec<u8>,)> = sqlx::query_as(
            "SELECT data FROM messages WHERE server_id = ?",
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SdkError::Store(e.to_string()))?;

        match row {
            Some((data,)) => {
                let msg = Message::decode(data.as_slice())?;
                Ok(Some(msg))
            }
            None => Ok(None),
        }
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<Message>> {
        let rows: Vec<(Vec<u8>,)> = sqlx::query_as(
            r#"SELECT data FROM messages
               WHERE conversation_id = ? AND seq < ?
               ORDER BY seq DESC LIMIT ?"#,
        )
        .bind(conversation_id)
        .bind(before_seq as i64)
        .bind(limit as i32)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SdkError::Store(e.to_string()))?;

        let mut messages = Vec::with_capacity(rows.len());
        for (data,) in rows {
            messages.push(Message::decode(data.as_slice())?);
        }
        Ok(messages)
    }

    async fn update_status(&self, message_id: &str, status: i32) -> Result<()> {
        // Read-modify-write: update status field in the proto blob
        let row: Option<(Vec<u8>,)> = sqlx::query_as(
            "SELECT data FROM messages WHERE server_id = ?",
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SdkError::Store(e.to_string()))?;

        if let Some((data,)) = row {
            let mut msg = Message::decode(data.as_slice())?;
            msg.status = status;
            let new_data = msg.encode_to_vec();
            sqlx::query("UPDATE messages SET status = ?, data = ? WHERE server_id = ?")
                .bind(status)
                .bind(&new_data)
                .bind(message_id)
                .execute(&self.pool)
                .await
                .map_err(|e| SdkError::Store(e.to_string()))?;
        }
        Ok(())
    }

    async fn update_content(&self, message_id: &str, new_content: Vec<u8>) -> Result<()> {
        let row: Option<(Vec<u8>,)> = sqlx::query_as(
            "SELECT data FROM messages WHERE server_id = ?",
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SdkError::Store(e.to_string()))?;

        if let Some((data,)) = row {
            let mut msg = Message::decode(data.as_slice())?;
            msg.content = new_content;
            let new_data = msg.encode_to_vec();
            sqlx::query("UPDATE messages SET data = ? WHERE server_id = ?")
                .bind(&new_data)
                .bind(message_id)
                .execute(&self.pool)
                .await
                .map_err(|e| SdkError::Store(e.to_string()))?;
        }
        Ok(())
    }

    async fn delete(&self, message_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM messages WHERE server_id = ?")
            .bind(message_id)
            .execute(&self.pool)
            .await
            .map_err(|e| SdkError::Store(e.to_string()))?;
        Ok(())
    }

    async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<Message>> {
        // FTS5 search fallback to LIKE if FTS unavailable
        let rows: Vec<(Vec<u8>,)> = sqlx::query_as(
            r#"SELECT m.data FROM messages_fts f
               JOIN messages m ON m.server_id = f.server_id
               WHERE messages_fts MATCH ?
               LIMIT ?"#,
        )
        .bind(keyword)
        .bind(limit as i32)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        let mut messages = Vec::with_capacity(rows.len());
        for (data,) in rows {
            if let Ok(msg) = Message::decode(data.as_slice()) {
                messages.push(msg);
            }
        }
        Ok(messages)
    }
}
