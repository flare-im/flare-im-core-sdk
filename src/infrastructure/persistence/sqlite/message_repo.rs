//! SQLite 消息仓储：与 [schema] 中 messages 表结构一致，按列读写；row 直接映射为 IMMessage（不经 ProtoMessage）。

use std::collections::HashMap;

use async_trait::async_trait;
use base64::prelude::*;
use flare_proto::common::ReactionAction;
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use tracing::debug;

use crate::domain::{
    EditApplyResult, MessageDeliveryService, MessageReader, MessageStore, MessageWriter,
    OperationApplyResult, local_cleared_through_seq, message_visible_after_clear,
};
use crate::model::conversation::ConversationType;
use crate::model::message::{
    MessageLocalState, ReactionEntry, has_reaction_snapshot_in_extra, parse_reactions_from_extra,
};
use crate::model::search::escaped_like_contains;
use crate::model::{
    Elem, IMMessage, MessageSearchKind, MessageSearchQuery, decode_content_bytes,
    decoded_content_to_elem, message_elem::TextElem,
};
use crate::shared::error::{ErrorCode, FlareError, Result};
use flare_proto::common::{BurnStatus, MessageStatus, MessageType};

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
/// - **`0`**：表示客户端刚打开会话、尚无游标，等价于「上界无穷」，取当前库中 **最新一页**（与 Tauri `INITIAL_BEFORE_SEQ` 语义对齐，客户端可直接传 `0`）。
///   最新一页在仓储层按 **`max(sort_ts, timestamp, client_timestamp) DESC`**（见 `get_by_conversation`），与 `effective_sort_ts_for_persist` 一致，**不**伪造服务端 `seq`。
/// - **`u64::MAX`**：不可直接 `as i64`（会变成 `-1`，导致 `seq < -1` 恒空），钳制到 `i64::MAX`。
/// - 其它正值：`seq < before_seq`，用于「加载更早消息」。
fn before_seq_for_sqlite(before_seq: u64) -> i64 {
    if before_seq == 0 || before_seq >= i64::MAX as u64 {
        i64::MAX
    } else {
        before_seq as i64
    }
}

fn u64_to_i64_saturating(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

/// 写入 `messages.sort_ts` 的最终值：仅用于**本地列表**「最新一页」排序，**不**参与多端 `seq` 同步语义。
///
/// 取 `max(入队/本地 sort_ts, 服务端/客户端 timestamp, client_timestamp, 墙钟)`，避免仅保留较小入队时间而弱于历史消息、被 `LIMIT` 裁掉。
fn effective_sort_ts_for_persist(message: &IMMessage) -> i64 {
    let wall = now_ms_i64().max(0) as u64;
    let merged = message
        .local_state
        .sort_ts
        .max(message.timestamp)
        .max(message.client_timestamp)
        .max(wall);
    u64_to_i64_saturating(merged)
}

fn message_preview_for_storage(message: &IMMessage) -> Option<String> {
    message.text_for_storage().or_else(|| {
        message
            .extra
            .get("contentText")
            .map(|s| s.trim())
            .filter(|s| !crate::model::preview_storage::is_redundant_content_text_extra(s))
            .map(str::to_string)
    })
}

fn conversation_projection_ts(message: &IMMessage) -> i64 {
    if message.seq > 0 {
        let server_time = if message.timestamp > 0 {
            message.timestamp
        } else if message.client_timestamp > 0 {
            message.client_timestamp
        } else {
            message.local_state.sort_ts
        };
        return u64_to_i64_saturating(server_time);
    }

    let merged = message
        .timestamp
        .max(message.client_timestamp)
        .max(message.local_state.sort_ts);
    if merged > 0 {
        u64_to_i64_saturating(merged)
    } else {
        effective_sort_ts_for_persist(message)
    }
}

fn should_replace_conversation_projection(prev: &IMMessage, candidate: &IMMessage) -> bool {
    match (prev.seq > 0, candidate.seq > 0) {
        (true, true) => {
            candidate.seq > prev.seq
                || (candidate.seq == prev.seq
                    && effective_sort_ts_for_persist(candidate)
                        >= effective_sort_ts_for_persist(prev))
        }
        _ => {
            let candidate_sort = effective_sort_ts_for_persist(candidate);
            let prev_sort = effective_sort_ts_for_persist(prev);
            candidate_sort > prev_sort || (candidate_sort == prev_sort && candidate.seq >= prev.seq)
        }
    }
}

fn search_effective_time_sql(prefix: &str) -> String {
    format!(
        "COALESCE(NULLIF({prefix}.timestamp, 0), NULLIF({prefix}.client_timestamp, 0), NULLIF({prefix}.sort_ts, 0), 0)"
    )
}

fn message_type_values_for_search(kinds: &[MessageSearchKind]) -> Vec<i32> {
    let mut values = Vec::new();
    for kind in kinds {
        match kind {
            MessageSearchKind::Message => return Vec::new(),
            MessageSearchKind::Text => {
                values.push(MessageType::Text as i32);
                values.push(MessageType::RichText as i32);
                values.push(MessageType::Quote as i32);
            }
            MessageSearchKind::Media => {
                values.push(MessageType::Image as i32);
                values.push(MessageType::Video as i32);
                values.push(MessageType::Audio as i32);
                values.push(MessageType::File as i32);
                values.push(MessageType::ImageGroup as i32);
            }
            MessageSearchKind::Image => {
                values.push(MessageType::Image as i32);
                values.push(MessageType::ImageGroup as i32);
            }
            MessageSearchKind::Video => values.push(MessageType::Video as i32),
            MessageSearchKind::Audio => values.push(MessageType::Audio as i32),
            MessageSearchKind::File => values.push(MessageType::File as i32),
        }
    }
    values.sort_unstable();
    values.dedup();
    values
}

/// 从 `MessageContent` 字节解码后仅取 Text 正文，供编辑等路径更新 `messages.text`（会话列表/预览列为 JSON 载荷，见 [`IMMessage::text_for_storage`]）。
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
    sender_name, sender_avatar, sender_display_name, content, status,
    burn_enabled, burn_after_read_seconds, burn_status, first_read_at, burn_at, burned_at,
    is_read, is_recalled, is_edited,
    reply_to, quote_preview, mention_users, mention_all, extra, extensions, version, updated_at, text,
    sending, failed, is_local, sort_ts"#;

pub struct SqliteMessageRepo {
    pool: SqlitePool,
}

fn parse_conversation_ext_json(s: Option<&str>) -> HashMap<String, String> {
    let s = match s {
        Some(x) if !x.is_empty() => x,
        _ => return HashMap::new(),
    };
    serde_json::from_str(s).unwrap_or_default()
}

impl SqliteMessageRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn local_cleared_floor(&self, conversation_id: &str) -> Result<u64> {
        let row: Option<(Option<String>, i64)> =
            sqlx::query_as(
                "SELECT ext, COALESCE(visible_after_seq, 0) FROM conversations WHERE conversation_id = ? LIMIT 1",
            )
                .bind(conversation_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let Some((ext, visible_after_seq)) = row else {
            return Ok(0);
        };
        Ok(
            local_cleared_through_seq(&parse_conversation_ext_json(ext.as_deref()))
                .max(visible_after_seq.max(0) as u64),
        )
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
        let burn_enabled: i32 = row.try_get("burn_enabled").map_err(sqlx_err)?;
        let burn_after_read_seconds: Option<i64> =
            row.try_get("burn_after_read_seconds").map_err(sqlx_err)?;
        let burn_status: i32 = row.try_get("burn_status").map_err(sqlx_err)?;
        let first_read_at: Option<i64> = row.try_get("first_read_at").map_err(sqlx_err)?;
        let burn_at: Option<i64> = row.try_get("burn_at").map_err(sqlx_err)?;
        let burned_at: Option<i64> = row.try_get("burned_at").map_err(sqlx_err)?;
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
        if content.is_none()
            && message_type == MessageType::Text as i32
            && let Some(ref t) = text_col
        {
            let trimmed = t.trim();
            if !trimmed.is_empty() {
                extra
                    .entry("contentText".to_string())
                    .or_insert_with(|| trimmed.to_string());
                content = Some(Elem::Text(TextElem {
                    text: trimmed.to_string(),
                    mentions: vec![],
                }));
            }
        }

        let mut ts_u = timestamp.max(0) as u64;
        let mut cts_u = client_timestamp.max(0) as u64;
        let sort_u = sort_ts.max(0) as u64;
        // 待发/旧数据可能未写 `timestamp`，但 `sort_ts` 已在落库时规范化（见 `effective_sort_ts_for_persist`），
        // 读出时回填给前端时间排序，避免 0 被当成「最早」。
        if ts_u == 0 && cts_u == 0 && sort_u > 0 {
            ts_u = sort_u;
            cts_u = sort_u;
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
            timestamp: ts_u,
            client_timestamp: cts_u,
            message_type,
            content,
            content_bytes,
            sender_name,
            sender_avatar,
            sender_display_name,
            reply_to,
            quote_preview,
            status,
            burn_enabled: burn_enabled != 0,
            burn_after_read_seconds,
            burn_status,
            first_read_at,
            burn_at,
            burned_at,
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
        // 与 `before_seq_for_sqlite` 一致：`0` / `>= i64::MAX` 表示「最新一页」游标。
        let is_latest_window = before_seq == 0 || before_seq >= i64::MAX as u64;
        let bound = before_seq_for_sqlite(before_seq);
        let cleared_floor = self.local_cleared_floor(conversation_id).await?;

        let rows = if is_latest_window {
            // 待发/ACK 后仅 `sort_ts` 可能小于历史行的服务端时间，单按 sort_ts 会把**最新一条**挤出 LIMIT。
            // 用列上 max 与 `effective_sort_ts_for_persist` 语义一致。
            let sql = if cleared_floor > 0 {
                format!(
                    r#"SELECT {} FROM messages
                   WHERE conversation_id = ? AND seq < ? AND (seq = 0 OR seq > ?)
                   ORDER BY max(max(sort_ts, timestamp), client_timestamp) DESC, seq DESC LIMIT ?"#,
                    MESSAGE_SELECT_COLS
                )
            } else {
                format!(
                    r#"SELECT {} FROM messages
                   WHERE conversation_id = ? AND seq < ?
                   ORDER BY max(max(sort_ts, timestamp), client_timestamp) DESC, seq DESC LIMIT ?"#,
                    MESSAGE_SELECT_COLS
                )
            };
            let mut q = sqlx::query(&sql).bind(conversation_id).bind(bound);
            if cleared_floor > 0 {
                q = q.bind(cleared_floor as i64);
            }
            q.bind(limit as i32).fetch_all(&self.pool).await
        } else {
            // 翻页只拉已分配 seq 的历史消息，避免 `seq == 0` 的待发送行在第二页重复出现。
            let sql = if cleared_floor > 0 {
                format!(
                    r#"SELECT {} FROM messages
                   WHERE conversation_id = ? AND seq > 0 AND seq < ? AND seq > ?
                   ORDER BY seq DESC LIMIT ?"#,
                    MESSAGE_SELECT_COLS
                )
            } else {
                format!(
                    r#"SELECT {} FROM messages
                   WHERE conversation_id = ? AND seq > 0 AND seq < ?
                   ORDER BY seq DESC LIMIT ?"#,
                    MESSAGE_SELECT_COLS
                )
            };
            let mut q = sqlx::query(&sql).bind(conversation_id).bind(bound);
            if cleared_floor > 0 {
                q = q.bind(cleared_floor as i64);
            }
            q.bind(limit as i32).fetch_all(&self.pool).await
        }
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(self.row_to_immessage(&row)?);
        }
        Ok(out)
    }

    async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<IMMessage>> {
        self.search_by_query(&MessageSearchQuery::text(keyword, limit))
            .await
    }

    async fn search_by_query(&self, query: &MessageSearchQuery) -> Result<Vec<IMMessage>> {
        let effective_time = search_effective_time_sql("messages");
        let mut sql = format!("SELECT {} FROM messages WHERE 1 = 1", MESSAGE_SELECT_COLS);
        sql.push_str(" AND (seq = 0 OR seq > COALESCE((SELECT visible_after_seq FROM conversations WHERE conversation_id = messages.conversation_id LIMIT 1), 0))");
        if !query.include_recalled {
            sql.push_str(" AND COALESCE(is_recalled, 0) = 0");
        }

        let mut qb = QueryBuilder::<Sqlite>::new(sql);
        if let Some(conversation_id) = query
            .conversation_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            qb.push(" AND conversation_id = ");
            qb.push_bind(conversation_id);
        }
        if let Some(sender_id) = query
            .sender_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            qb.push(" AND sender_id = ");
            qb.push_bind(sender_id);
        }
        if let Some(keyword) = query.normalized_keyword() {
            let like = escaped_like_contains(&keyword);
            qb.push(" AND (LOWER(COALESCE(text, '')) LIKE ");
            qb.push_bind(like.clone());
            qb.push(" ESCAPE '\\' OR LOWER(COALESCE(extra, '')) LIKE ");
            qb.push_bind(like);
            qb.push(" ESCAPE '\\')");
        }
        if let Some(from_time) = query.from_time {
            qb.push(" AND ");
            qb.push(&effective_time);
            qb.push(" >= ");
            qb.push_bind(from_time.min(i64::MAX as u64) as i64);
        }
        if let Some(to_time) = query.to_time {
            qb.push(" AND ");
            qb.push(&effective_time);
            qb.push(" <= ");
            qb.push_bind(to_time.min(i64::MAX as u64) as i64);
        }

        let message_types = message_type_values_for_search(&query.kinds);
        if !message_types.is_empty() {
            qb.push(" AND message_type IN (");
            let mut separated = qb.separated(", ");
            for value in message_types {
                separated.push_bind(value);
            }
            separated.push_unseparated(")");
        }

        qb.push(" ORDER BY ");
        qb.push(&effective_time);
        qb.push(" DESC, seq DESC LIMIT ");
        qb.push_bind(query.normalized_limit() as i32);

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(self.row_to_immessage(&row)?);
        }
        Ok(out)
    }

    async fn search_in_conversation(
        &self,
        conversation_id: &str,
        keyword: &str,
        limit: u32,
    ) -> Result<Vec<IMMessage>> {
        let keyword = keyword.trim();
        if keyword.is_empty() {
            return Ok(Vec::new());
        }
        self.search_by_query(&MessageSearchQuery::in_conversation(
            conversation_id,
            keyword,
            limit,
        ))
        .await
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

async fn refresh_reactions_json_snapshot(pool: &SqlitePool, message_id: &str) -> Result<()> {
    let id = message_id.trim();
    if id.is_empty() {
        return Ok(());
    }
    let message_row = sqlx::query(
        r#"SELECT server_id, extra FROM messages
           WHERE server_id = ? OR client_msg_id = ?
           LIMIT 1"#,
    )
    .bind(id)
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?;
    let Some(row) = message_row else {
        return Ok(());
    };
    let server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
    if server_id.trim().is_empty() {
        return Ok(());
    }

    let reaction_rows = sqlx::query(
        r#"SELECT emoji, user_id
           FROM message_reactions
           WHERE message_server_id = ?
           ORDER BY updated_at ASC"#,
    )
    .bind(server_id.trim())
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    let mut grouped: HashMap<String, Vec<String>> = HashMap::new();
    for rr in reaction_rows {
        let emoji: String = rr.try_get("emoji").map_err(sqlx_err)?;
        let user_id: String = rr.try_get("user_id").map_err(sqlx_err)?;
        if emoji.trim().is_empty() || user_id.trim().is_empty() {
            continue;
        }
        grouped
            .entry(emoji.trim().to_string())
            .or_default()
            .push(user_id.trim().to_string());
    }
    let mut reactions: Vec<ReactionEntry> = grouped
        .into_iter()
        .map(|(emoji, user_ids)| ReactionEntry {
            emoji,
            count: user_ids.len() as u32,
            user_ids,
        })
        .collect();
    reactions.sort_by(|a, b| a.emoji.cmp(&b.emoji));
    debug!(
        message_id = %id,
        server_id = %server_id,
        reaction_group_count = reactions.len(),
        "refresh_reactions_json_snapshot"
    );

    let extra_raw: Option<String> = row.try_get("extra").map_err(sqlx_err)?;
    let mut extra = parse_extra(extra_raw.as_deref());
    if reactions.is_empty() {
        extra.remove("reactionsJson");
    } else if let Ok(raw) = serde_json::to_string(&reactions) {
        extra.insert("reactionsJson".to_string(), raw);
    }
    sqlx::query("UPDATE messages SET extra = ? WHERE server_id = ?")
        .bind(extra_to_json(&extra))
        .bind(server_id)
        .execute(pool)
        .await
        .map_err(sqlx_err)?;
    Ok(())
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
    let last_message_at = conversation_projection_ts(message);
    let preview = message_preview_for_storage(message).unwrap_or_default();
    let max_seq = message.seq as i64;
    let now = now_ms_i64();
    let created_at = if last_message_at > 0 {
        last_message_at
    } else {
        now
    };
    let updated_at = if last_message_at > 0 {
        last_message_at
    } else {
        now
    };

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
                   WHEN
                       (COALESCE(excluded.max_seq, 0) > COALESCE(conversations.max_seq, 0))
                    OR (COALESCE(excluded.max_seq, 0) > 0
                        AND COALESCE(excluded.max_seq, 0) = COALESCE(conversations.max_seq, 0)
                        AND COALESCE(excluded.last_message_at, 0) >= COALESCE(conversations.last_message_at, 0))
                    OR (COALESCE(excluded.max_seq, 0) = 0
                        AND COALESCE(excluded.last_message_at, 0) >= COALESCE(conversations.last_message_at, 0))
                   THEN excluded.last_message_id
                   ELSE conversations.last_message_id
               END,
               last_sender_id = CASE
                   WHEN
                       (COALESCE(excluded.max_seq, 0) > COALESCE(conversations.max_seq, 0))
                    OR (COALESCE(excluded.max_seq, 0) > 0
                        AND COALESCE(excluded.max_seq, 0) = COALESCE(conversations.max_seq, 0)
                        AND COALESCE(excluded.last_message_at, 0) >= COALESCE(conversations.last_message_at, 0))
                    OR (COALESCE(excluded.max_seq, 0) = 0
                        AND COALESCE(excluded.last_message_at, 0) >= COALESCE(conversations.last_message_at, 0))
                   THEN excluded.last_sender_id
                   ELSE conversations.last_sender_id
               END,
               last_message_at = CASE
                   WHEN
                       (COALESCE(excluded.max_seq, 0) > COALESCE(conversations.max_seq, 0))
                    OR (COALESCE(excluded.max_seq, 0) > 0
                        AND COALESCE(excluded.max_seq, 0) = COALESCE(conversations.max_seq, 0)
                        AND COALESCE(excluded.last_message_at, 0) >= COALESCE(conversations.last_message_at, 0))
                    OR (COALESCE(excluded.max_seq, 0) = 0
                        AND COALESCE(excluded.last_message_at, 0) >= COALESCE(conversations.last_message_at, 0))
                   THEN excluded.last_message_at
                   ELSE conversations.last_message_at
               END,
               last_message_preview = CASE
                   WHEN
                       (COALESCE(excluded.max_seq, 0) > COALESCE(conversations.max_seq, 0))
                    OR (COALESCE(excluded.max_seq, 0) > 0
                        AND COALESCE(excluded.max_seq, 0) = COALESCE(conversations.max_seq, 0)
                        AND COALESCE(excluded.last_message_at, 0) >= COALESCE(conversations.last_message_at, 0))
                    OR (COALESCE(excluded.max_seq, 0) = 0
                        AND COALESCE(excluded.last_message_at, 0) >= COALESCE(conversations.last_message_at, 0))
                   THEN excluded.last_message_preview
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
    let has_snapshot =
        has_reaction_snapshot_in_extra(&message.extra) || !message.reactions.is_empty();
    if !has_snapshot {
        // 下行消息通常不携带 reactions 快照，不能把已落库的反应误删。
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

async fn refresh_conversation_snapshot_after_message_delete_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    conversation_id: &str,
) -> Result<()> {
    if conversation_id.trim().is_empty() {
        return Ok(());
    }

    let row = sqlx::query(
        r#"SELECT m.server_id, m.client_msg_id, m.sender_id,
                  CASE
                    WHEN m.seq > 0 THEN COALESCE(NULLIF(m.timestamp, 0), NULLIF(m.client_timestamp, 0), NULLIF(m.sort_ts, 0), 0)
                    ELSE max(max(COALESCE(m.sort_ts, 0), COALESCE(m.timestamp, 0)), COALESCE(m.client_timestamp, 0))
                  END AS message_at,
                  text
           FROM messages m
           LEFT JOIN conversations c ON c.conversation_id = m.conversation_id
           WHERE m.conversation_id = ?
             AND TRIM(COALESCE(m.text, '')) != ''
             AND (
                 m.seq = 0
                 OR m.seq > COALESCE(c.visible_after_seq, 0)
             )
           ORDER BY
             CASE
               WHEN m.seq = 0
                AND max(max(COALESCE(m.sort_ts, 0), COALESCE(m.timestamp, 0)), COALESCE(m.client_timestamp, 0)) >
                    COALESCE((
                      SELECT max(COALESCE(NULLIF(s.timestamp, 0), NULLIF(s.client_timestamp, 0), NULLIF(s.sort_ts, 0), 0))
                      FROM messages s
                      WHERE s.conversation_id = m.conversation_id
                        AND s.seq > COALESCE(c.visible_after_seq, 0)
                        AND s.seq > 0
                        AND TRIM(COALESCE(s.text, '')) != ''
                    ), 0)
               THEN 1 ELSE 0
             END DESC,
             CASE WHEN m.seq > 0 THEN m.seq ELSE 0 END DESC,
             CASE
               WHEN m.seq > 0 THEN COALESCE(NULLIF(m.timestamp, 0), NULLIF(m.client_timestamp, 0), NULLIF(m.sort_ts, 0), 0)
               ELSE max(max(COALESCE(m.sort_ts, 0), COALESCE(m.timestamp, 0)), COALESCE(m.client_timestamp, 0))
             END DESC
           LIMIT 1"#,
    )
    .bind(conversation_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(sqlx_err)?;

    if let Some(row) = row {
        let server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
        let client_msg_id: String = row.try_get("client_msg_id").map_err(sqlx_err)?;
        let sender_id: String = row.try_get("sender_id").map_err(sqlx_err)?;
        let message_at: i64 = row.try_get("message_at").map_err(sqlx_err)?;
        let text: Option<String> = row.try_get("text").map_err(sqlx_err)?;
        let message_id = if server_id.trim().is_empty() {
            client_msg_id
        } else {
            server_id
        };
        sqlx::query(
            r#"UPDATE conversations
               SET last_message_id = ?,
                   last_sender_id = ?,
                   last_message_at = ?,
                   last_message_preview = ?,
                   updated_at = MAX(COALESCE(updated_at, 0), ?),
                   updated_at_ts = MAX(COALESCE(updated_at_ts, 0), ?)
               WHERE conversation_id = ?"#,
        )
        .bind(message_id)
        .bind(sender_id)
        .bind(message_at)
        .bind(text.as_deref())
        .bind(message_at)
        .bind(message_at)
        .bind(conversation_id)
        .execute(&mut **tx)
        .await
        .map_err(sqlx_err)?;
    } else {
        sqlx::query(
            r#"UPDATE conversations
               SET last_message_id = NULL,
                   last_sender_id = NULL,
                   last_message_at = NULL,
                   last_message_preview = NULL
               WHERE conversation_id = ?"#,
        )
        .bind(conversation_id)
        .execute(&mut **tx)
        .await
        .map_err(sqlx_err)?;
    }

    Ok(())
}

#[async_trait]
impl MessageWriter for SqliteMessageRepo {
    async fn save_batch(&self, messages: &[IMMessage]) -> Result<()> {
        let mut cleared_floors: HashMap<String, u64> = HashMap::new();
        let mut persistable: Vec<&IMMessage> = Vec::new();
        for m in messages {
            let cid = m.conversation_id.trim();
            if cid.is_empty() {
                continue;
            }
            let floor = if let Some(v) = cleared_floors.get(cid) {
                *v
            } else {
                let v = self.local_cleared_floor(cid).await?;
                cleared_floors.insert(cid.to_string(), v);
                v
            };
            if message_visible_after_clear(m, floor) {
                persistable.push(m);
            }
        }
        if persistable.is_empty() {
            return Ok(());
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let mut latest_per_conversation: HashMap<&str, &IMMessage> = HashMap::new();
        for m in persistable {
            let extra_json = serde_json::to_string(&m.extra).unwrap_or_default();
            let mention_users_json = serde_json::to_string(&m.mention_users).unwrap_or_default();
            let extensions_json = extensions_to_json(&m.extensions);
            let text = message_preview_for_storage(m);
            sqlx::query(
                r#"INSERT OR REPLACE INTO messages (
                   server_id, conversation_id, client_msg_id, sender_id, source, seq, timestamp, client_timestamp,
                   conversation_type, message_type, channel_id, sender_name, sender_avatar,
                   sender_display_name, content, status,
                   burn_enabled, burn_after_read_seconds, burn_status, first_read_at, burn_at, burned_at,
                   is_read, is_recalled, is_edited,
                   reply_to, quote_preview, mention_users, mention_all, extra, extensions, version, updated_at, text,
                   sending, failed, is_local, sort_ts)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
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
            .bind(if m.burn_enabled { 1i32 } else { 0 })
            .bind(m.burn_after_read_seconds)
            .bind(m.burn_status)
            .bind(m.first_read_at)
            .bind(m.burn_at)
            .bind(m.burned_at)
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
            .bind(effective_sort_ts_for_persist(m))
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
            replace_reaction_snapshot_tx(&mut tx, m).await?;

            let conv_id = m.conversation_id.trim();
            if conv_id.is_empty() {
                continue;
            }
            match latest_per_conversation.get(conv_id) {
                Some(prev) if !should_replace_conversation_projection(prev, m) => {}
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
        MessageWriter::save_batch(self, std::slice::from_ref(message)).await
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
            sqlx::query("UPDATE messages SET status = ? WHERE server_id = ? OR client_msg_id = ?")
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
        let deleted_conversation_id = sqlx::query(
            r#"SELECT conversation_id
               FROM messages
               WHERE server_id = ? OR client_msg_id = ?
               LIMIT 1"#,
        )
        .bind(message_id)
        .bind(message_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?
        .and_then(|row| row.try_get::<String, _>("conversation_id").ok());
        sqlx::query(
            r#"DELETE FROM message_reactions
               WHERE message_server_id = ?
                  OR message_server_id IN (
                      SELECT server_id
                      FROM messages
                      WHERE server_id = ? OR client_msg_id = ?
                  )"#,
        )
        .bind(message_id)
        .bind(message_id)
        .bind(message_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        sqlx::query("DELETE FROM messages WHERE server_id = ? OR client_msg_id = ?")
            .bind(message_id)
            .bind(message_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        if let Some(conversation_id) = deleted_conversation_id {
            refresh_conversation_snapshot_after_message_delete_tx(&mut tx, &conversation_id)
                .await?;
        }
        tx.commit()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn update_after_ack(&self, client_msg_id: &str, message: &IMMessage) -> Result<()> {
        let message = MessageDeliveryService::sanitize_send_ack_message(message);
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
        let text = message_preview_for_storage(&message);
        sqlx::query(
            r#"INSERT OR REPLACE INTO messages (
               server_id, conversation_id, client_msg_id, sender_id, source, seq, timestamp, client_timestamp,
               conversation_type, message_type, channel_id, sender_name, sender_avatar,
               sender_display_name, content, status,
               burn_enabled, burn_after_read_seconds, burn_status, first_read_at, burn_at, burned_at,
               is_read, is_recalled, is_edited,
               reply_to, quote_preview, mention_users, mention_all, extra, extensions, version, updated_at, text,
               sending, failed, is_local, sort_ts)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
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
        .bind(if message.burn_enabled { 1i32 } else { 0 })
        .bind(message.burn_after_read_seconds)
        .bind(message.burn_status)
        .bind(message.first_read_at)
        .bind(message.burn_at)
        .bind(message.burned_at)
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
        .bind(effective_sort_ts_for_persist(&message))
        .execute(&mut *tx)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        replace_reaction_snapshot_tx(&mut tx, &message).await?;
        upsert_conversation_snapshot_tx(&mut tx, &message).await?;
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
            .get("currentEditVersion")
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
        extra.insert(
            "currentEditVersion".to_string(),
            next_edit_version.to_string(),
        );
        extra.insert("messageFsmState".to_string(), "EDITED".to_string());
        extra.insert("lastEditedAt".to_string(), now_ms.to_string());

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

    async fn reconcile_outgoing_read_by_peer_seq(
        &self,
        conversation_id: &str,
        sender_user_id: &str,
        peer_read_seq: u64,
    ) -> Result<()> {
        if conversation_id.trim().is_empty() || sender_user_id.trim().is_empty() {
            return Ok(());
        }
        if peer_read_seq > 0 {
            self.mark_outgoing_read_upto_seq(conversation_id, sender_user_id, peer_read_seq)
                .await?;
        }
        let sent = MessageStatus::Sent as i32;
        let read = MessageStatus::Read as i32;
        sqlx::query(
            r#"UPDATE messages
               SET status = ?, is_read = 0
               WHERE conversation_id = ?
                 AND sender_id = ?
                 AND seq > ?
                 AND status = ?"#,
        )
        .bind(sent)
        .bind(conversation_id)
        .bind(sender_user_id)
        .bind(peer_read_seq as i64)
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
            debug!(
                conversation_id = %conversation_id,
                message_server_id = %message_server_id,
                user_id = %user_id,
                emoji = %emoji,
                action = action,
                "apply_reaction remove"
            );
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
        debug!(
            conversation_id = %conversation_id,
            message_server_id = %message_server_id,
            user_id = %user_id,
            emoji = %emoji,
            action = action,
            "apply_reaction add"
        );
        refresh_reactions_json_snapshot(&self.pool, message_server_id).await?;
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

        let seq_key = format!("lastReactionEventSeq:{user_id}:{emoji}");
        let applied = apply_message_extra_with_seq(
            &self.pool,
            message_server_id,
            &seq_key,
            event_seq,
            |_| {},
        )
        .await?;
        if applied == OperationApplyResult::IgnoredStale {
            return Ok(OperationApplyResult::IgnoredStale);
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
            debug!(
                conversation_id = %conversation_id,
                message_server_id = %message_server_id,
                user_id = %user_id,
                emoji = %emoji,
                action = action,
                "apply_reaction_event remove"
            );
            refresh_reactions_json_snapshot(&self.pool, message_server_id).await?;
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
        debug!(
            conversation_id = %conversation_id,
            message_server_id = %message_server_id,
            user_id = %user_id,
            emoji = %emoji,
            action = action,
            "apply_reaction_event add"
        );
        refresh_reactions_json_snapshot(&self.pool, message_server_id).await?;
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
        if is_stale_operation(&extra, "lastDeleteEventSeq", event_seq) {
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
            "lastPinEventSeq",
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
        let seq_key = format!("lastMarkEventSeq:{mark_type}");
        apply_message_extra_with_seq(&self.pool, message_id, &seq_key, event_seq, |extra| {
            if set_mark {
                extra.insert("markType".to_string(), mark_type.to_string());
                if let Some(c) = color {
                    if !c.trim().is_empty() {
                        extra.insert("markColor".to_string(), c.trim().to_string());
                    } else {
                        extra.remove("markColor");
                    }
                } else {
                    extra.remove("markColor");
                }
            } else {
                extra.remove("markType");
                extra.remove("markColor");
            }
        })
        .await
    }

    async fn apply_burn_scheduled_event(
        &self,
        message_id: &str,
        burn_at: i64,
        first_read_at: i64,
        event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        let applied = apply_message_extra_with_seq(
            &self.pool,
            message_id,
            "lastBurnScheduledEventSeq",
            event_seq,
            |extra| {
                extra.insert("burn_event".to_string(), "scheduled".to_string());
            },
        )
        .await?;
        if !matches!(applied, OperationApplyResult::Applied) {
            return Ok(applied);
        }
        let rows = sqlx::query(
            r#"UPDATE messages
               SET burn_enabled = 1,
                   burn_status = ?,
                   first_read_at = COALESCE(first_read_at, ?),
                   burn_at = COALESCE(burn_at, ?)
               WHERE (server_id = ? OR client_msg_id = ?)
                 AND burn_status < ?"#,
        )
        .bind(BurnStatus::BurnPending as i32)
        .bind(first_read_at)
        .bind(burn_at)
        .bind(message_id)
        .bind(message_id)
        .bind(BurnStatus::Burned as i32)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();
        if rows == 0 {
            return Ok(OperationApplyResult::IgnoredStale);
        }
        Ok(OperationApplyResult::Applied)
    }

    async fn apply_burned_event(
        &self,
        message_id: &str,
        burn_at: i64,
        burned_at: i64,
        event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        let applied = apply_message_extra_with_seq(
            &self.pool,
            message_id,
            "lastBurnedEventSeq",
            event_seq,
            |extra| {
                extra.insert("burn_event".to_string(), "burned".to_string());
                extra.insert("burn_placeholder".to_string(), "该消息已销毁".to_string());
            },
        )
        .await?;
        if !matches!(applied, OperationApplyResult::Applied) {
            return Ok(applied);
        }
        let now_ms = now_ms_i64();
        let rows = sqlx::query(
            r#"UPDATE messages
               SET burn_enabled = 1,
                   burn_status = ?,
                   burn_at = COALESCE(burn_at, ?),
                   burned_at = COALESCE(burned_at, ?),
                   content = ?,
                   text = NULL,
                   updated_at = ?
               WHERE (server_id = ? OR client_msg_id = ?)
                 AND burn_status < ?"#,
        )
        .bind(BurnStatus::Burned as i32)
        .bind(burn_at)
        .bind(burned_at)
        .bind(Vec::<u8>::new())
        .bind(now_ms)
        .bind(message_id)
        .bind(message_id)
        .bind(BurnStatus::HardDeleted as i32)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();
        if rows == 0 {
            return Ok(OperationApplyResult::IgnoredStale);
        }
        Ok(OperationApplyResult::Applied)
    }

    async fn apply_hard_deleted_event(
        &self,
        message_id: &str,
        burn_at: Option<i64>,
        burned_at: Option<i64>,
        hard_deleted_at: i64,
        event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        let applied = apply_message_extra_with_seq(
            &self.pool,
            message_id,
            "lastHardDeleteEventSeq",
            event_seq,
            |extra| {
                extra.insert("burn_event".to_string(), "hard_deleted".to_string());
                extra.insert("burn_placeholder".to_string(), "该消息已销毁".to_string());
            },
        )
        .await?;
        if !matches!(applied, OperationApplyResult::Applied) {
            return Ok(applied);
        }
        let now_ms = now_ms_i64();
        let rows = sqlx::query(
            r#"UPDATE messages
               SET burn_enabled = 1,
                   burn_status = ?,
                   burn_at = COALESCE(burn_at, ?),
                   burned_at = COALESCE(burned_at, ?),
                   content = ?,
                   text = NULL,
                   updated_at = ?
               WHERE (server_id = ? OR client_msg_id = ?)"#,
        )
        .bind(BurnStatus::HardDeleted as i32)
        .bind(burn_at)
        .bind(burned_at.or(Some(hard_deleted_at)))
        .bind(Vec::<u8>::new())
        .bind(now_ms)
        .bind(message_id)
        .bind(message_id)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();
        if rows == 0 {
            return Ok(OperationApplyResult::NotFound);
        }
        Ok(OperationApplyResult::Applied)
    }

    async fn list_reactions(
        &self,
        message_server_ids: &[String],
    ) -> Result<HashMap<String, Vec<ReactionEntry>>> {
        if message_server_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut id_keys: Vec<String> = message_server_ids
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if id_keys.is_empty() {
            return Ok(HashMap::new());
        }
        id_keys.sort();
        id_keys.dedup();

        let mut alias_qb = QueryBuilder::<Sqlite>::new(
            "SELECT server_id, client_msg_id FROM messages WHERE server_id IN (",
        );
        let mut alias_sep_a = alias_qb.separated(", ");
        for id in &id_keys {
            alias_sep_a.push_bind(id);
        }
        alias_qb.push(") OR client_msg_id IN (");
        let mut alias_sep_b = alias_qb.separated(", ");
        for id in &id_keys {
            alias_sep_b.push_bind(id);
        }
        alias_qb.push(")");
        let alias_rows = alias_qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        let mut canonical: HashMap<String, String> = HashMap::new();
        for row in alias_rows {
            let sid: String = row.try_get("server_id").map_err(sqlx_err)?;
            let cid: String = row.try_get("client_msg_id").map_err(sqlx_err)?;
            let sid_t = sid.trim();
            let cid_t = cid.trim();
            if !sid_t.is_empty() {
                canonical.insert(sid_t.to_string(), sid_t.to_string());
            }
            if !cid_t.is_empty() {
                canonical.insert(cid_t.to_string(), sid_t.to_string());
            }
        }

        let mut qb = QueryBuilder::<Sqlite>::new(
            "SELECT message_server_id, emoji, user_id FROM message_reactions WHERE message_server_id IN (",
        );
        let mut separated = qb.separated(", ");
        for id in &id_keys {
            separated.push_bind(id);
        }
        qb.push(
            ") OR message_server_id IN (SELECT client_msg_id FROM messages WHERE server_id IN (",
        );
        let mut sid_sep = qb.separated(", ");
        for id in &id_keys {
            sid_sep.push_bind(id);
        }
        qb.push(")) ORDER BY updated_at ASC");
        let rows = qb.build().fetch_all(&self.pool).await.map_err(sqlx_err)?;

        let mut grouped: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();
        for row in rows {
            let msg_id: String = row.try_get("message_server_id").map_err(sqlx_err)?;
            let emoji: String = row.try_get("emoji").map_err(sqlx_err)?;
            let user_id: String = row.try_get("user_id").map_err(sqlx_err)?;
            let resolved_id = canonical
                .get(msg_id.trim())
                .cloned()
                .unwrap_or_else(|| msg_id.trim().to_string());
            if resolved_id.is_empty() {
                continue;
            }
            grouped
                .entry(resolved_id)
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

    async fn set_message_flag(
        &self,
        message_id: &str,
        flag_key: &str,
        enabled: bool,
    ) -> Result<()> {
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
            extra.insert("markType".to_string(), mark_type.to_string());
            if let Some(c) = color {
                if !c.trim().is_empty() {
                    extra.insert("markColor".to_string(), c.trim().to_string());
                } else {
                    extra.remove("markColor");
                }
            } else {
                extra.remove("markColor");
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
            extra.remove("markType");
            extra.remove("markColor");
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
        let mut update_qb =
            QueryBuilder::<Sqlite>::new("UPDATE messages SET sending = 0, failed = 1, status = ");
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
        let mut update_qb =
            QueryBuilder::<Sqlite>::new("UPDATE messages SET sending = 0, failed = 1, status = ");
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
        ConversationReader, EditApplyResult, MessageReader, MessageStore, MessageWriter,
        OperationApplyResult,
    };
    use crate::infrastructure::persistence::sqlite::conversation_repo::SqliteConversationRepo;
    use crate::infrastructure::persistence::sqlite_init_schema;
    use crate::model::message::{MessageStatus, ReactionAction};
    use crate::model::message_elem::{Elem, TextElem};
    use crate::model::{IMMessage, MessageSearchKind, MessageSearchQuery, MessageType};
    use flare_proto::common::BurnStatus;
    use sqlx::SqlitePool;

    async fn make_repo() -> SqliteMessageRepo {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite_init_schema(&pool).await.unwrap();
        SqliteMessageRepo::new(pool)
    }

    fn text_message(
        server_id: &str,
        conversation_id: &str,
        sender_id: &str,
        seq: u64,
        timestamp: u64,
        text: &str,
    ) -> IMMessage {
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = server_id.to_string();
        message.client_msg_id = format!("client-{server_id}");
        message.conversation_id = conversation_id.to_string();
        message.sender_id = sender_id.to_string();
        message.seq = seq;
        message.timestamp = timestamp;
        message.client_timestamp = timestamp;
        message.content = Some(Elem::Text(TextElem {
            text: text.to_string(),
            mentions: Vec::new(),
        }));
        message
    }

    #[tokio::test]
    async fn conversation_projection_uses_seq_over_timestamp_for_server_messages() {
        let repo = make_repo().await;
        let older_seq_future_time =
            text_message("server-10", "conv-order", "u2", 10, 2_000, "older seq");
        let newer_seq_past_time =
            text_message("server-11", "conv-order", "u2", 11, 1_000, "newer seq");

        repo.save_batch(&[older_seq_future_time]).await.unwrap();
        repo.save_batch(&[newer_seq_past_time]).await.unwrap();

        let conversations = SqliteConversationRepo::new(repo.pool.clone());
        let conversation = conversations
            .get("conv-order")
            .await
            .unwrap()
            .expect("conversation snapshot");

        assert_eq!(conversation.max_seq, 11);
        assert_eq!(conversation.last_message_id.as_deref(), Some("server-11"));
        assert_eq!(conversation.last_sender_id.as_deref(), Some("u2"));
    }

    #[tokio::test]
    async fn deleting_last_message_rebuilds_conversation_preview_from_previous_visible_message() {
        let repo = make_repo().await;
        let first = text_message("server-del-1", "conv-delete", "u2", 1, 1_000, "first");
        let second = text_message("server-del-2", "conv-delete", "u2", 2, 2_000, "second");
        repo.save_batch(&[first, second]).await.unwrap();

        repo.delete("server-del-2").await.unwrap();

        let conversations = SqliteConversationRepo::new(repo.pool.clone());
        let conversation = conversations
            .get("conv-delete")
            .await
            .unwrap()
            .expect("conversation snapshot");

        assert_eq!(conversation.max_seq, 2);
        assert_eq!(
            conversation.last_message_id.as_deref(),
            Some("server-del-1")
        );
        assert_eq!(conversation.last_message_at, Some(1_000));
    }

    #[tokio::test]
    async fn pending_local_message_updates_conversation_preview_with_client_time() {
        let repo = make_repo().await;
        let server = text_message(
            "server-pending-base",
            "conv-pending",
            "u2",
            7,
            1_000,
            "server",
        );
        let mut pending = text_message("", "conv-pending", "u1", 0, 0, "pending");
        pending.client_msg_id = "client-pending-new".to_string();
        pending.client_timestamp = 5_000;
        pending.local_state.sending = true;
        pending.local_state.is_local = true;

        repo.save_batch(&[server]).await.unwrap();
        repo.save_batch(&[pending]).await.unwrap();

        let conversations = SqliteConversationRepo::new(repo.pool.clone());
        let conversation = conversations
            .get("conv-pending")
            .await
            .unwrap()
            .expect("conversation snapshot");

        assert_eq!(
            conversation.last_message_id.as_deref(),
            Some("client-pending-new")
        );
        assert!(conversation.last_message_at.unwrap_or_default() >= 5_000);
        assert!(
            conversation
                .last_message_preview
                .as_deref()
                .unwrap_or_default()
                .contains("pending")
        );
    }

    #[tokio::test]
    async fn advanced_search_filters_conversation_sender_time_and_media_kind() {
        let repo = make_repo().await;
        let text = text_message("server-search-text", "conv-search", "u2", 1, 1_000, "alpha");
        let mut image = text_message(
            "server-search-image",
            "conv-search",
            "u3",
            2,
            2_000,
            "alpha image",
        );
        image.message_type = MessageType::Image as i32;
        let mut other_sender = text_message(
            "server-search-other",
            "conv-search",
            "u4",
            3,
            3_000,
            "alpha other",
        );
        other_sender.message_type = MessageType::Image as i32;
        let other_conversation = text_message(
            "server-search-foreign",
            "conv-foreign",
            "u3",
            4,
            4_000,
            "alpha image",
        );
        repo.save_batch(&[text, image, other_sender, other_conversation])
            .await
            .unwrap();

        let results = repo
            .search_by_query(&MessageSearchQuery {
                keyword: Some("alpha".to_string()),
                conversation_id: Some("conv-search".to_string()),
                sender_id: Some("u3".to_string()),
                from_time: Some(1_500),
                to_time: Some(2_500),
                kinds: vec![MessageSearchKind::Media],
                limit: 20,
                include_recalled: false,
            })
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].server_id, "server-search-image");

        let wildcard_results = repo
            .search_by_query(&MessageSearchQuery {
                keyword: Some("%".to_string()),
                limit: 20,
                ..MessageSearchQuery::default()
            })
            .await
            .unwrap();
        assert!(wildcard_results.is_empty());
    }

    #[tokio::test]
    async fn deleting_client_only_pending_message_rebuilds_preview() {
        let repo = make_repo().await;
        let server = text_message(
            "server-client-delete",
            "conv-client-delete",
            "u2",
            3,
            1_000,
            "server",
        );
        let mut pending = text_message("", "conv-client-delete", "u1", 0, 2_000, "pending");
        pending.client_msg_id = "client-only-delete".to_string();
        pending.local_state.sending = true;
        pending.local_state.is_local = true;

        repo.save_batch(&[server, pending]).await.unwrap();
        repo.delete("client-only-delete").await.unwrap();

        assert!(repo.get("client-only-delete").await.unwrap().is_none());

        let conversations = SqliteConversationRepo::new(repo.pool.clone());
        let conversation = conversations
            .get("conv-client-delete")
            .await
            .unwrap()
            .expect("conversation snapshot");

        assert_eq!(
            conversation.last_message_id.as_deref(),
            Some("server-client-delete")
        );
        assert!(
            conversation
                .last_message_preview
                .as_deref()
                .unwrap_or_default()
                .contains("server")
        );
    }

    #[tokio::test]
    async fn update_after_ack_clamps_self_echo_to_sent() {
        let repo = make_repo().await;
        let mut pending = IMMessage::new(flare_proto::common::Message::default());
        pending.server_id = "client-ack-1".to_string();
        pending.client_msg_id = "client-ack-1".to_string();
        pending.conversation_id = "conv-ack".to_string();
        pending.sender_id = "u1".to_string();
        pending.status = MessageStatus::Created as i32;
        pending.local_state.sending = true;
        pending.local_state.is_local = true;
        repo.save_batch(&[pending.clone()]).await.unwrap();

        let mut echoed = pending;
        echoed.server_id = "server-ack-1".to_string();
        echoed.seq = 11;
        echoed.status = MessageStatus::Read as i32;
        echoed.is_read = true;

        repo.update_after_ack("client-ack-1", &echoed)
            .await
            .unwrap();

        let stored = repo.get("server-ack-1").await.unwrap().unwrap();
        assert_eq!(stored.status, MessageStatus::Sent as i32);
        assert!(!stored.is_read);
        assert!(!stored.local_state.sending);
        assert!(!stored.local_state.is_local);
        assert!(repo.get("client-ack-1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn reconcile_outgoing_read_by_peer_seq_downgrades_polluted_tail() {
        let repo = make_repo().await;
        let mut first = IMMessage::new(flare_proto::common::Message::default());
        first.server_id = "server-read-1".to_string();
        first.client_msg_id = "client-read-1".to_string();
        first.conversation_id = "conv-read".to_string();
        first.sender_id = "u1".to_string();
        first.seq = 1;
        first.status = MessageStatus::Read as i32;
        first.is_read = true;

        let mut polluted_tail = first.clone();
        polluted_tail.server_id = "server-read-2".to_string();
        polluted_tail.client_msg_id = "client-read-2".to_string();
        polluted_tail.seq = 2;

        let mut other_sender = first.clone();
        other_sender.server_id = "server-read-other".to_string();
        other_sender.client_msg_id = "client-read-other".to_string();
        other_sender.sender_id = "u2".to_string();
        other_sender.seq = 3;

        repo.save_batch(&[first, polluted_tail, other_sender])
            .await
            .unwrap();

        repo.reconcile_outgoing_read_by_peer_seq("conv-read", "u1", 1)
            .await
            .unwrap();

        let first = repo.get("server-read-1").await.unwrap().unwrap();
        let tail = repo.get("server-read-2").await.unwrap().unwrap();
        let other = repo.get("server-read-other").await.unwrap().unwrap();
        assert_eq!(first.status, MessageStatus::Read as i32);
        assert!(first.is_read);
        assert_eq!(tail.status, MessageStatus::Sent as i32);
        assert!(!tail.is_read);
        assert_eq!(other.status, MessageStatus::Read as i32);
        assert!(other.is_read);
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
            .insert("currentEditVersion".to_string(), "2".to_string());
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
            after_new
                .extra
                .get("currentEditVersion")
                .map(String::as_str),
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
        assert_eq!(
            after_stale.extra.get("pinned").map(String::as_str),
            Some("true")
        );

        let newer = repo
            .apply_pin_event("server-2", false, Some(11))
            .await
            .unwrap();
        assert_eq!(newer, OperationApplyResult::Applied);
        let after_new = repo.get("server-2").await.unwrap().unwrap();
        assert_eq!(
            after_new.extra.get("pinned").map(String::as_str),
            Some("false")
        );
        assert_eq!(
            after_new.extra.get("lastPinEventSeq").map(String::as_str),
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
        assert_eq!(
            after_stale.extra.get("markType").map(String::as_str),
            Some("7")
        );
        assert_eq!(
            after_stale.extra.get("markColor").map(String::as_str),
            Some("#ff0000")
        );

        let newer = repo
            .apply_mark_event("server-3", 7, None, false, Some(21))
            .await
            .unwrap();
        assert_eq!(newer, OperationApplyResult::Applied);

        let after_new = repo.get("server-3").await.unwrap().unwrap();
        assert!(!after_new.extra.contains_key("markType"));
        assert!(!after_new.extra.contains_key("markColor"));
        assert_eq!(
            after_new
                .extra
                .get("lastMarkEventSeq:7")
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
        assert!(!reactions_after_remove.contains_key("server-4"));
    }

    #[tokio::test]
    async fn save_batch_without_reaction_snapshot_keeps_existing_reactions() {
        let repo = make_repo().await;
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-5".to_string();
        message.client_msg_id = "client-5".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();
        repo.save_batch(&[message]).await.unwrap();

        repo.apply_reaction_event(
            "conv-1",
            "server-5",
            "u2",
            "👍",
            ReactionAction::Add as i32,
            Some(1),
        )
        .await
        .unwrap();
        let before = repo
            .list_reactions(&["server-5".to_string()])
            .await
            .unwrap();
        assert_eq!(
            before
                .get("server-5")
                .and_then(|entries| entries.iter().find(|entry| entry.emoji == "👍"))
                .map(|entry| entry.count),
            Some(1)
        );

        // 模拟同步下行：消息不带 reactions 快照（extra 无 reactionsJson、reactions 为空）。
        let mut sync_message = IMMessage::new(flare_proto::common::Message::default());
        sync_message.server_id = "server-5".to_string();
        sync_message.client_msg_id = "client-5".to_string();
        sync_message.conversation_id = "conv-1".to_string();
        sync_message.sender_id = "u1".to_string();
        repo.save_batch(&[sync_message]).await.unwrap();

        let after = repo
            .list_reactions(&["server-5".to_string()])
            .await
            .unwrap();
        assert_eq!(
            after
                .get("server-5")
                .and_then(|entries| entries.iter().find(|entry| entry.emoji == "👍"))
                .map(|entry| entry.count),
            Some(1)
        );
    }

    #[tokio::test]
    async fn apply_reaction_event_before_message_arrival_is_not_lost() {
        let repo = make_repo().await;

        // 先收到 reaction 事件（消息主体尚未落库）
        let applied = repo
            .apply_reaction_event(
                "conv-9",
                "server-9",
                "u9",
                "👍",
                ReactionAction::Add as i32,
                Some(9),
            )
            .await
            .unwrap();
        assert_eq!(applied, OperationApplyResult::Applied);

        let before = repo
            .list_reactions(&["server-9".to_string()])
            .await
            .unwrap();
        assert_eq!(
            before
                .get("server-9")
                .and_then(|entries| entries.iter().find(|entry| entry.emoji == "👍"))
                .map(|entry| entry.count),
            Some(1)
        );

        // 后续消息同步到本地
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-9".to_string();
        message.client_msg_id = "client-9".to_string();
        message.conversation_id = "conv-9".to_string();
        message.sender_id = "u1".to_string();
        repo.save_batch(&[message]).await.unwrap();

        // 反应仍可通过消息 ID 聚合读取，确保 UI 可展示
        let after = repo
            .list_reactions(&["server-9".to_string()])
            .await
            .unwrap();
        assert_eq!(
            after
                .get("server-9")
                .and_then(|entries| entries.iter().find(|entry| entry.emoji == "👍"))
                .map(|entry| entry.count),
            Some(1)
        );
    }

    #[tokio::test]
    async fn apply_burned_event_is_idempotent_and_sets_placeholder() {
        let repo = make_repo().await;
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-burn-1".to_string();
        message.client_msg_id = "client-burn-1".to_string();
        message.conversation_id = "conv-burn".to_string();
        message.sender_id = "u1".to_string();
        message.content_bytes = b"hello".to_vec();
        repo.save_batch(&[message]).await.unwrap();

        let scheduled = repo
            .apply_burn_scheduled_event("server-burn-1", 1_800_000_001, 1_800_000_000, Some(10))
            .await
            .unwrap();
        assert_eq!(scheduled, OperationApplyResult::Applied);

        let burned = repo
            .apply_burned_event("server-burn-1", 1_800_000_001, 1_800_000_005, Some(20))
            .await
            .unwrap();
        assert_eq!(burned, OperationApplyResult::Applied);

        let stale = repo
            .apply_burned_event("server-burn-1", 1_800_000_009, 1_800_000_099, Some(19))
            .await
            .unwrap();
        assert_eq!(stale, OperationApplyResult::IgnoredStale);

        let after = repo.get("server-burn-1").await.unwrap().unwrap();
        assert!(after.burn_enabled);
        assert_eq!(after.burn_status, BurnStatus::Burned as i32);
        assert_eq!(after.burn_at, Some(1_800_000_001));
        assert_eq!(after.burned_at, Some(1_800_000_005));
        assert!(after.content_bytes.is_empty());
        assert_eq!(
            after.extra.get("burn_placeholder").map(String::as_str),
            Some("该消息已销毁")
        );
        assert_eq!(
            after.extra.get("lastBurnedEventSeq").map(String::as_str),
            Some("20")
        );
    }

    #[tokio::test]
    async fn apply_hard_deleted_event_is_idempotent_and_keeps_first_terminal_state() {
        let repo = make_repo().await;
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-burn-2".to_string();
        message.client_msg_id = "client-burn-2".to_string();
        message.conversation_id = "conv-burn".to_string();
        message.sender_id = "u1".to_string();
        message.content_bytes = b"hello".to_vec();
        repo.save_batch(&[message]).await.unwrap();

        let applied = repo
            .apply_hard_deleted_event(
                "server-burn-2",
                Some(1_900_000_001),
                Some(1_900_000_003),
                1_900_000_004,
                Some(30),
            )
            .await
            .unwrap();
        assert_eq!(applied, OperationApplyResult::Applied);

        let stale = repo
            .apply_hard_deleted_event(
                "server-burn-2",
                Some(1_900_000_111),
                Some(1_900_000_222),
                1_900_000_333,
                Some(29),
            )
            .await
            .unwrap();
        assert_eq!(stale, OperationApplyResult::IgnoredStale);

        let after = repo.get("server-burn-2").await.unwrap().unwrap();
        assert!(after.burn_enabled);
        assert_eq!(after.burn_status, BurnStatus::HardDeleted as i32);
        assert_eq!(after.burn_at, Some(1_900_000_001));
        assert_eq!(after.burned_at, Some(1_900_000_003));
        assert!(after.content_bytes.is_empty());
        assert_eq!(
            after.extra.get("burn_event").map(String::as_str),
            Some("hard_deleted")
        );
        assert_eq!(
            after
                .extra
                .get("lastHardDeleteEventSeq")
                .map(String::as_str),
            Some("30")
        );
    }
}
