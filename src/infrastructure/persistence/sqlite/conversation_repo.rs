//! SQLite 会话仓储：与 [schema] 中 conversations 表结构一致，按列读写，无 data BLOB。
//! 排序与 idx_conversations_sort 一致：is_archived → is_pinned DESC → last_message_at DESC。

use std::collections::HashMap;

use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

use crate::domain::{ConversationReader, ConversationWriter};
use crate::error::{ErrorCode, FlareError, Result};
use crate::model::Conversation;
use crate::model::conversation::{ConversationLocalState, ConversationType};
use crate::model::message_elem::MessagePreviewElem;

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
        let row = sqlx::query(
            r#"SELECT conversation_id, conversation_type, business_type, channel_id, members_count,
                      display_name, avatar_url, remark, description, last_message_id, last_sender_id,
                      last_message_at, last_message_preview, last_sender_nickname, last_sender_avatar_url,
                      unread_count, last_read_seq, max_seq, is_pinned, is_muted, is_archived,
                      version, updated_at, created_at, updated_at_ts, ext, draft,
                      mention_count, mention_me, badge, role
               FROM conversations WHERE conversation_id = ?"#,
        )
        .bind(conversation_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        row.map(|r| self.row_to_conversation(&r)).transpose()
    }

    /// 列表：与 idx_conversations_sort 一致 — is_archived → is_pinned DESC → last_message_at DESC
    async fn list(&self) -> Result<Vec<Conversation>> {
        let rows = sqlx::query(
            r#"SELECT conversation_id, conversation_type, business_type, channel_id, members_count,
                      display_name, avatar_url, remark, description, last_message_id, last_sender_id,
                      last_message_at, last_message_preview, last_sender_nickname, last_sender_avatar_url,
                      unread_count, last_read_seq, max_seq, is_pinned, is_muted, is_archived,
                      version, updated_at, created_at, updated_at_ts, ext, draft,
                      mention_count, mention_me, badge, role
               FROM conversations
               ORDER BY is_archived ASC, is_pinned DESC, COALESCE(last_message_at, 0) DESC"#,
        )
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
            let incoming_derived_last_read = c.max_seq.saturating_sub(c.unread_count as u64);
            // 保护读位单调性：服务端摘要偶发滞后时，不允许把本地 last_read_seq 回退。
            // 这样 read_states 能继续把更高读位回推服务端，避免重登未读“回弹”。
            let existing = sqlx::query(
                r#"SELECT last_read_seq, max_seq, unread_count,
                          last_message_id, last_sender_id, last_message_at, last_message_preview
                   FROM conversations
                   WHERE conversation_id = ?"#,
            )
            .bind(&c.conversation_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

            let mut merged = c.clone();
            if let Some(row) = existing {
                let prev_last_read_seq =
                    row.try_get::<i64, _>("last_read_seq").unwrap_or(0).max(0) as u64;
                let prev_max_seq = row.try_get::<i64, _>("max_seq").unwrap_or(0).max(0) as u64;
                let prev_unread = row.try_get::<i64, _>("unread_count").unwrap_or(0).max(0) as u32;
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

                merged.last_read_seq = merged
                    .last_read_seq
                    .max(incoming_derived_last_read)
                    .max(prev_last_read_seq);
                merged.max_seq = merged.max_seq.max(prev_max_seq);

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

                // 若本次摘要没有提供更新的序列位点，且 read_seq 反而更旧，则保持本地 unread，防止未读突增。
                if c.max_seq <= prev_max_seq && c.last_read_seq < prev_last_read_seq {
                    merged.unread_count = prev_unread;
                }
            } else {
                merged.last_read_seq = merged.last_read_seq.max(incoming_derived_last_read);
            }
            // unread 上界：最多为“最新位点 - 已读位点”
            let unread_upper_bound = merged.max_seq.saturating_sub(merged.last_read_seq) as u32;
            merged.unread_count = merged.unread_count.min(unread_upper_bound);

            let ext_json = serde_json::to_string(&merged.ext).unwrap_or_default();
            sqlx::query(
                r#"INSERT OR REPLACE INTO conversations (
                   conversation_id, conversation_type, business_type, channel_id, members_count,
                   display_name, avatar_url, remark, description, last_message_id, last_sender_id,
                   last_message_at, last_message_preview, last_sender_nickname, last_sender_avatar_url,
                   unread_count, last_read_seq, max_seq, is_pinned, is_muted, is_archived,
                   version, updated_at, created_at, updated_at_ts, ext, draft,
                   mention_count, mention_me, badge, role)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
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
        ConversationWriter::save_batch(self, &[conversation.clone()]).await
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
                 last_read_seq = MAX(COALESCE(last_read_seq, 0), ?),
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
               WHERE conversation_id = ? AND COALESCE(max_seq, 0) <= ?"#,
        )
        .bind(last_message_id)
        .bind(last_sender_id)
        .bind(last_message_at as i64)
        .bind(last_message_preview.unwrap_or(""))
        .bind(max_seq as i64)
        .bind(conversation_id)
        .bind(max_seq as i64)
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

    async fn delete(&self, conversation_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM conversations WHERE conversation_id = ?")
            .bind(conversation_id)
            .execute(&self.pool)
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
}

#[cfg(test)]
mod tests {
    use super::SqliteConversationRepo;
    use crate::domain::{ConversationReader, ConversationWriter};
    use crate::model::{Conversation, MessagePreviewElem};
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
}
