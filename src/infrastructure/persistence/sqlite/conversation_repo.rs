//! SQLite 会话仓储：与 [schema] 中 conversations 表结构一致，按列读写，无 data BLOB。
//! 排序与 idx_conversations_sort 一致：is_archived → is_pinned DESC → last_message_at DESC。

use std::collections::HashMap;

use async_trait::async_trait;
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

use crate::domain::{
    ConversationReader, ConversationWriter, ReadPosition, local_cleared_through_seq,
    preserve_local_remark, preserve_local_single_chat_channel, set_local_cleared_through_seq,
};
use crate::error::{ErrorCode, FlareError, Result};
use crate::model::conversation::{ConversationLocalState, ConversationType};
use crate::model::message_elem::MessagePreviewElem;
use crate::model::search::escaped_like_contains;
use crate::model::{Conversation, ConversationListQuery};

/// 与 schema 中 conversations 表列顺序一致的 i32 枚举（与 model prefix 一致：1=单聊 2=群聊 3=AI 4=系统 5=客服 6=临时）
fn conversation_type_to_i32(t: &ConversationType) -> i32 {
    match t {
        ConversationType::Unspecified => 0,
        ConversationType::Single => 1,
        ConversationType::Group => 2,
        ConversationType::Ai => 3,
        ConversationType::System => 4,
        ConversationType::Customer => 5,
        ConversationType::Temp => 6,
    }
}

fn i32_to_conversation_type(v: i32) -> ConversationType {
    match v {
        1 => ConversationType::Single,
        2 => ConversationType::Group,
        3 => ConversationType::Ai,
        4 => ConversationType::System,
        5 => ConversationType::Customer,
        6 => ConversationType::Temp,
        _ => ConversationType::Unspecified,
    }
}

fn sqlx_err(e: sqlx::Error) -> FlareError {
    FlareError::localized(ErrorCode::DatabaseError, e.to_string())
}

fn parse_ext(s: Option<&str>) -> HashMap<String, String> {
    let s = match s {
        Some(x) if !x.is_empty() => x,
        _ => return HashMap::new(),
    };
    serde_json::from_str(s).unwrap_or_default()
}

pub struct SqliteConversationRepo {
    pool: SqlitePool,
}

impl SqliteConversationRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn repair_missing_conversations_from_messages(&self) -> Result<()> {
        sqlx::query(
            r#"WITH latest_messages AS (
                   SELECT
                       m.conversation_id,
                       CASE
                           WHEN COALESCE(m.conversation_type, 0) > 0 THEN m.conversation_type
                           ELSE 1
                       END AS conversation_type,
                       m.channel_id,
                       m.sender_id,
                       m.sender_name,
                       m.sender_display_name,
                       m.server_id,
                       m.client_msg_id,
                       m.seq,
                       COALESCE(
                           NULLIF(TRIM(m.text), ''),
                           NULLIF(TRIM(CASE WHEN json_valid(COALESCE(m.extra, '')) THEN json_extract(m.extra, '$.contentText') ELSE NULL END), ''),
                           ''
                       ) AS preview,
                       CASE
                           WHEN m.seq > 0 THEN COALESCE(NULLIF(m.timestamp, 0), NULLIF(m.client_timestamp, 0), NULLIF(m.sort_ts, 0), 0)
                           ELSE max(max(COALESCE(m.sort_ts, 0), COALESCE(m.timestamp, 0)), COALESCE(m.client_timestamp, 0))
                       END AS effective_at
                   FROM messages m
                   WHERE TRIM(COALESCE(m.conversation_id, '')) != ''
                     AND COALESCE(m.is_recalled, 0) = 0
                     AND NOT EXISTS (
                         SELECT 1
                         FROM conversations c
                         WHERE c.conversation_id = m.conversation_id
                     )
                     AND m.rowid = (
                         SELECT s.rowid
                         FROM messages s
                         WHERE s.conversation_id = m.conversation_id
                           AND COALESCE(s.is_recalled, 0) = 0
                         ORDER BY
                             CASE
                                 WHEN s.seq = 0
                                  AND max(max(COALESCE(s.sort_ts, 0), COALESCE(s.timestamp, 0)), COALESCE(s.client_timestamp, 0)) >
                                      COALESCE((
                                          SELECT max(COALESCE(NULLIF(x.timestamp, 0), NULLIF(x.client_timestamp, 0), NULLIF(x.sort_ts, 0), 0))
                                          FROM messages x
                                          WHERE x.conversation_id = s.conversation_id
                                            AND x.seq > 0
                                            AND COALESCE(x.is_recalled, 0) = 0
                                      ), 0)
                                 THEN 3
                                 WHEN s.seq > 0
                                 THEN 2
                                 ELSE 0
                             END DESC,
                             CASE WHEN s.seq > 0 THEN s.seq ELSE 0 END DESC,
                             CASE
                                 WHEN s.seq > 0 THEN COALESCE(NULLIF(s.timestamp, 0), NULLIF(s.client_timestamp, 0), NULLIF(s.sort_ts, 0), 0)
                                 ELSE max(max(COALESCE(s.sort_ts, 0), COALESCE(s.timestamp, 0)), COALESCE(s.client_timestamp, 0))
                             END DESC,
                             s.rowid DESC
                         LIMIT 1
                     )
               )
               INSERT OR IGNORE INTO conversations (
                   conversation_id, conversation_type, business_type, channel_id, members_count,
                   display_name, avatar_url, remark, description,
                   last_message_id, last_sender_id, last_message_at, last_message_preview,
                   last_sender_nickname, last_sender_avatar_url,
                   unread_count, last_read_seq, max_seq,
                   is_pinned, is_muted, is_archived,
                   version, updated_at, created_at, updated_at_ts,
                   ext, draft, mention_count, mention_me, badge, role
               )
               SELECT
                   lm.conversation_id,
                   lm.conversation_type,
                   CASE
                       WHEN lm.conversation_type = 1 THEN 'single'
                       WHEN lm.conversation_type = 2 THEN 'group'
                       WHEN lm.conversation_type = 3 THEN 'ai'
                       WHEN lm.conversation_type = 4 THEN 'system'
                       WHEN lm.conversation_type = 5 THEN 'customer'
                       WHEN lm.conversation_type = 6 THEN 'temp'
                       ELSE 'chat'
                   END,
                   COALESCE(
                       NULLIF(TRIM(lm.channel_id), ''),
                       CASE
                           WHEN lm.conversation_type = 1 THEN NULLIF(TRIM(lm.sender_id), '')
                           ELSE NULL
                       END,
                       lm.conversation_id
                   ),
                   0,
                   COALESCE(
                       NULLIF(TRIM(lm.channel_id), ''),
                       NULLIF(TRIM(lm.sender_display_name), ''),
                       NULLIF(TRIM(lm.sender_name), ''),
                       NULLIF(TRIM(lm.sender_id), ''),
                       lm.conversation_id
                   ),
                   '',
                   NULL,
                   NULL,
                   COALESCE(NULLIF(TRIM(lm.server_id), ''), NULLIF(TRIM(lm.client_msg_id), '')),
                   NULLIF(TRIM(lm.sender_id), ''),
                   CASE
                       WHEN COALESCE(lm.effective_at, 0) > 0 THEN lm.effective_at
                       ELSE CAST(strftime('%s', 'now') AS INTEGER) * 1000
                   END,
                   lm.preview,
                   '',
                   '',
                   0,
                   0,
                   COALESCE((
                       SELECT MAX(COALESCE(mm.seq, 0))
                       FROM messages mm
                       WHERE mm.conversation_id = lm.conversation_id
                         AND COALESCE(mm.is_recalled, 0) = 0
                   ), 0),
                   0,
                   0,
                   0,
                   0,
                   CASE
                       WHEN COALESCE(lm.effective_at, 0) > 0 THEN lm.effective_at
                       ELSE CAST(strftime('%s', 'now') AS INTEGER) * 1000
                   END,
                   CASE
                       WHEN COALESCE(lm.effective_at, 0) > 0 THEN lm.effective_at
                       ELSE CAST(strftime('%s', 'now') AS INTEGER) * 1000
                   END,
                   CASE
                       WHEN COALESCE(lm.effective_at, 0) > 0 THEN lm.effective_at
                       ELSE CAST(strftime('%s', 'now') AS INTEGER) * 1000
                   END,
                   '',
                   NULL,
                   0,
                   0,
                   NULL,
                   NULL
               FROM latest_messages lm"#,
        )
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    fn select_with_latest_visible_message(where_clause: &str) -> String {
        format!(
            r#"SELECT c.conversation_id, c.conversation_type, c.business_type, c.channel_id,
                      c.members_count, c.display_name, c.avatar_url, c.remark, c.description,
                      CASE
                          WHEN lm.rowid IS NOT NULL AND (
                                   TRIM(COALESCE(c.last_message_preview, '')) = ''
                                OR (lm.seq > 0 AND lm.seq >= COALESCE(c.max_seq, 0))
                                OR (lm.seq > 0 AND COALESCE(lm.effective_at, 0) > COALESCE(c.last_message_at, 0))
                                OR (lm.seq = 0 AND COALESCE(lm.effective_at, 0) > COALESCE(c.last_message_at, 0))
                               )
                              THEN COALESCE(NULLIF(lm.server_id, ''), NULLIF(lm.client_msg_id, ''), NULLIF(TRIM(c.last_message_id), ''))
                          ELSE NULLIF(TRIM(c.last_message_id), '')
                      END AS last_message_id,
                      CASE
                          WHEN lm.rowid IS NOT NULL AND (
                                   TRIM(COALESCE(c.last_message_preview, '')) = ''
                                OR (lm.seq > 0 AND lm.seq >= COALESCE(c.max_seq, 0))
                                OR (lm.seq > 0 AND COALESCE(lm.effective_at, 0) > COALESCE(c.last_message_at, 0))
                                OR (lm.seq = 0 AND COALESCE(lm.effective_at, 0) > COALESCE(c.last_message_at, 0))
                               )
                              THEN COALESCE(NULLIF(lm.sender_id, ''), NULLIF(TRIM(c.last_sender_id), ''))
                          ELSE NULLIF(TRIM(c.last_sender_id), '')
                      END AS last_sender_id,
                      CASE
                          WHEN lm.rowid IS NOT NULL AND (
                                   TRIM(COALESCE(c.last_message_preview, '')) = ''
                                OR (lm.seq > 0 AND lm.seq >= COALESCE(c.max_seq, 0))
                                OR (lm.seq > 0 AND COALESCE(lm.effective_at, 0) > COALESCE(c.last_message_at, 0))
                                OR (lm.seq = 0 AND COALESCE(lm.effective_at, 0) > COALESCE(c.last_message_at, 0))
                               )
                              THEN COALESCE(NULLIF(lm.effective_at, 0), c.last_message_at)
                          ELSE c.last_message_at
                      END AS last_message_at,
                      CASE
                          WHEN lm.rowid IS NOT NULL AND (
                                   TRIM(COALESCE(c.last_message_preview, '')) = ''
                                OR (lm.seq > 0 AND lm.seq >= COALESCE(c.max_seq, 0))
                                OR (lm.seq > 0 AND COALESCE(lm.effective_at, 0) > COALESCE(c.last_message_at, 0))
                                OR (lm.seq = 0 AND COALESCE(lm.effective_at, 0) > COALESCE(c.last_message_at, 0))
                               ) THEN lm.text
                          ELSE c.last_message_preview
                      END AS last_message_preview,
                      c.last_sender_nickname, c.last_sender_avatar_url,
                      c.unread_count, c.last_read_seq, c.max_seq, c.visible_after_seq,
                      c.is_pinned, c.is_muted, c.is_archived, c.version, c.updated_at,
                      c.created_at, c.updated_at_ts, c.ext, c.draft,
                      c.mention_count, c.mention_me, c.badge, c.role
               FROM conversations c
               LEFT JOIN (
                   SELECT rowid, server_id, client_msg_id, sender_id, seq,
                          COALESCE(
                              NULLIF(TRIM(text), ''),
                              NULLIF(TRIM(CASE WHEN json_valid(COALESCE(extra, '')) THEN json_extract(extra, '$.contentText') ELSE NULL END), '')
                          ) AS text,
                          CASE
                              WHEN seq > 0 THEN COALESCE(NULLIF(timestamp, 0), NULLIF(client_timestamp, 0), NULLIF(sort_ts, 0), 0)
                              ELSE max(max(COALESCE(sort_ts, 0), COALESCE(timestamp, 0)), COALESCE(client_timestamp, 0))
                          END AS effective_at
                   FROM messages
               ) lm ON lm.rowid = (
                   SELECT m.rowid
                   FROM messages m
                   WHERE m.conversation_id = c.conversation_id
                     AND (m.seq = 0 OR m.seq > COALESCE(c.visible_after_seq, 0))
                     AND COALESCE(
                         NULLIF(TRIM(m.text), ''),
                         NULLIF(TRIM(CASE WHEN json_valid(COALESCE(m.extra, '')) THEN json_extract(m.extra, '$.contentText') ELSE NULL END), '')
                     ) IS NOT NULL
                   ORDER BY
                            CASE
                                WHEN m.seq = 0
                                 AND max(max(COALESCE(m.sort_ts, 0), COALESCE(m.timestamp, 0)), COALESCE(m.client_timestamp, 0)) >
                                     COALESCE((
                                         SELECT max(COALESCE(NULLIF(s.timestamp, 0), NULLIF(s.client_timestamp, 0), NULLIF(s.sort_ts, 0), 0))
                                         FROM messages s
                                         WHERE s.conversation_id = m.conversation_id
                                           AND s.seq > 0
                                           AND COALESCE(
                                               NULLIF(TRIM(s.text), ''),
                                               NULLIF(TRIM(CASE WHEN json_valid(COALESCE(s.extra, '')) THEN json_extract(s.extra, '$.contentText') ELSE NULL END), '')
                                           ) IS NOT NULL
                                     ), 0)
                                THEN 3
                                WHEN m.seq > 0
                                THEN 2
                                ELSE 0
                            END DESC,
                            CASE WHEN m.seq > 0 THEN m.seq ELSE 0 END DESC,
                            CASE
                                WHEN m.seq > 0 THEN COALESCE(NULLIF(m.timestamp, 0), NULLIF(m.client_timestamp, 0), NULLIF(m.sort_ts, 0), 0)
                                ELSE max(max(COALESCE(m.sort_ts, 0), COALESCE(m.timestamp, 0)), COALESCE(m.client_timestamp, 0))
                            END DESC
                   LIMIT 1
               )
               {where_clause}"#
        )
    }

    fn row_to_conversation(&self, row: &sqlx::sqlite::SqliteRow) -> Result<Conversation> {
        let conversation_id: String = row.try_get("conversation_id").map_err(sqlx_err)?;
        let conversation_type: i32 = row.try_get("conversation_type").map_err(sqlx_err)?;
        let business_type: String = row.try_get("business_type").map_err(sqlx_err)?;
        let channel_id: String = row.try_get("channel_id").map_err(sqlx_err)?;
        let members_count: i64 = row.try_get("members_count").map_err(sqlx_err)?;
        let display_name: String = row.try_get("display_name").map_err(sqlx_err)?;
        let avatar_url: String = row.try_get("avatar_url").map_err(sqlx_err)?;
        let remark: Option<String> = row.try_get("remark").map_err(sqlx_err)?;
        let description: Option<String> = row.try_get("description").map_err(sqlx_err)?;
        let last_message_id: Option<String> = row.try_get("last_message_id").map_err(sqlx_err)?;
        let last_sender_id: Option<String> = row.try_get("last_sender_id").map_err(sqlx_err)?;
        let last_message_at: Option<i64> = row.try_get("last_message_at").map_err(sqlx_err)?;
        let last_message_preview: Option<String> =
            row.try_get("last_message_preview").map_err(sqlx_err)?;
        let last_sender_nickname: String = row.try_get("last_sender_nickname").map_err(sqlx_err)?;
        let last_sender_avatar_url: String =
            row.try_get("last_sender_avatar_url").map_err(sqlx_err)?;
        let unread_count: i32 = row.try_get("unread_count").map_err(sqlx_err)?;
        let last_read_seq: i64 = row.try_get("last_read_seq").map_err(sqlx_err)?;
        let max_seq: i64 = row.try_get("max_seq").map_err(sqlx_err)?;
        let visible_after_seq: i64 = row.try_get("visible_after_seq").map_err(sqlx_err)?;
        let is_pinned: i32 = row.try_get("is_pinned").map_err(sqlx_err)?;
        let is_muted: i32 = row.try_get("is_muted").map_err(sqlx_err)?;
        let is_archived: i32 = row.try_get("is_archived").map_err(sqlx_err)?;
        let version: i64 = row.try_get("version").map_err(sqlx_err)?;
        let updated_at: i64 = row.try_get("updated_at").map_err(sqlx_err)?;
        let created_at: i64 = row.try_get("created_at").map_err(sqlx_err)?;
        let updated_at_ts: Option<i64> = row.try_get("updated_at_ts").map_err(sqlx_err)?;
        let ext_json: Option<String> = row.try_get("ext").map_err(sqlx_err)?;
        let draft: Option<String> = row.try_get("draft").map_err(sqlx_err)?;
        let mention_count: i32 = row.try_get("mention_count").map_err(sqlx_err)?;
        let mention_me: i32 = row.try_get("mention_me").map_err(sqlx_err)?;
        let badge: Option<String> = row.try_get("badge").map_err(sqlx_err)?;
        let role: Option<String> = row.try_get("role").map_err(sqlx_err)?;
        let ext = parse_ext(ext_json.as_deref());
        let peer_read_seq = ext
            .get("peer_read_seq")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or_default();

        let last_message = last_message_id.as_ref().map(|id| MessagePreviewElem {
            message_id: id.clone(),
            sender_id: last_sender_id.clone().unwrap_or_default(),
            r#type: 0,
            text: last_message_preview.clone().unwrap_or_default(),
            time: last_message_at.map(|ts| ts.max(0) as u64).unwrap_or(0),
        });

        Ok(Conversation {
            conversation_id,
            conversation_type: i32_to_conversation_type(conversation_type),
            business_type,
            channel_id,
            members_count: members_count.max(0) as u32,
            display_name,
            avatar_url,
            remark,
            description,
            last_message_id,
            last_sender_id,
            last_message_at: last_message_at.map(|t| t as u64),
            last_message_preview,
            last_message,
            last_sender_nickname,
            last_sender_avatar_url,
            unread_count: unread_count.max(0) as u32,
            last_read_seq: last_read_seq.max(0) as u64,
            peer_read_seq,
            max_seq: max_seq.max(0) as u64,
            visible_after_seq: visible_after_seq.max(0) as u64,
            is_pinned: is_pinned != 0,
            is_muted: is_muted != 0,
            is_archived: is_archived != 0,
            version: version.max(0) as u64,
            updated_at: updated_at.max(0) as u64,
            created_at: created_at.max(0) as u64,
            updated_at_ts: updated_at_ts.map(|t| t as u64),
            ext,
            participant_version: 0,
            member_preview: Vec::new(),
            draft,
            mention_count: mention_count.max(0) as u32,
            mention_me: mention_me != 0,
            badge,
            role,
            participants: Vec::new(),
            local_state: ConversationLocalState::default(),
        })
    }
}

#[async_trait]
impl ConversationReader for SqliteConversationRepo {
    async fn get(&self, conversation_id: &str) -> Result<Option<Conversation>> {
        self.repair_missing_conversations_from_messages().await?;
        let sql = Self::select_with_latest_visible_message("WHERE c.conversation_id = ?");
        let row = sqlx::query(&sql)
            .bind(conversation_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        row.map(|r| self.row_to_conversation(&r)).transpose()
    }

    /// 列表：与 idx_conversations_sort 一致 — is_archived → is_pinned DESC → last_message_at DESC
    async fn list(&self) -> Result<Vec<Conversation>> {
        self.repair_missing_conversations_from_messages().await?;
        let sql = Self::select_with_latest_visible_message(
            "ORDER BY c.is_archived ASC, c.is_pinned DESC, COALESCE(last_message_at, 0) DESC",
        );
        let rows = sqlx::query(&sql)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(self.row_to_conversation(&row)?);
        }
        Ok(out)
    }

    async fn list_by_query(&self, query: &ConversationListQuery) -> Result<Vec<Conversation>> {
        self.repair_missing_conversations_from_messages().await?;
        let base = Self::select_with_latest_visible_message("WHERE 1 = 1");
        let mut qb = QueryBuilder::<Sqlite>::new(base);

        if !query.include_archived {
            qb.push(" AND COALESCE(c.is_archived, 0) = 0");
        }
        if query.unread_only {
            qb.push(" AND COALESCE(c.unread_count, 0) > 0");
        }
        if query.mention_me_only {
            qb.push(" AND (COALESCE(c.mention_me, 0) != 0 OR COALESCE(c.mention_count, 0) > 0)");
        }
        if query.pinned_only {
            qb.push(" AND COALESCE(c.is_pinned, 0) != 0");
        }
        if let Some(muted) = query.muted_only {
            qb.push(" AND COALESCE(c.is_muted, 0) = ");
            qb.push_bind(if muted { 1i32 } else { 0i32 });
        }
        if query.has_draft_only {
            qb.push(" AND TRIM(COALESCE(c.draft, '')) != ''");
        }
        if !query.conversation_types.is_empty() {
            qb.push(" AND c.conversation_type IN (");
            let mut separated = qb.separated(", ");
            for conversation_type in &query.conversation_types {
                separated.push_bind(conversation_type_to_i32(conversation_type));
            }
            separated.push_unseparated(")");
        }
        if query.has_marked_messages {
            qb.push(
                r#" AND EXISTS (
                    SELECT 1
                    FROM messages mm
                    WHERE mm.conversation_id = c.conversation_id
                      AND (mm.seq = 0 OR mm.seq > COALESCE(c.visible_after_seq, 0))
                      AND COALESCE(mm.is_recalled, 0) = 0
                      AND COALESCE(mm.extra, '') LIKE '%markType%'
                    LIMIT 1
                )"#,
            );
        }
        if let Some(keyword) = query.normalized_keyword() {
            let like = escaped_like_contains(&keyword);
            qb.push(" AND (LOWER(COALESCE(c.conversation_id, '')) LIKE ");
            qb.push_bind(like.clone());
            qb.push(" ESCAPE '\\' OR LOWER(COALESCE(c.channel_id, '')) LIKE ");
            qb.push_bind(like.clone());
            qb.push(" ESCAPE '\\' OR LOWER(COALESCE(c.display_name, '')) LIKE ");
            qb.push_bind(like.clone());
            qb.push(" ESCAPE '\\' OR LOWER(COALESCE(c.remark, '')) LIKE ");
            qb.push_bind(like.clone());
            qb.push(" ESCAPE '\\' OR LOWER(COALESCE(c.description, '')) LIKE ");
            qb.push_bind(like.clone());
            qb.push(" ESCAPE '\\' OR LOWER(COALESCE(c.last_message_preview, '')) LIKE ");
            qb.push_bind(like);
            qb.push(" ESCAPE '\\')");
        }

        qb.push(" ORDER BY c.is_archived ASC, c.is_pinned DESC, COALESCE(last_message_at, 0) DESC");

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(self.row_to_conversation(&row)?);
        }
        Ok(out)
    }
}

#[async_trait]
impl ConversationWriter for SqliteConversationRepo {
    async fn save_batch(&self, conversations: &[Conversation]) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        for c in conversations {
            let existing = sqlx::query(
                r#"SELECT last_read_seq, max_seq, unread_count,
                          last_message_id, last_sender_id, last_message_at, last_message_preview,
                          is_pinned, is_muted, is_archived, visible_after_seq, ext, draft,
                          remark, channel_id, conversation_type
                   FROM conversations
                   WHERE conversation_id = ?"#,
            )
            .bind(&c.conversation_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

            let mut merged = c.clone();
            if let Some(row) = existing {
                let prev_max_seq = row.try_get::<i64, _>("max_seq").unwrap_or(0).max(0) as u64;
                let prev_unread = row.try_get::<i64, _>("unread_count").unwrap_or(0).max(0) as u32;
                let raw_prev_last_read =
                    row.try_get::<i64, _>("last_read_seq").unwrap_or(0).max(0) as u64;
                let local_read =
                    ReadPosition::from_parts(prev_max_seq, raw_prev_last_read, prev_unread);
                let merged_read = ReadPosition::merge_with_incoming_summary(
                    local_read,
                    ReadPosition::from_conversation(c),
                );
                merged.last_read_seq = merged_read.last_read_seq;
                merged.unread_count = merged_read.unread_count;
                merged.max_seq = merged.max_seq.max(merged_read.max_seq);
                let prev_last_message_id = row
                    .try_get::<Option<String>, _>("last_message_id")
                    .unwrap_or(None);
                let prev_last_sender_id = row
                    .try_get::<Option<String>, _>("last_sender_id")
                    .unwrap_or(None);
                let prev_last_message_at = row
                    .try_get::<Option<i64>, _>("last_message_at")
                    .unwrap_or(None)
                    .map(|t| t.max(0) as u64);
                let prev_last_message_preview = row
                    .try_get::<Option<String>, _>("last_message_preview")
                    .unwrap_or(None);
                let prev_is_pinned = row.try_get::<i32, _>("is_pinned").unwrap_or(0) != 0;
                let prev_is_muted = row.try_get::<i32, _>("is_muted").unwrap_or(0) != 0;
                let prev_is_archived = row.try_get::<i32, _>("is_archived").unwrap_or(0) != 0;
                let prev_draft = row.try_get::<Option<String>, _>("draft").unwrap_or(None);
                let prev_ext_json = row.try_get::<Option<String>, _>("ext").unwrap_or(None);
                let prev_ext: std::collections::HashMap<String, String> = prev_ext_json
                    .as_deref()
                    .and_then(|raw| serde_json::from_str(raw).ok())
                    .unwrap_or_default();
                let prev_settings_version = prev_ext
                    .get(crate::model::EXT_USER_SETTINGS_VERSION)
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);
                let incoming_settings_version = merged
                    .ext
                    .get(crate::model::EXT_USER_SETTINGS_VERSION)
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);
                let prev_dirty = prev_ext
                    .get(crate::model::EXT_SETTINGS_DIRTY)
                    .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));

                if prev_dirty && incoming_settings_version <= prev_settings_version {
                    merged.is_pinned = prev_is_pinned;
                    merged.is_muted = prev_is_muted;
                    merged.is_archived = prev_is_archived;
                    merged.draft = prev_draft;
                    merged.ext = prev_ext;
                } else if incoming_settings_version >= prev_settings_version {
                    for (k, v) in prev_ext {
                        merged.ext.entry(k).or_insert(v);
                    }
                    merged.ext.insert(
                        crate::model::EXT_SETTINGS_DIRTY.to_string(),
                        "0".to_string(),
                    );
                } else {
                    merged.is_pinned = prev_is_pinned;
                    merged.is_muted = prev_is_muted;
                    merged.is_archived = prev_is_archived;
                    merged.draft = prev_draft;
                    merged.ext = prev_ext;
                }

                let prev_has_last_message = prev_last_message_id
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|v| !v.is_empty())
                    || prev_last_message_preview
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|v| !v.is_empty())
                    || prev_last_message_at.unwrap_or_default() > 0;
                let incoming_has_last_message = c
                    .last_message_id
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|v| !v.is_empty())
                    || c.last_message_preview
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|v| !v.is_empty())
                    || c.last_message_at.unwrap_or_default() > 0;
                // 会话摘要只是服务端投影；本地消息表同步出来的最新消息更权威。
                // 避免服务端摘要滞后时把会话列表预览回滚到旧消息或空消息。
                if prev_has_last_message
                    && (c.max_seq <= prev_max_seq || !incoming_has_last_message)
                {
                    merged.last_message_id = prev_last_message_id;
                    merged.last_sender_id = prev_last_sender_id;
                    merged.last_message_at = prev_last_message_at;
                    merged.last_message_preview = prev_last_message_preview;
                }

                let prev_remark = row.try_get::<Option<String>, _>("remark").unwrap_or(None);
                merged.remark =
                    preserve_local_remark(merged.remark.as_deref(), prev_remark.as_deref());
                if let Some(remark) = merged
                    .remark
                    .as_deref()
                    .map(str::trim)
                    .filter(|remark| !remark.is_empty())
                {
                    merged.display_name = remark.to_string();
                }

                let prev_channel_id = row
                    .try_get::<Option<String>, _>("channel_id")
                    .unwrap_or(None);
                let prev_conv_type = row.try_get::<i32, _>("conversation_type").unwrap_or(0);
                merged.channel_id = preserve_local_single_chat_channel(
                    i32_to_conversation_type(prev_conv_type),
                    &merged.channel_id,
                    prev_channel_id.as_deref().unwrap_or_default(),
                );
            } else {
                let merged_read = ReadPosition::merge_with_incoming_summary(
                    ReadPosition::default(),
                    ReadPosition::from_conversation(c),
                );
                merged.last_read_seq = merged_read.last_read_seq;
                merged.unread_count = merged_read.unread_count;
                merged.max_seq = merged.max_seq.max(merged_read.max_seq);
            }

            // Server-visible history boundary is authoritative from the incoming summary.
            // Local clear is preserved by its explicit local_cleared_through_seq marker.
            let local_cleared_floor =
                local_cleared_through_seq(&merged.ext).max(merged.visible_after_seq);
            merged.visible_after_seq = local_cleared_floor;
            if local_cleared_floor > 0 {
                merged.last_read_seq = merged.last_read_seq.max(local_cleared_floor);
                merged.max_seq = merged.max_seq.max(local_cleared_floor);
                if c.max_seq <= local_cleared_floor {
                    merged.last_message_id = None;
                    merged.last_sender_id = None;
                    merged.last_message_at = None;
                    merged.last_message_preview = None;
                    merged.last_message = None;
                    merged.unread_count = 0;
                }
            }
            // unread 上界：最多为“最新位点 - 已读位点”
            merged.unread_count = merged
                .unread_count
                .min(ReadPosition::from_conversation(&merged).unread_upper_bound());

            let ext_json = serde_json::to_string(&merged.ext).unwrap_or_default();
            sqlx::query(
                r#"INSERT OR REPLACE INTO conversations (
                   conversation_id, conversation_type, business_type, channel_id, members_count,
                   display_name, avatar_url, remark, description, last_message_id, last_sender_id,
                   last_message_at, last_message_preview, last_sender_nickname, last_sender_avatar_url,
                   unread_count, last_read_seq, max_seq, visible_after_seq, is_pinned, is_muted, is_archived,
                   version, updated_at, created_at, updated_at_ts, ext, draft,
                   mention_count, mention_me, badge, role)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&merged.conversation_id)
            .bind(conversation_type_to_i32(&merged.conversation_type))
            .bind(&merged.business_type)
            .bind(&merged.channel_id)
            .bind(merged.members_count as i64)
            .bind(&merged.display_name)
            .bind(&merged.avatar_url)
            .bind(&merged.remark)
            .bind(&merged.description)
            .bind(&merged.last_message_id)
            .bind(&merged.last_sender_id)
            .bind(merged.last_message_at.map(|t| t as i64))
            .bind(&merged.last_message_preview)
            .bind(&merged.last_sender_nickname)
            .bind(&merged.last_sender_avatar_url)
            .bind(merged.unread_count as i32)
            .bind(merged.last_read_seq as i64)
            .bind(merged.max_seq as i64)
            .bind(merged.visible_after_seq as i64)
            .bind(if merged.is_pinned { 1i32 } else { 0 })
            .bind(if merged.is_muted { 1i32 } else { 0 })
            .bind(if merged.is_archived { 1i32 } else { 0 })
            .bind(merged.version as i64)
            .bind(merged.updated_at as i64)
            .bind(merged.created_at as i64)
            .bind(merged.updated_at_ts.map(|t| t as i64))
            .bind(&ext_json)
            .bind(&merged.draft)
            .bind(merged.mention_count as i32)
            .bind(if merged.mention_me { 1i32 } else { 0 })
            .bind(&merged.badge)
            .bind(&merged.role)
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn save_one(&self, conversation: &Conversation) -> Result<()> {
        ConversationWriter::save_batch(self, std::slice::from_ref(conversation)).await
    }

    async fn update_unread(
        &self,
        conversation_id: &str,
        unread_count: u32,
        last_read_seq: u64,
    ) -> Result<()> {
        sqlx::query(
            r#"UPDATE conversations
               SET
                 last_read_seq = MAX(0, ?),
                 max_seq = MAX(COALESCE(max_seq, 0), ?),
                 unread_count = MAX(0, ?)
               WHERE conversation_id = ?"#,
        )
        .bind(last_read_seq as i64)
        .bind(last_read_seq as i64)
        .bind(unread_count as i64)
        .bind(conversation_id)
        .execute(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn set_pinned(&self, conversation_id: &str, pinned: bool) -> Result<()> {
        sqlx::query(r#"UPDATE conversations SET is_pinned = ? WHERE conversation_id = ?"#)
            .bind(if pinned { 1i32 } else { 0 })
            .bind(conversation_id)
            .execute(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn set_muted(&self, conversation_id: &str, muted: bool) -> Result<()> {
        sqlx::query(r#"UPDATE conversations SET is_muted = ? WHERE conversation_id = ?"#)
            .bind(if muted { 1i32 } else { 0 })
            .bind(conversation_id)
            .execute(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn set_archived(&self, conversation_id: &str, archived: bool) -> Result<()> {
        sqlx::query(r#"UPDATE conversations SET is_archived = ? WHERE conversation_id = ?"#)
            .bind(if archived { 1i32 } else { 0 })
            .bind(conversation_id)
            .execute(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn mark_unread(&self, conversation_id: &str) -> Result<u32> {
        sqlx::query(
            r#"UPDATE conversations
               SET last_read_seq = CASE WHEN max_seq > 0 THEN max_seq - 1 ELSE 0 END,
                   unread_count = CASE WHEN max_seq > 0 THEN 1 ELSE 1 END
               WHERE conversation_id = ?"#,
        )
        .bind(conversation_id)
        .execute(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let row =
            sqlx::query(r#"SELECT unread_count FROM conversations WHERE conversation_id = ?"#)
                .bind(conversation_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(row
            .and_then(|r| r.try_get::<i64, _>("unread_count").ok())
            .unwrap_or(1)
            .max(0) as u32)
    }

    async fn update_draft(&self, conversation_id: &str, draft: Option<&str>) -> Result<()> {
        sqlx::query(r#"UPDATE conversations SET draft = ? WHERE conversation_id = ?"#)
            .bind(draft)
            .bind(conversation_id)
            .execute(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn update_last_message(
        &self,
        conversation_id: &str,
        last_message_id: &str,
        last_sender_id: &str,
        last_message_at: u64,
        last_message_preview: Option<&str>,
        max_seq: u64,
    ) -> Result<()> {
        sqlx::query(
            r#"UPDATE conversations SET
               last_message_id = ?, last_sender_id = ?, last_message_at = ?, last_message_preview = ?,
               max_seq = MAX(COALESCE(max_seq, 0), ?)
               WHERE conversation_id = ?
                 AND (
                   COALESCE(max_seq, 0) <= ?
                   OR COALESCE(last_message_at, 0) <= ?
                 )"#,
        )
        .bind(last_message_id)
        .bind(last_sender_id)
        .bind(last_message_at as i64)
        .bind(last_message_preview.unwrap_or(""))
        .bind(max_seq as i64)
        .bind(conversation_id)
        .bind(max_seq as i64)
        .bind(last_message_at as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn recompute_unread_for_user(
        &self,
        conversation_id: &str,
        current_user_id: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"UPDATE conversations
               SET unread_count = (
                   SELECT COUNT(1)
                   FROM messages m
                   WHERE m.conversation_id = conversations.conversation_id
                     AND COALESCE(m.seq, 0) > COALESCE(conversations.last_read_seq, 0)
                     AND COALESCE(m.sender_id, '') <> ?
                     AND COALESCE(m.is_recalled, 0) = 0
               )
               WHERE conversation_id = ?"#,
        )
        .bind(current_user_id)
        .bind(conversation_id)
        .execute(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn clear_local_chat_history(
        &self,
        conversation_id: &str,
        cleared_through_seq: u64,
    ) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

        if cleared_through_seq > 0 {
            let ext_row: Option<(Option<String>,)> =
                sqlx::query_as("SELECT ext FROM conversations WHERE conversation_id = ?")
                    .bind(conversation_id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
            let mut ext = parse_ext(ext_row.and_then(|(e,)| e).as_deref());
            set_local_cleared_through_seq(&mut ext, cleared_through_seq);
            let ext_json = serde_json::to_string(&ext)
                .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
            sqlx::query("UPDATE conversations SET ext = ? WHERE conversation_id = ?")
                .bind(ext_json)
                .bind(conversation_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        }

        sqlx::query("DELETE FROM message_reactions WHERE conversation_id = ?")
            .bind(conversation_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        sqlx::query("DELETE FROM messages WHERE conversation_id = ?")
            .bind(conversation_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        sqlx::query("DELETE FROM pending_sends WHERE conversation_id = ?")
            .bind(conversation_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        sqlx::query(
            r#"UPDATE conversations SET
               last_message_id = NULL,
               last_sender_id = NULL,
               last_message_at = NULL,
               last_message_preview = NULL,
               unread_count = 0,
               last_read_seq = MAX(COALESCE(last_read_seq, 0), ?),
               max_seq = MAX(COALESCE(max_seq, 0), ?),
               visible_after_seq = MAX(COALESCE(visible_after_seq, 0), ?)
               WHERE conversation_id = ?"#,
        )
        .bind(cleared_through_seq as i64)
        .bind(cleared_through_seq as i64)
        .bind(cleared_through_seq as i64)
        .bind(conversation_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, conversation_id: &str) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

        sqlx::query("DELETE FROM message_reactions WHERE conversation_id = ?")
            .bind(conversation_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        sqlx::query("DELETE FROM messages WHERE conversation_id = ?")
            .bind(conversation_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        sqlx::query("DELETE FROM pending_sends WHERE conversation_id = ?")
            .bind(conversation_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        sqlx::query("DELETE FROM conversation_participants WHERE conversation_id = ?")
            .bind(conversation_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        sqlx::query("DELETE FROM sync_conversation_cursors WHERE conversation_id = ?")
            .bind(conversation_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        sqlx::query("DELETE FROM conversations WHERE conversation_id = ?")
            .bind(conversation_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }
    async fn get_local_max_seq(&self, conversation_id: &str) -> Result<u64> {
        let row = sqlx::query(
            r#"SELECT COALESCE(MAX(seq), 0) AS max_seq
               FROM messages
               WHERE conversation_id = ?"#,
        )
        .bind(conversation_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let max_seq = row.try_get::<i64, _>("max_seq").unwrap_or(0).max(0) as u64;
        Ok(max_seq)
    }

    async fn apply_user_profile_snapshot(
        &self,
        user_id: &str,
        nickname: &str,
        avatar_url: &str,
    ) -> Result<Vec<String>> {
        let rows = sqlx::query(
            r#"SELECT conversation_id
               FROM conversations
               WHERE (conversation_type = 1 AND channel_id = ?)
                  OR last_sender_id = ?"#,
        )
        .bind(user_id)
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let mut affected = Vec::with_capacity(rows.len());
        let now = crate::util::id::now_millis() as i64;
        for row in rows {
            let conversation_id: String = row.try_get("conversation_id").map_err(sqlx_err)?;
            affected.push(conversation_id.clone());
            sqlx::query(
                r#"UPDATE conversations SET
                   display_name = CASE
                       WHEN conversation_type = 1 AND channel_id = ? AND TRIM(COALESCE(remark, '')) = '' THEN ?
                       ELSE display_name
                   END,
                   avatar_url = CASE
                       WHEN conversation_type = 1 AND channel_id = ? THEN ?
                       ELSE avatar_url
                   END,
                   last_sender_nickname = CASE
                       WHEN last_sender_id = ? THEN ?
                       ELSE last_sender_nickname
                   END,
                   last_sender_avatar_url = CASE
                       WHEN last_sender_id = ? THEN ?
                       ELSE last_sender_avatar_url
                   END,
                   updated_at = ?
                   WHERE conversation_id = ?"#,
            )
            .bind(user_id)
            .bind(nickname)
            .bind(user_id)
            .bind(avatar_url)
            .bind(user_id)
            .bind(nickname)
            .bind(user_id)
            .bind(avatar_url)
            .bind(now)
            .bind(conversation_id)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        }
        Ok(affected)
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteConversationRepo;
    use crate::domain::{ConversationReader, ConversationWriter};
    use crate::model::{Conversation, ConversationListQuery, MessagePreviewElem};
    use sqlx::SqlitePool;

    async fn repo() -> SqliteConversationRepo {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        super::super::schema::init_schema(&pool).await.unwrap();
        SqliteConversationRepo::new(pool)
    }

    fn conversation(conversation_id: &str, seq: u64, text: &str) -> Conversation {
        Conversation {
            conversation_id: conversation_id.to_string(),
            business_type: "chat".to_string(),
            channel_id: conversation_id.to_string(),
            display_name: conversation_id.to_string(),
            last_message_id: Some(format!("msg-{seq}")),
            last_sender_id: Some("u1".to_string()),
            last_message_at: Some(seq * 1000),
            last_message_preview: Some(text.to_string()),
            last_message: Some(MessagePreviewElem {
                message_id: format!("msg-{seq}"),
                sender_id: "u1".to_string(),
                r#type: 1,
                text: text.to_string(),
                time: seq * 1000,
            }),
            max_seq: seq,
            updated_at: seq * 1000,
            created_at: 1,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn list_repairs_missing_conversation_from_local_messages() {
        let repo = repo().await;

        sqlx::query(
            r#"INSERT INTO messages (
                   server_id, conversation_id, client_msg_id, sender_id, seq,
                   timestamp, client_timestamp, conversation_type, message_type,
                   channel_id, sender_name, sender_display_name,
                   content, text, sort_ts, updated_at
               ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, X'', ?, ?, ?)"#,
        )
        .bind("msg-orphan-1")
        .bind("conv-orphan")
        .bind("client-orphan-1")
        .bind("peer-1")
        .bind(1_i64)
        .bind(10_000_i64)
        .bind(10_000_i64)
        .bind(1_i32)
        .bind(1_i32)
        .bind("peer-1")
        .bind("Peer One")
        .bind("Peer One")
        .bind("older")
        .bind(10_000_i64)
        .bind(10_000_i64)
        .execute(&repo.pool)
        .await
        .unwrap();

        sqlx::query(
            r#"INSERT INTO messages (
                   server_id, conversation_id, client_msg_id, sender_id, seq,
                   timestamp, client_timestamp, conversation_type, message_type,
                   channel_id, sender_name, sender_display_name,
                   content, text, sort_ts, updated_at
               ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, X'', ?, ?, ?)"#,
        )
        .bind("msg-orphan-2")
        .bind("conv-orphan")
        .bind("client-orphan-2")
        .bind("peer-1")
        .bind(2_i64)
        .bind(20_000_i64)
        .bind(20_000_i64)
        .bind(1_i32)
        .bind(1_i32)
        .bind("peer-1")
        .bind("Peer One")
        .bind("Peer One")
        .bind("latest")
        .bind(20_000_i64)
        .bind(20_000_i64)
        .execute(&repo.pool)
        .await
        .unwrap();

        let listed = repo.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].conversation_id, "conv-orphan");
        assert_eq!(listed[0].channel_id, "peer-1");
        assert_eq!(listed[0].display_name, "peer-1");
        assert_eq!(listed[0].last_message_id.as_deref(), Some("msg-orphan-2"));
        assert_eq!(listed[0].last_sender_id.as_deref(), Some("peer-1"));
        assert_eq!(listed[0].last_message_at, Some(20_000));
        assert_eq!(listed[0].last_message_preview.as_deref(), Some("latest"));
        assert_eq!(listed[0].max_seq, 2);

        let loaded = repo.get("conv-orphan").await.unwrap().unwrap();
        assert_eq!(loaded.last_message_preview.as_deref(), Some("latest"));
    }

    #[tokio::test]
    async fn save_batch_keeps_local_latest_message_when_summary_is_stale() {
        let repo = repo().await;
        repo.save_one(&conversation("conv-1", 10, "new-local"))
            .await
            .unwrap();

        repo.save_one(&conversation("conv-1", 8, "old-server"))
            .await
            .unwrap();

        let loaded = repo.get("conv-1").await.unwrap().unwrap();
        assert_eq!(loaded.max_seq, 10);
        assert_eq!(loaded.last_message_preview.as_deref(), Some("new-local"));
        assert_eq!(loaded.last_message_id.as_deref(), Some("msg-10"));
    }

    #[tokio::test]
    async fn save_batch_does_not_restore_messages_before_local_clear_floor() {
        let repo = repo().await;
        repo.save_one(&conversation("conv-1", 10, "before-delete"))
            .await
            .unwrap();
        repo.clear_local_chat_history("conv-1", 10).await.unwrap();

        repo.save_one(&conversation("conv-1", 10, "old-server"))
            .await
            .unwrap();
        let hidden = repo.get("conv-1").await.unwrap().unwrap();
        assert_eq!(hidden.last_read_seq, 10);
        assert_eq!(hidden.unread_count, 0);
        assert!(hidden.last_message_id.is_none());
        assert!(hidden.last_message_preview.is_none());

        repo.save_one(&conversation("conv-1", 11, "after-readd"))
            .await
            .unwrap();
        let visible = repo.get("conv-1").await.unwrap().unwrap();
        assert_eq!(visible.max_seq, 11);
        assert_eq!(visible.last_message_preview.as_deref(), Some("after-readd"));
        assert_eq!(visible.last_message_id.as_deref(), Some("msg-11"));
    }

    #[tokio::test]
    async fn save_batch_does_not_treat_previous_sync_cursor_as_visible_floor() {
        let repo = repo().await;
        let mut polluted = conversation("conv-1", 10, "cursor-polluted");
        polluted.visible_after_seq = 10;
        repo.save_one(&polluted).await.unwrap();

        repo.save_one(&conversation("conv-1", 10, "server-summary"))
            .await
            .unwrap();

        let loaded = repo.get("conv-1").await.unwrap().unwrap();
        assert_eq!(loaded.visible_after_seq, 0);
        assert_eq!(
            loaded.last_message_preview.as_deref(),
            Some("server-summary")
        );
    }

    #[tokio::test]
    async fn save_batch_keeps_local_remark_when_summary_has_none() {
        let repo = repo().await;
        let mut local = conversation("conv-1", 10, "hello");
        local.conversation_type = crate::model::conversation::ConversationType::Single;
        local.channel_id = "peer-u2".to_string();
        local.remark = Some("我的备注".to_string());
        repo.save_one(&local).await.unwrap();

        let mut summary = conversation("conv-1", 10, "hello");
        summary.conversation_type = crate::model::conversation::ConversationType::Single;
        summary.channel_id = "123456".to_string();
        summary.remark = None;
        repo.save_batch(&[summary]).await.unwrap();

        let loaded = repo.get("conv-1").await.unwrap().unwrap();
        assert_eq!(loaded.remark.as_deref(), Some("我的备注"));
        assert_eq!(loaded.channel_id, "peer-u2");
    }

    #[tokio::test]
    async fn update_last_message_does_not_roll_back_newer_projection() {
        let repo = repo().await;
        repo.save_one(&conversation("conv-1", 10, "new-local"))
            .await
            .unwrap();

        repo.update_last_message("conv-1", "msg-8", "u2", 8000, Some("old-server"), 8)
            .await
            .unwrap();

        let loaded = repo.get("conv-1").await.unwrap().unwrap();
        assert_eq!(loaded.max_seq, 10);
        assert_eq!(loaded.last_message_preview.as_deref(), Some("new-local"));
        assert_eq!(loaded.last_message_id.as_deref(), Some("msg-10"));
    }

    #[tokio::test]
    async fn update_last_message_accepts_newer_time_when_summary_max_seq_is_ahead() {
        let repo = repo().await;
        let mut summary = conversation("conv-1", 99, "stale-summary");
        summary.last_message_id = Some("msg-11".to_string());
        summary.last_sender_id = Some("u1".to_string());
        summary.last_message_at = Some(11_000);
        summary.last_message_preview = Some("stale-summary".to_string());
        repo.save_one(&summary).await.unwrap();

        repo.update_last_message("conv-1", "msg-12", "u2", 12_345, Some("111"), 12)
            .await
            .unwrap();

        let loaded = repo.get("conv-1").await.unwrap().unwrap();
        assert_eq!(loaded.max_seq, 99);
        assert_eq!(loaded.last_message_id.as_deref(), Some("msg-12"));
        assert_eq!(loaded.last_sender_id.as_deref(), Some("u2"));
        assert_eq!(loaded.last_message_at, Some(12_345));
        assert_eq!(loaded.last_message_preview.as_deref(), Some("111"));
    }

    #[tokio::test]
    async fn list_by_query_filters_unread_type_keyword_and_marked_messages() {
        let repo = repo().await;
        let mut single = conversation("conv-single", 10, "needle marked");
        single.conversation_type = crate::model::conversation::ConversationType::Single;
        single.unread_count = 2;
        repo.save_one(&single).await.unwrap();

        let mut group = conversation("conv-group", 8, "needle plain");
        group.conversation_type = crate::model::conversation::ConversationType::Group;
        group.unread_count = 1;
        repo.save_one(&group).await.unwrap();

        sqlx::query(
            r#"INSERT INTO messages (
                   server_id, conversation_id, client_msg_id, sender_id, seq,
                   timestamp, client_timestamp, conversation_type, message_type,
                   channel_id, content, text, extra, sort_ts, updated_at
               ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, X'', ?, ?, ?, ?)"#,
        )
        .bind("marked-msg")
        .bind("conv-single")
        .bind("client-marked-msg")
        .bind("u2")
        .bind(10_i64)
        .bind(10_000_i64)
        .bind(10_000_i64)
        .bind(1_i32)
        .bind(1_i32)
        .bind("u2")
        .bind("needle marked")
        .bind(r#"{"markType":"1"}"#)
        .bind(10_000_i64)
        .bind(10_000_i64)
        .execute(&repo.pool)
        .await
        .unwrap();

        let filtered = repo
            .list_by_query(&ConversationListQuery {
                keyword: Some("needle".to_string()),
                unread_only: true,
                has_marked_messages: true,
                conversation_types: vec![crate::model::conversation::ConversationType::Single],
                ..ConversationListQuery::default()
            })
            .await
            .unwrap();

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].conversation_id, "conv-single");
    }

    #[tokio::test]
    async fn read_model_uses_latest_visible_message_when_summary_preview_is_missing() {
        let repo = repo().await;
        let mut summary = conversation("conv-1", 12, "");
        summary.last_message_id = None;
        summary.last_sender_id = None;
        summary.last_message_at = Some(12_000);
        summary.last_message_preview = None;
        summary.last_message = None;
        summary.visible_after_seq = 10;
        repo.save_one(&summary).await.unwrap();

        sqlx::query(
            r#"INSERT INTO messages (
                   server_id, conversation_id, client_msg_id, sender_id, seq,
                   timestamp, client_timestamp, conversation_type, message_type,
                   channel_id, content, text, sort_ts, updated_at
               ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, X'', ?, ?, ?)"#,
        )
        .bind("msg-12")
        .bind("conv-1")
        .bind("client-12")
        .bind("u2")
        .bind(12_i64)
        .bind(12_345_i64)
        .bind(12_300_i64)
        .bind(1_i32)
        .bind(1_i32)
        .bind("u2")
        .bind("latest-visible")
        .bind(12_345_i64)
        .bind(12_345_i64)
        .execute(&repo.pool)
        .await
        .unwrap();

        let loaded = repo.get("conv-1").await.unwrap().unwrap();
        assert_eq!(loaded.last_message_id.as_deref(), Some("msg-12"));
        assert_eq!(loaded.last_sender_id.as_deref(), Some("u2"));
        assert_eq!(loaded.last_message_at, Some(12_345));
        assert_eq!(
            loaded.last_message_preview.as_deref(),
            Some("latest-visible")
        );

        let listed = repo.list().await.unwrap();
        assert_eq!(
            listed[0].last_message_preview.as_deref(),
            Some("latest-visible")
        );
    }

    #[tokio::test]
    async fn read_model_uses_latest_visible_message_when_summary_preview_is_stale() {
        let repo = repo().await;
        let mut summary = conversation("conv-1", 12, "stale-summary");
        summary.last_message_id = Some("msg-11".to_string());
        summary.last_sender_id = Some("u1".to_string());
        summary.last_message_at = Some(11_000);
        summary.max_seq = 12;
        repo.save_one(&summary).await.unwrap();

        sqlx::query(
            r#"INSERT INTO messages (
                   server_id, conversation_id, client_msg_id, sender_id, seq,
                   timestamp, client_timestamp, conversation_type, message_type,
                   channel_id, content, text, sort_ts, updated_at
               ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, X'', ?, ?, ?)"#,
        )
        .bind("msg-12")
        .bind("conv-1")
        .bind("client-12")
        .bind("u2")
        .bind(12_i64)
        .bind(12_345_i64)
        .bind(12_300_i64)
        .bind(1_i32)
        .bind(1_i32)
        .bind("u2")
        .bind("latest-visible")
        .bind(12_345_i64)
        .bind(12_345_i64)
        .execute(&repo.pool)
        .await
        .unwrap();

        let loaded = repo.get("conv-1").await.unwrap().unwrap();
        assert_eq!(loaded.last_message_id.as_deref(), Some("msg-12"));
        assert_eq!(loaded.last_sender_id.as_deref(), Some("u2"));
        assert_eq!(loaded.last_message_at, Some(12_345));
        assert_eq!(
            loaded.last_message_preview.as_deref(),
            Some("latest-visible")
        );
    }

    #[tokio::test]
    async fn read_model_uses_newer_local_message_when_summary_max_seq_is_ahead() {
        let repo = repo().await;
        let mut summary = conversation("conv-1", 99, "stale-summary");
        summary.last_message_id = Some("msg-11".to_string());
        summary.last_sender_id = Some("u1".to_string());
        summary.last_message_at = Some(11_000);
        summary.last_message_preview = Some("stale-summary".to_string());
        summary.max_seq = 99;
        repo.save_one(&summary).await.unwrap();

        sqlx::query(
            r#"INSERT INTO messages (
                   server_id, conversation_id, client_msg_id, sender_id, seq,
                   timestamp, client_timestamp, conversation_type, message_type,
                   channel_id, content, text, sort_ts, updated_at
               ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, X'', ?, ?, ?)"#,
        )
        .bind("msg-12")
        .bind("conv-1")
        .bind("client-12")
        .bind("u2")
        .bind(12_i64)
        .bind(12_345_i64)
        .bind(12_300_i64)
        .bind(1_i32)
        .bind(1_i32)
        .bind("u2")
        .bind("111")
        .bind(12_345_i64)
        .bind(12_345_i64)
        .execute(&repo.pool)
        .await
        .unwrap();

        let loaded = repo.get("conv-1").await.unwrap().unwrap();
        assert_eq!(loaded.max_seq, 99);
        assert_eq!(loaded.last_message_id.as_deref(), Some("msg-12"));
        assert_eq!(loaded.last_sender_id.as_deref(), Some("u2"));
        assert_eq!(loaded.last_message_at, Some(12_345));
        assert_eq!(loaded.last_message_preview.as_deref(), Some("111"));
    }

    #[tokio::test]
    async fn read_model_uses_extra_content_text_when_message_text_is_missing() {
        let repo = repo().await;
        let mut summary = conversation("conv-1", 99, "stale-summary");
        summary.last_message_id = Some("msg-11".to_string());
        summary.last_sender_id = Some("u1".to_string());
        summary.last_message_at = Some(11_000);
        summary.last_message_preview = Some("stale-summary".to_string());
        summary.max_seq = 99;
        repo.save_one(&summary).await.unwrap();

        sqlx::query(
            r#"INSERT INTO messages (
                   server_id, conversation_id, client_msg_id, sender_id, seq,
                   timestamp, client_timestamp, conversation_type, message_type,
                   channel_id, content, text, extra, sort_ts, updated_at
               ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, X'', NULL, ?, ?, ?)"#,
        )
        .bind("msg-12")
        .bind("conv-1")
        .bind("client-12")
        .bind("u2")
        .bind(12_i64)
        .bind(12_345_i64)
        .bind(12_300_i64)
        .bind(1_i32)
        .bind(1_i32)
        .bind("u2")
        .bind(r#"{"contentText":"111"}"#)
        .bind(12_345_i64)
        .bind(12_345_i64)
        .execute(&repo.pool)
        .await
        .unwrap();

        let loaded = repo.get("conv-1").await.unwrap().unwrap();
        assert_eq!(loaded.max_seq, 99);
        assert_eq!(loaded.last_message_id.as_deref(), Some("msg-12"));
        assert_eq!(loaded.last_sender_id.as_deref(), Some("u2"));
        assert_eq!(loaded.last_message_at, Some(12_345));
        assert_eq!(loaded.last_message_preview.as_deref(), Some("111"));
    }
}
