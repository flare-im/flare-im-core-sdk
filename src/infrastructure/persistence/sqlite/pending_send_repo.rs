use async_trait::async_trait;
use prost::Message as ProstMessage;
use sqlx::SqlitePool;

use crate::domain::{PendingSendReader, PendingSendVo, PendingSendWriter};
use crate::error::{ErrorCode, FlareError, Result};

pub struct SqlitePendingSendRepo {
    pool: SqlitePool,
}

impl SqlitePendingSendRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn encode_entry(vo: &PendingSendVo) -> Vec<u8> {
    vo.message.to_proto().encode_to_vec()
}

fn decode_entry(data: &[u8], enqueued_at_ms: u64) -> Result<PendingSendVo> {
    let proto = crate::model::Message::decode(data).map_err(|e| {
        FlareError::localized(crate::error::ErrorCode::DatabaseError, e.to_string())
    })?;
    let message = crate::model::IMMessage::from(proto);
    Ok(PendingSendVo {
        client_msg_id: message.client_msg_id.clone(),
        conversation_id: message.conversation_id.clone(),
        message,
        enqueued_at_ms,
    })
}

#[async_trait]
impl PendingSendReader for SqlitePendingSendRepo {
    async fn get(&self, client_msg_id: &str) -> Result<Option<PendingSendVo>> {
        let row: Option<(Vec<u8>, i64)> = sqlx::query_as(
            "SELECT data, enqueued_at_ms FROM pending_sends WHERE client_msg_id = ?",
        )
        .bind(client_msg_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        match row {
            Some((data, enqueued_at_ms)) => Ok(Some(decode_entry(&data, enqueued_at_ms as u64)?)),
            None => Ok(None),
        }
    }

    async fn list(&self) -> Result<Vec<PendingSendVo>> {
        let rows: Vec<(Vec<u8>, i64)> = sqlx::query_as(
            "SELECT data, enqueued_at_ms FROM pending_sends ORDER BY enqueued_at_ms ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for (data, enqueued_at_ms) in rows {
            out.push(decode_entry(&data, enqueued_at_ms as u64)?);
        }
        Ok(out)
    }

    async fn take_oldest(&self) -> Result<Option<PendingSendVo>> {
        let row: Option<(Vec<u8>, i64)> = sqlx::query_as(
            "SELECT data, enqueued_at_ms FROM pending_sends ORDER BY enqueued_at_ms ASC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(row
            .map(|(data, enqueued_at_ms)| decode_entry(&data, enqueued_at_ms as u64))
            .transpose()?)
    }
}

#[async_trait]
impl PendingSendWriter for SqlitePendingSendRepo {
    async fn push(&self, entry: PendingSendVo) -> Result<()> {
        let data = encode_entry(&entry);
        sqlx::query(
            r#"INSERT OR REPLACE INTO pending_sends (client_msg_id, conversation_id, enqueued_at_ms, data)
               VALUES (?, ?, ?, ?)"#,
        )
        .bind(&entry.client_msg_id)
        .bind(&entry.conversation_id)
        .bind(entry.enqueued_at_ms as i64)
        .bind(&data)
        .execute(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn pop(&self, client_msg_id: &str) -> Result<Option<PendingSendVo>> {
        let vo = PendingSendReader::get(self, client_msg_id).await?;
        if vo.is_some() {
            sqlx::query("DELETE FROM pending_sends WHERE client_msg_id = ?")
                .bind(client_msg_id)
                .execute(&self.pool)
                .await
                .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        }
        Ok(vo)
    }
}
