//! SQLite 消息仓储：与 [schema] 中 messages 表结构一致，按列读写；row 直接映射为 IMMessage（不经 ProtoMessage）。

use std::collections::HashMap;

use async_trait::async_trait;
use base64::prelude::*;
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use flare_proto::common::ReactionAction;

use crate::domain::{
    EditApplyResult, MessageReader, MessageStore, MessageWriter, OperationApplyResult,
};
use crate::error::{ErrorCode, FlareError, Result};
use crate::model::conversation::ConversationType;
use crate::model::message::{MessageLocalState, ReactionEntry, parse_reactions_from_extra};
use crate::model::{
    IMMessage, decode_content_bytes, decoded_content_to_elem, message_elem::TextElem, Elem,
};
use flare_proto::common::{MessageStatus, MessageType};

fn parse_extra(s: Option<&str>) -> HashMap<String, String> {
    let s = match s {
        Some(x) if !x.is_empty() => x,
        _ => return HashMap::new(),
    };
    serde_json::from_str(s).unwrap_or_default()
}

fn extra_to_json(extra: &HashMap<String, String>) -> String {
    serde_json::to_string(extra).unwrap_or_default()
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

/// 从 `MessageContent` 字节提取纯文本，供编辑后更新 `messages.text` 列（与 `IMMessage::text_for_storage` 语义一致）。
fn text_for_sqlite_from_content_bytes(bytes: &[u8]) -> Option<String> {
    decode_content_bytes(bytes)
        .ok()
        .and_then(|decoded| decoded_content_to_elem(&decoded))
        .and_then(|elem| {
            if let Elem::Text(t) = elem {
                Some(t.text)
            } else {
                None
            }
        })
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
            reactions: parse_reactions_from_extra(&extra),
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

fn now_ms_i64() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn conversation_display_name_from_message(message: &IMMessage) -> String {
    if !message.channel_id.trim().is_empty() {
        return message.channel_id.trim().to_string();
    }
    if !message.sender_name.trim().is_empty() {
        return message.sender_name.trim().to_string();
    }
    if !message.sender_id.trim().is_empty() {
        return message.sender_id.trim().to_string();
    }
    message.conversation_id.trim().to_string()
}

async fn upsert_conversation_snapshot_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    message: &IMMessage,
) -> Result<()> {
    let conversation_id = message.conversation_id.trim();
    if conversation_id.is_empty() {
        return Ok(());
    }

    let conv_type = ConversationType::from_proto_int(message.conversation_type);
    let conversation_type = conv_type.to_proto_int();
    let display_name = conversation_display_name_from_message(message);
    let business_type = if conv_type.is_single_chat_conversation() {
        "single"
    } else {
        "chat"
    };
    let last_message_id = if message.server_id.trim().is_empty() {
        message.client_msg_id.trim()
    } else {
        message.server_id.trim()
    };
    let last_sender_id = message.sender_id.trim();
    let last_message_at = message.timestamp as i64;
    let preview = message.text_for_storage().unwrap_or_default();
    let max_seq = message.seq as i64;
    let now = now_ms_i64();
    let created_at = if last_message_at > 0 { last_message_at } else { now };
    let updated_at = if last_message_at > 0 { last_message_at } else { now };

    sqlx::query(
        r#"INSERT INTO conversations (
               conversation_id, conversation_type, business_type, channel_id, members_count,
               display_name, avatar_url, remark, description,
               last_message_id, last_sender_id, last_message_at, last_message_preview,
               last_sender_nickname, last_sender_avatar_url,
               unread_count, last_read_seq, max_seq,
               is_pinned, is_muted, is_archived,
               version, updated_at, created_at, updated_at_ts,
               ext, draft, mention_count, mention_me, badge, role
           ) VALUES (
               ?, ?, ?, ?, 0,
               ?, '', NULL, NULL,
               ?, ?, ?, ?,
               '', '',
               0, 0, ?,
               0, 0, 0,
               0, ?, ?, ?,
               '', NULL, 0, 0, NULL, NULL
           )
           ON CONFLICT(conversation_id) DO UPDATE SET
               conversation_type = CASE
                   WHEN conversations.conversation_type = 0 AND excluded.conversation_type != 0 THEN excluded.conversation_type
                   ELSE conversations.conversation_type
               END,
               business_type = CASE
                   WHEN conversations.business_type = '' THEN excluded.business_type
                   ELSE conversations.business_type
               END,
               channel_id = CASE
                   WHEN conversations.channel_id = '' THEN excluded.channel_id
                   ELSE conversations.channel_id
               END,
               display_name = CASE
                   WHEN conversations.display_name = '' THEN excluded.display_name
                   ELSE conversations.display_name
               END,
               avatar_url = CASE
                   WHEN conversations.avatar_url = '' THEN excluded.avatar_url
                   ELSE conversations.avatar_url
               END,
               last_message_id = CASE
                   WHEN COALESCE(conversations.last_message_at, 0) <= COALESCE(excluded.last_message_at, 0) THEN excluded.last_message_id
                   ELSE conversations.last_message_id
               END,
               last_sender_id = CASE
                   WHEN COALESCE(conversations.last_message_at, 0) <= COALESCE(excluded.last_message_at, 0) THEN excluded.last_sender_id
                   ELSE conversations.last_sender_id
               END,
               last_message_at = CASE
                   WHEN COALESCE(conversations.last_message_at, 0) <= COALESCE(excluded.last_message_at, 0) THEN excluded.last_message_at
                   ELSE conversations.last_message_at
               END,
               last_message_preview = CASE
                   WHEN COALESCE(conversations.last_message_at, 0) <= COALESCE(excluded.last_message_at, 0) THEN excluded.last_message_preview
                   ELSE conversations.last_message_preview
               END,
               max_seq = MAX(COALESCE(conversations.max_seq, 0), COALESCE(excluded.max_seq, 0)),
               updated_at = MAX(COALESCE(conversations.updated_at, 0), COALESCE(excluded.updated_at, 0)),
               updated_at_ts = MAX(COALESCE(conversations.updated_at_ts, 0), COALESCE(excluded.updated_at_ts, 0))
        "#,
    )
    .bind(conversation_id)
    .bind(conversation_type)
    .bind(business_type)
    .bind(&message.channel_id)
    .bind(&display_name)
    .bind(last_message_id)
    .bind(last_sender_id)
    .bind(last_message_at)
    .bind(&preview)
    .bind(max_seq)
    .bind(updated_at)
    .bind(created_at)
    .bind(updated_at)
    .execute(&mut **tx)
    .await
    .map_err(sqlx_err)?;
    Ok(())
}

async fn replace_reaction_snapshot_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    message: &IMMessage,
) -> Result<()> {
    if message.server_id.is_empty() {
        return Ok(());
    }
    sqlx::query("DELETE FROM message_reactions WHERE message_server_id = ?")
        .bind(&message.server_id)
        .execute(&mut **tx)
        .await
        .map_err(sqlx_err)?;
    if message.reactions.is_empty() {
        return Ok(());
    }
    let now = now_ms_i64();
    for reaction in &message.reactions {
        if reaction.emoji.trim().is_empty() {
            continue;
        }
        for uid in &reaction.user_ids {
            if uid.trim().is_empty() {
                continue;
            }
            sqlx::query(
                r#"INSERT OR REPLACE INTO message_reactions
                   (message_server_id, conversation_id, emoji, user_id, created_at, updated_at)
                   VALUES (?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&message.server_id)
            .bind(&message.conversation_id)
            .bind(&reaction.emoji)
            .bind(uid)
            .bind(now)
            .bind(now)
            .execute(&mut **tx)
            .await
            .map_err(sqlx_err)?;
        }
    }
    Ok(())
}

#[async_trait]
impl MessageWriter for SqliteMessageRepo {
    async fn save_batch(&self, messages: &[IMMessage]) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let mut latest_per_conversation: HashMap<&str, &IMMessage> = HashMap::new();
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
            replace_reaction_snapshot_tx(&mut tx, m).await?;

            let conv_id = m.conversation_id.trim();
            if conv_id.is_empty() {
                continue;
            }
            match latest_per_conversation.get(conv_id) {
                Some(prev)
                    if prev.timestamp > m.timestamp
                        || (prev.timestamp == m.timestamp && prev.seq >= m.seq) => {}
                _ => {
                    latest_per_conversation.insert(conv_id, m);
                }
            }
        }

        for (_, latest) in latest_per_conversation {
            upsert_conversation_snapshot_tx(&mut tx, latest).await?;
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
        let recalled = MessageStatus::Recalled as i32;
        let read = MessageStatus::Read as i32;
        if status == recalled {
            sqlx::query(
                r#"UPDATE messages SET status = ?, is_recalled = 1
                   WHERE server_id = ? OR client_msg_id = ?"#,
            )
            .bind(status)
            .bind(message_id)
            .bind(message_id)
            .execute(&self.pool)
            .await
        } else if status == read {
            sqlx::query(
                r#"UPDATE messages SET status = ?, is_read = 1
                   WHERE server_id = ? OR client_msg_id = ?"#,
            )
            .bind(status)
            .bind(message_id)
            .bind(message_id)
            .execute(&self.pool)
            .await
        } else {
            sqlx::query(
                "UPDATE messages SET status = ? WHERE server_id = ? OR client_msg_id = ?",
            )
            .bind(status)
            .bind(message_id)
            .bind(message_id)
            .execute(&self.pool)
            .await
        }
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn update_content(&self, message_id: &str, new_content: Vec<u8>) -> Result<bool> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let text = text_for_sqlite_from_content_bytes(&new_content);
        let rows = sqlx::query(
            r#"UPDATE messages SET content = ?, is_edited = 1, text = ?, updated_at = ?
               WHERE server_id = ? OR client_msg_id = ?"#,
        )
        .bind(&new_content)
        .bind(text.as_deref())
        .bind(now_ms)
        .bind(message_id)
        .bind(message_id)
        .execute(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?
        .rows_affected();
        if rows == 0 {
            tracing::warn!(
                message_id = %message_id,
                "update_content: no row matched server_id/client_msg_id"
            );
        }
        Ok(rows > 0)
    }

    async fn delete(&self, message_id: &str) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        sqlx::query("DELETE FROM message_reactions WHERE message_server_id = ?")
            .bind(message_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        sqlx::query("DELETE FROM messages WHERE server_id = ?")
            .bind(message_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        tx.commit()
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
        sqlx::query(
            r#"UPDATE message_reactions
               SET message_server_id = ?, conversation_id = ?, updated_at = ?
               WHERE message_server_id = ?"#,
        )
        .bind(&message.server_id)
        .bind(&message.conversation_id)
        .bind(now_ms_i64())
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
        replace_reaction_snapshot_tx(&mut tx, message).await?;
        upsert_conversation_snapshot_tx(&mut tx, message).await?;
        tx.commit()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl MessageStore for SqliteMessageRepo {
    async fn apply_edit_event(
        &self,
        message_id: &str,
        new_content: Vec<u8>,
        edit_version: i32,
    ) -> Result<EditApplyResult> {
        let row = sqlx::query(
            r#"SELECT extra
               FROM messages
               WHERE server_id = ? OR client_msg_id = ?
               LIMIT 1"#,
        )
        .bind(message_id)
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let Some(row) = row else {
            return Ok(EditApplyResult::NotFound);
        };

        let extra_raw: Option<String> = row.try_get("extra").map_err(sqlx_err)?;
        let mut extra = parse_extra(extra_raw.as_deref());
        let current_edit_version = extra
            .get("current_edit_version")
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(0);
        if edit_version > 0 && current_edit_version > 0 && edit_version <= current_edit_version {
            return Ok(EditApplyResult::IgnoredStale);
        }

        let now_ms = now_ms_i64();
        let text = text_for_sqlite_from_content_bytes(&new_content);
        let next_edit_version = if edit_version > 0 {
            edit_version.max(current_edit_version)
        } else {
            current_edit_version.max(1)
        };
        extra.insert("current_edit_version".to_string(), next_edit_version.to_string());
        extra.insert("message_fsm_state".to_string(), "EDITED".to_string());
        extra.insert("last_edited_at".to_string(), now_ms.to_string());

        let rows = sqlx::query(
            r#"UPDATE messages
               SET content = ?, is_edited = 1, text = ?, updated_at = ?, extra = ?
               WHERE server_id = ? OR client_msg_id = ?"#,
        )
        .bind(&new_content)
        .bind(text.as_deref())
        .bind(now_ms)
        .bind(extra_to_json(&extra))
        .bind(message_id)
        .bind(message_id)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        Ok(if rows > 0 {
            EditApplyResult::Applied
        } else {
            EditApplyResult::NotFound
        })
    }

    async fn mark_outgoing_read_upto_seq(
        &self,
        conversation_id: &str,
        sender_user_id: &str,
        read_seq: u64,
    ) -> Result<()> {
        if conversation_id.trim().is_empty() || sender_user_id.trim().is_empty() || read_seq == 0 {
            return Ok(());
        }
        let sent = MessageStatus::Sent as i32;
        let delivered = MessageStatus::Delivered as i32;
        let read = MessageStatus::Read as i32;
        sqlx::query(
            r#"UPDATE messages
               SET status = ?, is_read = 1
               WHERE conversation_id = ?
                 AND sender_id = ?
                 AND seq > 0
                 AND seq <= ?
                 AND status IN (?, ?, ?)"#,
        )
        .bind(read)
        .bind(conversation_id)
        .bind(sender_user_id)
        .bind(read_seq as i64)
        .bind(sent)
        .bind(delivered)
        .bind(read)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn apply_reaction(
        &self,
        conversation_id: &str,
        message_server_id: &str,
        user_id: &str,
        emoji: &str,
        action: i32,
    ) -> Result<()> {
        if message_server_id.is_empty() || user_id.is_empty() || emoji.is_empty() {
            return Ok(());
        }
        let add_action = ReactionAction::Add as i32;
        let remove_action = ReactionAction::Remove as i32;
        if action != add_action && action != remove_action {
            return Ok(());
        }
        let now = now_ms_i64();
        if action == remove_action {
            sqlx::query(
                r#"DELETE FROM message_reactions
                   WHERE message_server_id = ? AND emoji = ? AND user_id = ?"#,
            )
            .bind(message_server_id)
            .bind(emoji)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
            return Ok(());
        }
        sqlx::query(
            r#"INSERT OR REPLACE INTO message_reactions
               (message_server_id, conversation_id, emoji, user_id, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?)"#,
        )
        .bind(message_server_id)
        .bind(conversation_id)
        .bind(emoji)
        .bind(user_id)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn apply_reaction_event(
        &self,
        conversation_id: &str,
        message_server_id: &str,
        user_id: &str,
        emoji: &str,
        action: i32,
        event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        if message_server_id.is_empty() || user_id.is_empty() || emoji.is_empty() {
            return Ok(OperationApplyResult::NotFound);
        }
        let add_action = ReactionAction::Add as i32;
        let remove_action = ReactionAction::Remove as i32;
        if action != add_action && action != remove_action {
            return Ok(OperationApplyResult::NotFound);
        }

        let seq_key = format!("last_reaction_event_seq:{user_id}:{emoji}");
        let applied = apply_message_extra_with_seq(
            &self.pool,
            message_server_id,
            &seq_key,
            event_seq,
            |_| {},
        )
        .await?;
        if applied != OperationApplyResult::Applied {
            return Ok(applied);
        }

        let now = now_ms_i64();
        if action == remove_action {
            sqlx::query(
                r#"DELETE FROM message_reactions
                   WHERE message_server_id = ? AND emoji = ? AND user_id = ?"#,
            )
            .bind(message_server_id)
            .bind(emoji)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
            return Ok(OperationApplyResult::Applied);
        }
        sqlx::query(
            r#"INSERT OR REPLACE INTO message_reactions
               (message_server_id, conversation_id, emoji, user_id, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?)"#,
        )
        .bind(message_server_id)
        .bind(conversation_id)
        .bind(emoji)
        .bind(user_id)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(OperationApplyResult::Applied)
    }

    async fn apply_delete_event(
        &self,
        message_id: &str,
        event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        let row = sqlx::query(
            r#"SELECT server_id, extra
               FROM messages
               WHERE server_id = ? OR client_msg_id = ?
               LIMIT 1"#,
        )
        .bind(message_id)
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let Some(row) = row else {
            return Ok(OperationApplyResult::NotFound);
        };

        let extra_raw: Option<String> = row.try_get("extra").map_err(sqlx_err)?;
        let extra = parse_extra(extra_raw.as_deref());
        if is_stale_operation(&extra, "last_delete_event_seq", event_seq) {
            return Ok(OperationApplyResult::IgnoredStale);
        }

        self.delete(message_id).await?;
        Ok(OperationApplyResult::Applied)
    }

    async fn apply_pin_event(
        &self,
        message_id: &str,
        enabled: bool,
        event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        apply_message_extra_with_seq(
            &self.pool,
            message_id,
            "last_pin_event_seq",
            event_seq,
            |extra| {
                extra.insert(
                    "pinned".to_string(),
                    if enabled { "true" } else { "false" }.to_string(),
                );
            },
        )
        .await
    }

    async fn apply_mark_event(
        &self,
        message_id: &str,
        mark_type: i32,
        color: Option<&str>,
        set_mark: bool,
        event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        let seq_key = format!("last_mark_event_seq:{mark_type}");
        apply_message_extra_with_seq(&self.pool, message_id, &seq_key, event_seq, |extra| {
            if set_mark {
                extra.insert("mark_type".to_string(), mark_type.to_string());
                if let Some(c) = color {
                    if !c.trim().is_empty() {
                        extra.insert("mark_color".to_string(), c.trim().to_string());
                    } else {
                        extra.remove("mark_color");
                    }
                } else {
                    extra.remove("mark_color");
                }
            } else {
                extra.remove("mark_type");
                extra.remove("mark_color");
            }
        })
        .await
    }

    async fn list_reactions(
        &self,
        message_server_ids: &[String],
    ) -> Result<HashMap<String, Vec<ReactionEntry>>> {
        if message_server_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut qb = QueryBuilder::<Sqlite>::new(
            "SELECT message_server_id, emoji, user_id FROM message_reactions WHERE message_server_id IN (",
        );
        let mut separated = qb.separated(", ");
        for id in message_server_ids {
            separated.push_bind(id);
        }
        qb.push(") ORDER BY updated_at ASC");
        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        let mut grouped: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();
        for row in rows {
            let msg_id: String = row.try_get("message_server_id").map_err(sqlx_err)?;
            let emoji: String = row.try_get("emoji").map_err(sqlx_err)?;
            let user_id: String = row.try_get("user_id").map_err(sqlx_err)?;
            grouped
                .entry(msg_id)
                .or_default()
                .entry(emoji)
                .or_default()
                .push(user_id);
        }

        let mut out: HashMap<String, Vec<ReactionEntry>> = HashMap::new();
        for (msg_id, emoji_map) in grouped {
            let mut reactions = Vec::with_capacity(emoji_map.len());
            for (emoji, user_ids) in emoji_map {
                reactions.push(ReactionEntry {
                    emoji,
                    count: user_ids.len() as u32,
                    user_ids,
                });
            }
            out.insert(msg_id, reactions);
        }
        Ok(out)
    }

    async fn set_message_flag(&self, message_id: &str, flag_key: &str, enabled: bool) -> Result<()> {
        if message_id.trim().is_empty() || flag_key.trim().is_empty() {
            return Ok(());
        }
        let rows = sqlx::query(
            r#"SELECT server_id, extra FROM messages
               WHERE server_id = ? OR client_msg_id = ?"#,
        )
        .bind(message_id)
        .bind(message_id)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if rows.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await.map_err(sqlx_err)?;
        for row in rows {
            let server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
            let extra_raw: Option<String> = row.try_get("extra").map_err(sqlx_err)?;
            let mut extra = parse_extra(extra_raw.as_deref());
            extra.insert(
                flag_key.to_string(),
                if enabled { "true" } else { "false" }.to_string(),
            );
            sqlx::query("UPDATE messages SET extra = ? WHERE server_id = ?")
                .bind(extra_to_json(&extra))
                .bind(server_id)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;
        }
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn set_message_mark(
        &self,
        message_id: &str,
        mark_type: i32,
        color: Option<&str>,
    ) -> Result<()> {
        let rows = sqlx::query(
            r#"SELECT server_id, extra FROM messages
               WHERE server_id = ? OR client_msg_id = ?"#,
        )
        .bind(message_id)
        .bind(message_id)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if rows.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await.map_err(sqlx_err)?;
        for row in rows {
            let server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
            let extra_raw: Option<String> = row.try_get("extra").map_err(sqlx_err)?;
            let mut extra = parse_extra(extra_raw.as_deref());
            extra.insert("mark_type".to_string(), mark_type.to_string());
            if let Some(c) = color {
                if !c.trim().is_empty() {
                    extra.insert("mark_color".to_string(), c.trim().to_string());
                } else {
                    extra.remove("mark_color");
                }
            } else {
                extra.remove("mark_color");
            }
            sqlx::query("UPDATE messages SET extra = ? WHERE server_id = ?")
                .bind(extra_to_json(&extra))
                .bind(server_id)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;
        }
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn clear_message_mark(&self, message_id: &str, _mark_type: i32) -> Result<()> {
        let rows = sqlx::query(
            r#"SELECT server_id, extra FROM messages
               WHERE server_id = ? OR client_msg_id = ?"#,
        )
        .bind(message_id)
        .bind(message_id)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if rows.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await.map_err(sqlx_err)?;
        for row in rows {
            let server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
            let extra_raw: Option<String> = row.try_get("extra").map_err(sqlx_err)?;
            let mut extra = parse_extra(extra_raw.as_deref());
            extra.remove("mark_type");
            extra.remove("mark_color");
            sqlx::query("UPDATE messages SET extra = ? WHERE server_id = ?")
                .bind(extra_to_json(&extra))
                .bind(server_id)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;
        }
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn heal_orphan_sending_messages(
        &self,
        sender_user_id: &str,
        pending_client_msg_ids: &[String],
    ) -> Result<Vec<String>> {
        if sender_user_id.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut select_qb = QueryBuilder::<Sqlite>::new(
            "SELECT client_msg_id FROM messages WHERE sending = 1 AND failed = 0 AND is_local = 1 AND sender_id = ",
        );
        select_qb.push_bind(sender_user_id);
        if !pending_client_msg_ids.is_empty() {
            select_qb.push(" AND client_msg_id NOT IN (");
            let mut separated = select_qb.separated(", ");
            for id in pending_client_msg_ids {
                separated.push_bind(id);
            }
            select_qb.push(")");
        }
        let orphan_rows = select_qb
            .build_query_as::<(String,)>()
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        if orphan_rows.is_empty() {
            return Ok(Vec::new());
        }

        let orphan_client_ids: Vec<String> = orphan_rows.into_iter().map(|(id,)| id).collect();
        let mut update_qb = QueryBuilder::<Sqlite>::new(
            "UPDATE messages SET sending = 0, failed = 1, status = ",
        );
        update_qb.push_bind(MessageStatus::Failed as i32);
        update_qb.push(", updated_at = ");
        update_qb.push_bind(now_ms_i64());
        update_qb.push(" WHERE client_msg_id IN (");
        let mut separated = update_qb.separated(", ");
        for id in &orphan_client_ids {
            separated.push_bind(id);
        }
        update_qb.push(")");
        let query = update_qb.build();
        query.execute(&self.pool).await.map_err(sqlx_err)?;
        Ok(orphan_client_ids)
    }

    async fn heal_cross_account_pending_messages(
        &self,
        sender_user_id: &str,
        pending_client_msg_ids: &[String],
    ) -> Result<Vec<String>> {
        if sender_user_id.trim().is_empty() || pending_client_msg_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut select_qb = QueryBuilder::<Sqlite>::new(
            "SELECT client_msg_id FROM messages WHERE client_msg_id IN (",
        );
        {
            let mut separated = select_qb.separated(", ");
            for id in pending_client_msg_ids {
                separated.push_bind(id);
            }
        }
        select_qb.push(") AND (sender_id = '' OR sender_id != ");
        select_qb.push_bind(sender_user_id);
        select_qb.push(")");
        let mismatched_rows = select_qb
            .build_query_as::<(String,)>()
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        if mismatched_rows.is_empty() {
            return Ok(Vec::new());
        }
        let mismatched_client_ids: Vec<String> =
            mismatched_rows.into_iter().map(|(id,)| id).collect();

        let mut tx = self.pool.begin().await.map_err(sqlx_err)?;
        let mut update_qb = QueryBuilder::<Sqlite>::new(
            "UPDATE messages SET sending = 0, failed = 1, status = ",
        );
        update_qb.push_bind(MessageStatus::Failed as i32);
        update_qb.push(", updated_at = ");
        update_qb.push_bind(now_ms_i64());
        update_qb.push(" WHERE client_msg_id IN (");
        {
            let mut separated = update_qb.separated(", ");
            for id in &mismatched_client_ids {
                separated.push_bind(id);
            }
        }
        update_qb.push(")");
        update_qb
            .build()
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        let mut delete_qb =
            QueryBuilder::<Sqlite>::new("DELETE FROM pending_sends WHERE client_msg_id IN (");
        {
            let mut separated = delete_qb.separated(", ");
            for id in &mismatched_client_ids {
                separated.push_bind(id);
            }
        }
        delete_qb.push(")");
        delete_qb
            .build()
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(mismatched_client_ids)
    }
}

fn is_stale_operation(
    extra: &HashMap<String, String>,
    seq_key: &str,
    incoming_event_seq: Option<u64>,
) -> bool {
    let Some(incoming_event_seq) = incoming_event_seq.filter(|seq| *seq > 0) else {
        return false;
    };
    let current_event_seq = extra
        .get(seq_key)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    current_event_seq > 0 && incoming_event_seq <= current_event_seq
}

async fn apply_message_extra_with_seq<F>(
    pool: &SqlitePool,
    message_id: &str,
    seq_key: &str,
    incoming_event_seq: Option<u64>,
    mut apply: F,
) -> Result<OperationApplyResult>
where
    F: FnMut(&mut HashMap<String, String>),
{
    let rows = sqlx::query(
        r#"SELECT server_id, extra FROM messages
           WHERE server_id = ? OR client_msg_id = ?"#,
    )
    .bind(message_id)
    .bind(message_id)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    if rows.is_empty() {
        return Ok(OperationApplyResult::NotFound);
    }

    let mut tx = pool.begin().await.map_err(sqlx_err)?;
    let mut applied_any = false;
    for row in rows {
        let server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
        let extra_raw: Option<String> = row.try_get("extra").map_err(sqlx_err)?;
        let mut extra = parse_extra(extra_raw.as_deref());
        if is_stale_operation(&extra, seq_key, incoming_event_seq) {
            continue;
        }
        if let Some(seq) = incoming_event_seq.filter(|seq| *seq > 0) {
            extra.insert(seq_key.to_string(), seq.to_string());
        }
        apply(&mut extra);
        sqlx::query("UPDATE messages SET extra = ? WHERE server_id = ?")
            .bind(extra_to_json(&extra))
            .bind(server_id)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        applied_any = true;
    }
    tx.commit().await.map_err(sqlx_err)?;

    Ok(if applied_any {
        OperationApplyResult::Applied
    } else {
        OperationApplyResult::IgnoredStale
    })
}

#[cfg(test)]
mod tests {
    use super::SqliteMessageRepo;
    use crate::domain::{
        EditApplyResult, MessageReader, MessageStore, MessageWriter, OperationApplyResult,
    };
    use crate::model::IMMessage;
    use crate::model::message::ReactionAction;
    use crate::store::sqlite_init_schema;
    use sqlx::SqlitePool;

    async fn make_repo() -> SqliteMessageRepo {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite_init_schema(&pool).await.unwrap();
        SqliteMessageRepo::new(pool)
    }

    #[tokio::test]
    async fn apply_edit_event_ignores_stale_version_and_accepts_newer_version() {
        let repo = make_repo().await;
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-1".to_string();
        message.client_msg_id = "client-1".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();
        message.content_bytes = b"old".to_vec();
        message
            .extra
            .insert("current_edit_version".to_string(), "2".to_string());
        repo.save_batch(&[message.clone()]).await.unwrap();

        let stale = repo
            .apply_edit_event("server-1", b"stale".to_vec(), 1)
            .await
            .unwrap();
        assert_eq!(stale, EditApplyResult::IgnoredStale);
        let after_stale = repo.get("server-1").await.unwrap().unwrap();
        assert_eq!(after_stale.content_bytes, b"old".to_vec());

        let newer = repo
            .apply_edit_event("server-1", b"new".to_vec(), 3)
            .await
            .unwrap();
        assert_eq!(newer, EditApplyResult::Applied);
        let after_new = repo.get("server-1").await.unwrap().unwrap();
        assert_eq!(after_new.content_bytes, b"new".to_vec());
        assert_eq!(
            after_new.extra.get("current_edit_version").map(String::as_str),
            Some("3")
        );
    }

    #[tokio::test]
    async fn apply_pin_event_ignores_stale_seq_and_accepts_newer_seq() {
        let repo = make_repo().await;
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-2".to_string();
        message.client_msg_id = "client-2".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();
        repo.save_batch(&[message]).await.unwrap();

        let applied = repo
            .apply_pin_event("server-2", true, Some(10))
            .await
            .unwrap();
        assert_eq!(applied, OperationApplyResult::Applied);

        let stale = repo
            .apply_pin_event("server-2", false, Some(9))
            .await
            .unwrap();
        assert_eq!(stale, OperationApplyResult::IgnoredStale);

        let after_stale = repo.get("server-2").await.unwrap().unwrap();
        assert_eq!(after_stale.extra.get("pinned").map(String::as_str), Some("true"));

        let newer = repo
            .apply_pin_event("server-2", false, Some(11))
            .await
            .unwrap();
        assert_eq!(newer, OperationApplyResult::Applied);
        let after_new = repo.get("server-2").await.unwrap().unwrap();
        assert_eq!(after_new.extra.get("pinned").map(String::as_str), Some("false"));
        assert_eq!(
            after_new
                .extra
                .get("last_pin_event_seq")
                .map(String::as_str),
            Some("11")
        );
    }

    #[tokio::test]
    async fn apply_mark_event_ignores_stale_seq_and_accepts_newer_seq() {
        let repo = make_repo().await;
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-3".to_string();
        message.client_msg_id = "client-3".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();
        repo.save_batch(&[message]).await.unwrap();

        let applied = repo
            .apply_mark_event("server-3", 7, Some("#ff0000"), true, Some(20))
            .await
            .unwrap();
        assert_eq!(applied, OperationApplyResult::Applied);

        let stale = repo
            .apply_mark_event("server-3", 7, None, false, Some(19))
            .await
            .unwrap();
        assert_eq!(stale, OperationApplyResult::IgnoredStale);

        let after_stale = repo.get("server-3").await.unwrap().unwrap();
        assert_eq!(after_stale.extra.get("mark_type").map(String::as_str), Some("7"));
        assert_eq!(after_stale.extra.get("mark_color").map(String::as_str), Some("#ff0000"));

        let newer = repo
            .apply_mark_event("server-3", 7, None, false, Some(21))
            .await
            .unwrap();
        assert_eq!(newer, OperationApplyResult::Applied);

        let after_new = repo.get("server-3").await.unwrap().unwrap();
        assert!(after_new.extra.get("mark_type").is_none());
        assert!(after_new.extra.get("mark_color").is_none());
        assert_eq!(
            after_new
                .extra
                .get("last_mark_event_seq:7")
                .map(String::as_str),
            Some("21")
        );
    }

    #[tokio::test]
    async fn apply_reaction_event_ignores_stale_seq_and_accepts_newer_seq() {
        let repo = make_repo().await;
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-4".to_string();
        message.client_msg_id = "client-4".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();
        repo.save_batch(&[message]).await.unwrap();

        let applied = repo
            .apply_reaction_event(
                "conv-1",
                "server-4",
                "u2",
                "👍",
                ReactionAction::Add as i32,
                Some(30),
            )
            .await
            .unwrap();
        assert_eq!(applied, OperationApplyResult::Applied);

        let stale = repo
            .apply_reaction_event(
                "conv-1",
                "server-4",
                "u2",
                "👍",
                ReactionAction::Remove as i32,
                Some(29),
            )
            .await
            .unwrap();
        assert_eq!(stale, OperationApplyResult::IgnoredStale);

        let reactions = repo
            .list_reactions(&["server-4".to_string()])
            .await
            .unwrap();
        assert_eq!(
            reactions
                .get("server-4")
                .and_then(|entries| entries.iter().find(|entry| entry.emoji == "👍"))
                .map(|entry| entry.count),
            Some(1)
        );

        let newer = repo
            .apply_reaction_event(
                "conv-1",
                "server-4",
                "u2",
                "👍",
                ReactionAction::Remove as i32,
                Some(31),
            )
            .await
            .unwrap();
        assert_eq!(newer, OperationApplyResult::Applied);

        let reactions_after_remove = repo
            .list_reactions(&["server-4".to_string()])
            .await
            .unwrap();
        assert!(reactions_after_remove.get("server-4").is_none());
    }
}
