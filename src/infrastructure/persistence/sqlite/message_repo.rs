//! SQLite 消息仓储：与 [schema] 中 messages 表结构一致，按列读写；row 直接映射为 IMMessage（不经 ProtoMessage）。

use std::collections::HashMap;

use async_trait::async_trait;
use base64::prelude::*;
use sqlx::{Row, SqlitePool};

use crate::domain::{MessageReader, MessageWriter};
use crate::error::{ErrorCode, FlareError, Result};
use crate::infrastructure::persistence::MessageStore;
use crate::model::message::MessageLocalState;
use crate::model::{
    IMMessage, decode_content_bytes, decoded_content_to_elem, message_elem::TextElem, Elem,
};
use flare_proto::common::MessageType;

fn parse_extra(s: Option<&str>) -> HashMap<String, String> {
    let s = match s {
        Some(x) if !x.is_empty() => x,
        _ => return HashMap::new(),
    };
    serde_json::from_str(s).unwrap_or_default()
}

fn parse_mention_users(s: Option<&str>) -> Vec<String> {
    let s = match s {
        Some(x) if !x.is_empty() => x,
        _ => return Vec::new(),
    };
    serde_json::from_str(s).unwrap_or_default()
}

fn sqlx_err(e: sqlx::Error) -> FlareError {
    FlareError::localized(ErrorCode::DatabaseError, e.to_string())
}

/// 将分页游标 `before_seq` 绑定为 SQLite INTEGER（`seq < ?`）。
///
/// 不可直接 `before_seq as i64`：`u64::MAX as i64` 为 **-1**，条件变成 `seq < -1`，结果集恒为空。
/// Rust 示例与部分调用方会用 `u64::MAX` 表示「无上限」，此处钳制到 `i64::MAX`。
fn before_seq_for_sqlite(before_seq: u64) -> i64 {
    if before_seq >= i64::MAX as u64 {
        i64::MAX
    } else {
        before_seq as i64
    }
}

fn parse_extensions(s: Option<&str>) -> HashMap<String, Vec<u8>> {
    let s = match s {
        Some(x) if !x.is_empty() => x,
        _ => return HashMap::new(),
    };
    let map: HashMap<String, String> = match serde_json::from_str(s) {
        Ok(m) => m,
        Err(_) => return HashMap::new(),
    };
    map.into_iter()
        .filter_map(|(k, v)| BASE64_STANDARD.decode(&v).ok().map(|b| (k, b)))
        .collect()
}

const MESSAGE_SELECT_COLS: &str = r#"server_id, conversation_id, client_msg_id, sender_id, source,
    seq, timestamp, client_timestamp, conversation_type, message_type, channel_id,
    sender_name, sender_avatar, sender_display_name, content, status, is_read, is_recalled, is_edited,
    reply_to, quote_preview, mention_users, mention_all, extra, extensions, version, updated_at, text,
    sending, failed, is_local, sort_ts"#;

pub struct SqliteMessageRepo {
    pool: SqlitePool,
}

impl SqliteMessageRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    fn row_to_immessage(&self, row: &sqlx::sqlite::SqliteRow) -> Result<IMMessage> {
        let server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
        let conversation_id: String = row.try_get("conversation_id").map_err(sqlx_err)?;
        let client_msg_id: String = row.try_get("client_msg_id").map_err(sqlx_err)?;
        let sender_id: String = row.try_get("sender_id").map_err(sqlx_err)?;
        let source: i32 = row.try_get("source").map_err(sqlx_err)?;
        let seq: i64 = row.try_get("seq").map_err(sqlx_err)?;
        let timestamp: i64 = row.try_get("timestamp").map_err(sqlx_err)?;
        let client_timestamp: i64 = row.try_get("client_timestamp").map_err(sqlx_err)?;
        let conversation_type: i32 = row.try_get("conversation_type").map_err(sqlx_err)?;
        let message_type: i32 = row.try_get("message_type").map_err(sqlx_err)?;
        let channel_id: String = row
            .try_get::<Option<String>, _>("channel_id")
            .map_err(sqlx_err)?
            .unwrap_or_default();
        let sender_name: String = row.try_get("sender_name").map_err(sqlx_err)?;
        let sender_avatar: String = row.try_get("sender_avatar").map_err(sqlx_err)?;
        let sender_display_name: String = row.try_get("sender_display_name").map_err(sqlx_err)?;
        let content_bytes: Vec<u8> = row.try_get("content").map_err(sqlx_err)?;
        let status: i32 = row.try_get("status").map_err(sqlx_err)?;
        let is_read: i32 = row.try_get("is_read").map_err(sqlx_err)?;
        let is_recalled: i32 = row.try_get("is_recalled").map_err(sqlx_err)?;
        let is_edited: i32 = row.try_get("is_edited").map_err(sqlx_err)?;
        let reply_to: Option<String> = row.try_get("reply_to").map_err(sqlx_err)?;
        let quote_preview: Option<String> = row.try_get("quote_preview").map_err(sqlx_err)?;
        let mention_users_json: Option<String> = row.try_get("mention_users").map_err(sqlx_err)?;
        let mention_all: i32 = row.try_get("mention_all").map_err(sqlx_err)?;
        let extra_json: Option<String> = row.try_get("extra").map_err(sqlx_err)?;
        let extensions_json: Option<String> = row.try_get("extensions").map_err(sqlx_err)?;
        let version: i64 = row.try_get("version").map_err(sqlx_err)?;
        let updated_at: i64 = row.try_get("updated_at").map_err(sqlx_err)?;
        let sending: i32 = row.try_get("sending").map_err(sqlx_err)?;
        let failed: i32 = row.try_get("failed").map_err(sqlx_err)?;
        let is_local: i32 = row.try_get("is_local").map_err(sqlx_err)?;
        let sort_ts: i64 = row.try_get("sort_ts").map_err(sqlx_err)?;
        let text_col: Option<String> = row.try_get("text").map_err(sqlx_err)?;

        let mut extra = parse_extra(extra_json.as_deref());
        let mut content = decode_content_bytes(&content_bytes)
            .ok()
            .and_then(|decoded| decoded_content_to_elem(&decoded));
        if content.is_none() && message_type == MessageType::Text as i32 {
            if let Some(ref t) = text_col {
                let trimmed = t.trim();
                if !trimmed.is_empty() {
                    extra
                        .entry("content_text".to_string())
                        .or_insert_with(|| trimmed.to_string());
                    content = Some(Elem::Text(TextElem {
                        text: trimmed.to_string(),
                        mentions: vec![],
                    }));
                }
            }
        }

        Ok(IMMessage {
            server_id,
            client_msg_id,
            conversation_id,
            conversation_type,
            channel_id,
            sender_id,
            source,
            seq: seq.max(0) as u64,
            timestamp: timestamp.max(0) as u64,
            client_timestamp: client_timestamp.max(0) as u64,
            message_type,
            content,
            content_bytes,
            sender_name,
            sender_avatar,
            sender_display_name,
            reply_to,
            quote_preview,
            status,
            is_read: is_read != 0,
            is_recalled: is_recalled != 0,
            is_edited: is_edited != 0,
            mention_users: parse_mention_users(mention_users_json.as_deref()),
            mention_all: mention_all != 0,
            offline_push_info: None,
            extra,
            extensions: parse_extensions(extensions_json.as_deref()),
            version: version.max(0) as u64,
            updated_at: updated_at.max(0) as u64,
            local_state: MessageLocalState {
                sending: sending != 0,
                failed: failed != 0,
                is_local: is_local != 0,
                sort_ts: sort_ts.max(0) as u64,
            },
        })
    }
}

#[async_trait]
impl MessageReader for SqliteMessageRepo {
    async fn get(&self, message_id: &str) -> Result<Option<IMMessage>> {
        let row = sqlx::query(&format!(
            "SELECT {} FROM messages WHERE server_id = ?",
            MESSAGE_SELECT_COLS
        ))
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        row.map(|r| self.row_to_immessage(&r)).transpose()
    }

    async fn get_by_client_msg_id(&self, client_msg_id: &str) -> Result<Option<IMMessage>> {
        let row = sqlx::query(&format!(
            "SELECT {} FROM messages WHERE client_msg_id = ? LIMIT 1",
            MESSAGE_SELECT_COLS
        ))
        .bind(client_msg_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        row.map(|r| self.row_to_immessage(&r)).transpose()
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<IMMessage>> {
        let rows = sqlx::query(&format!(
            r#"SELECT {} FROM messages
               WHERE conversation_id = ? AND seq < ?
               ORDER BY seq DESC LIMIT ?"#,
            MESSAGE_SELECT_COLS
        ))
        .bind(conversation_id)
        .bind(before_seq_for_sqlite(before_seq))
        .bind(limit as i32)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(self.row_to_immessage(&row)?);
        }
        Ok(out)
    }

    async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<IMMessage>> {
        let kw = keyword.trim().to_lowercase();
        let rows = if kw.is_empty() {
            sqlx::query(&format!(
                "SELECT {} FROM messages ORDER BY timestamp DESC LIMIT ?",
                MESSAGE_SELECT_COLS
            ))
            .bind(limit as i32)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query(&format!(
                "SELECT {} FROM messages WHERE text IS NOT NULL AND LOWER(text) LIKE ? ORDER BY timestamp DESC LIMIT ?",
                MESSAGE_SELECT_COLS
            ))
            .bind(format!("%{}%", kw))
            .bind(limit as i32)
            .fetch_all(&self.pool)
            .await
        }
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(self.row_to_immessage(&row)?);
        }
        Ok(out)
    }
}

fn extensions_to_json(ext: &HashMap<String, Vec<u8>>) -> String {
    if ext.is_empty() {
        return String::new();
    }
    let map: HashMap<String, String> = ext
        .iter()
        .map(|(k, v)| (k.clone(), BASE64_STANDARD.encode(v)))
        .collect();
    serde_json::to_string(&map).unwrap_or_default()
}

#[async_trait]
impl MessageWriter for SqliteMessageRepo {
    async fn save_batch(&self, messages: &[IMMessage]) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        for m in messages {
            let extra_json = serde_json::to_string(&m.extra).unwrap_or_default();
            let mention_users_json = serde_json::to_string(&m.mention_users).unwrap_or_default();
            let extensions_json = extensions_to_json(&m.extensions);
            let text = m.text_for_storage();
            sqlx::query(
                r#"INSERT OR REPLACE INTO messages (
                   server_id, conversation_id, client_msg_id, sender_id, source, seq, timestamp, client_timestamp,
                   conversation_type, message_type, channel_id, sender_name, sender_avatar,
                   sender_display_name, content, status, is_read, is_recalled, is_edited,
                   reply_to, quote_preview, mention_users, mention_all, extra, extensions, version, updated_at, text,
                   sending, failed, is_local, sort_ts)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&m.server_id)
            .bind(&m.conversation_id)
            .bind(&m.client_msg_id)
            .bind(&m.sender_id)
            .bind(m.source)
            .bind(m.seq as i64)
            .bind(m.timestamp as i64)
            .bind(m.client_timestamp as i64)
            .bind(m.conversation_type)
            .bind(m.message_type)
            .bind(&m.channel_id)
            .bind(&m.sender_name)
            .bind(&m.sender_avatar)
            .bind(&m.sender_display_name)
            .bind(&m.content_bytes)
            .bind(m.status)
            .bind(if m.is_read { 1i32 } else { 0 })
            .bind(if m.is_recalled { 1i32 } else { 0 })
            .bind(if m.is_edited { 1i32 } else { 0 })
            .bind(&m.reply_to)
            .bind(&m.quote_preview)
            .bind(&mention_users_json)
            .bind(if m.mention_all { 1i32 } else { 0 })
            .bind(&extra_json)
            .bind(&extensions_json)
            .bind(m.version as i64)
            .bind(m.updated_at as i64)
            .bind(text.as_deref())
            .bind(if m.local_state.sending { 1i32 } else { 0 })
            .bind(if m.local_state.failed { 1i32 } else { 0 })
            .bind(if m.local_state.is_local { 1i32 } else { 0 })
            .bind(m.local_state.sort_ts as i64)
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn save_one(&self, message: &IMMessage) -> Result<()> {
        MessageWriter::save_batch(self, &[message.clone()]).await
    }

    async fn update_status(&self, message_id: &str, status: i32) -> Result<()> {
        sqlx::query("UPDATE messages SET status = ? WHERE server_id = ?")
            .bind(status)
            .bind(message_id)
            .execute(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn update_content(&self, message_id: &str, new_content: Vec<u8>) -> Result<()> {
        sqlx::query("UPDATE messages SET content = ? WHERE server_id = ?")
            .bind(&new_content)
            .bind(message_id)
            .execute(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, message_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM messages WHERE server_id = ?")
            .bind(message_id)
            .execute(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn update_after_ack(&self, client_msg_id: &str, message: &IMMessage) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        sqlx::query("DELETE FROM messages WHERE server_id = ?")
            .bind(client_msg_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let extra_json = serde_json::to_string(&message.extra).unwrap_or_default();
        let mention_users_json = serde_json::to_string(&message.mention_users).unwrap_or_default();
        let extensions_json = extensions_to_json(&message.extensions);
        let text = message.text_for_storage();
        sqlx::query(
            r#"INSERT OR REPLACE INTO messages (
               server_id, conversation_id, client_msg_id, sender_id, source, seq, timestamp, client_timestamp,
               conversation_type, message_type, channel_id, sender_name, sender_avatar,
               sender_display_name, content, status, is_read, is_recalled, is_edited,
               reply_to, quote_preview, mention_users, mention_all, extra, extensions, version, updated_at, text,
               sending, failed, is_local, sort_ts)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(&message.server_id)
        .bind(&message.conversation_id)
        .bind(&message.client_msg_id)
        .bind(&message.sender_id)
        .bind(message.source)
        .bind(message.seq as i64)
        .bind(message.timestamp as i64)
        .bind(message.client_timestamp as i64)
        .bind(message.conversation_type)
        .bind(message.message_type)
        .bind(&message.channel_id)
        .bind(&message.sender_name)
        .bind(&message.sender_avatar)
        .bind(&message.sender_display_name)
        .bind(&message.content_bytes)
        .bind(message.status)
        .bind(if message.is_read { 1i32 } else { 0 })
        .bind(if message.is_recalled { 1i32 } else { 0 })
        .bind(if message.is_edited { 1i32 } else { 0 })
        .bind(&message.reply_to)
        .bind(&message.quote_preview)
        .bind(&mention_users_json)
        .bind(if message.mention_all { 1i32 } else { 0 })
        .bind(&extra_json)
        .bind(&extensions_json)
        .bind(message.version as i64)
        .bind(message.updated_at as i64)
        .bind(text.as_deref())
        .bind(if message.local_state.sending { 1i32 } else { 0 })
        .bind(if message.local_state.failed { 1i32 } else { 0 })
        .bind(if message.local_state.is_local { 1i32 } else { 0 })
        .bind(message.local_state.sort_ts as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }
}

// ---------- MessageStore 适配（供 StoreProvider / 应用层使用）----------

#[async_trait]
impl MessageStore for SqliteMessageRepo {
    async fn save_batch(&self, messages: &[IMMessage]) -> Result<()> {
        MessageWriter::save_batch(self, messages).await
    }

    async fn get(&self, message_id: &str) -> Result<Option<IMMessage>> {
        MessageReader::get(self, message_id).await
    }

    async fn get_by_client_msg_id(&self, client_msg_id: &str) -> Result<Option<IMMessage>> {
        MessageReader::get_by_client_msg_id(self, client_msg_id).await
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<IMMessage>> {
        MessageReader::get_by_conversation(self, conversation_id, before_seq, limit).await
    }

    async fn update_status(&self, message_id: &str, status: i32) -> Result<()> {
        MessageWriter::update_status(self, message_id, status).await
    }

    async fn update_content(&self, message_id: &str, new_content: Vec<u8>) -> Result<()> {
        MessageWriter::update_content(self, message_id, new_content).await
    }

    async fn delete(&self, message_id: &str) -> Result<()> {
        MessageWriter::delete(self, message_id).await
    }

    async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<IMMessage>> {
        MessageReader::search(self, keyword, limit).await
    }

    async fn update_after_ack(&self, client_msg_id: &str, message: &IMMessage) -> Result<()> {
        MessageWriter::update_after_ack(self, client_msg_id, message).await
    }
}
