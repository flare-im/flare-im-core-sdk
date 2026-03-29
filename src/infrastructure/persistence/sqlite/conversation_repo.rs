//! SQLite 会话仓储：与 [schema] 中 conversations 表结构一致，按列读写，无 data BLOB。
//! 排序与 idx_conversations_sort 一致：is_archived → is_pinned DESC → last_message_at DESC。

use std::collections::HashMap;

use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

use crate::domain::{ConversationReader, ConversationWriter};
use crate::error::{ErrorCode, FlareError, Result};
use crate::infrastructure::persistence::ConversationStore;
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
            max_seq: max_seq.max(0) as u64,
            is_pinned: is_pinned != 0,
            is_muted: is_muted != 0,
            is_archived: is_archived != 0,
            version: version.max(0) as u64,
            updated_at: updated_at.max(0) as u64,
            created_at: created_at.max(0) as u64,
            updated_at_ts: updated_at_ts.map(|t| t as u64),
            ext: parse_ext(ext_json.as_deref()),
            draft,
            mention_count: mention_count.max(0) as u32,
            mention_me: mention_me != 0,
            badge,
            role,
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
            let ext_json = serde_json::to_string(&c.ext).unwrap_or_default();
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
            .bind(&c.conversation_id)
            .bind(conversation_type_to_i32(&c.conversation_type))
            .bind(&c.business_type)
            .bind(&c.channel_id)
            .bind(c.members_count as i64)
            .bind(&c.display_name)
            .bind(&c.avatar_url)
            .bind(&c.remark)
            .bind(&c.description)
            .bind(&c.last_message_id)
            .bind(&c.last_sender_id)
            .bind(c.last_message_at.map(|t| t as i64))
            .bind(&c.last_message_preview)
            .bind(&c.last_sender_nickname)
            .bind(&c.last_sender_avatar_url)
            .bind(c.unread_count as i32)
            .bind(c.last_read_seq as i64)
            .bind(c.max_seq as i64)
            .bind(if c.is_pinned { 1i32 } else { 0 })
            .bind(if c.is_muted { 1i32 } else { 0 })
            .bind(if c.is_archived { 1i32 } else { 0 })
            .bind(c.version as i64)
            .bind(c.updated_at as i64)
            .bind(c.created_at as i64)
            .bind(c.updated_at_ts.map(|t| t as i64))
            .bind(&ext_json)
            .bind(&c.draft)
            .bind(c.mention_count as i32)
            .bind(if c.mention_me { 1i32 } else { 0 })
            .bind(&c.badge)
            .bind(&c.role)
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
            r#"UPDATE conversations SET unread_count = ?, last_read_seq = ? WHERE conversation_id = ?"#,
        )
        .bind(unread_count as i32)
        .bind(last_read_seq as i64)
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
               last_message_id = ?, last_sender_id = ?, last_message_at = ?, last_message_preview = ?, max_seq = ?
               WHERE conversation_id = ?"#,
        )
        .bind(last_message_id)
        .bind(last_sender_id)
        .bind(last_message_at as i64)
        .bind(last_message_preview.unwrap_or(""))
        .bind(max_seq as i64)
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
}

// ---------- ConversationStore 适配（供 StoreProvider / 应用层使用）----------

#[async_trait]
impl ConversationStore for SqliteConversationRepo {
    async fn save_batch(&self, conversations: &[Conversation]) -> Result<()> {
        ConversationWriter::save_batch(self, conversations).await
    }

    async fn save_one(&self, conversation: &Conversation) -> Result<()> {
        ConversationWriter::save_one(self, conversation).await
    }

    async fn get(&self, conversation_id: &str) -> Result<Option<Conversation>> {
        ConversationReader::get(self, conversation_id).await
    }

    async fn list(&self) -> Result<Vec<Conversation>> {
        ConversationReader::list(self).await
    }

    async fn update_unread(
        &self,
        conversation_id: &str,
        unread_count: u32,
        last_read_seq: u64,
    ) -> Result<()> {
        ConversationWriter::update_unread(self, conversation_id, unread_count, last_read_seq).await
    }

    async fn set_pinned(&self, conversation_id: &str, pinned: bool) -> Result<()> {
        ConversationWriter::set_pinned(self, conversation_id, pinned).await
    }

    async fn set_muted(&self, conversation_id: &str, muted: bool) -> Result<()> {
        ConversationWriter::set_muted(self, conversation_id, muted).await
    }

    async fn set_archived(&self, conversation_id: &str, archived: bool) -> Result<()> {
        ConversationWriter::set_archived(self, conversation_id, archived).await
    }

    async fn update_draft(&self, conversation_id: &str, draft: Option<&str>) -> Result<()> {
        ConversationWriter::update_draft(self, conversation_id, draft).await
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
        ConversationWriter::update_last_message(
            self,
            conversation_id,
            last_message_id,
            last_sender_id,
            last_message_at,
            last_message_preview,
            max_seq,
        )
        .await
    }

    async fn delete(&self, conversation_id: &str) -> Result<()> {
        ConversationWriter::delete(self, conversation_id).await
    }
}
