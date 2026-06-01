use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

use crate::domain::ConversationParticipantStore;
use crate::error::{ErrorCode, FlareError, Result};
use crate::model::ConversationParticipant;

fn sqlx_err(e: sqlx::Error) -> FlareError {
    FlareError::localized(ErrorCode::DatabaseError, e.to_string())
}

fn parse_roles(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

fn parse_attributes(raw: &str) -> std::collections::HashMap<String, String> {
    serde_json::from_str(raw).unwrap_or_default()
}

pub struct SqliteConversationParticipantRepo {
    pool: SqlitePool,
}

impl SqliteConversationParticipantRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ConversationParticipantStore for SqliteConversationParticipantRepo {
    async fn save_page(
        &self,
        conversation_id: &str,
        participants: &[ConversationParticipant],
        participant_version: u64,
        replace_all: bool,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(sqlx_err)?;
        if replace_all {
            sqlx::query("DELETE FROM conversation_participants WHERE conversation_id = ?")
                .bind(conversation_id)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;
        }

        for p in participants {
            let roles = serde_json::to_string(&p.roles).unwrap_or_default();
            let attributes = serde_json::to_string(&p.attributes).unwrap_or_default();
            sqlx::query(
                r#"INSERT INTO conversation_participants (
                       conversation_id, user_id, roles, muted, pinned, attributes,
                       joined_at, nickname, participant_version, updated_at
                   ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                   ON CONFLICT(conversation_id, user_id) DO UPDATE SET
                       roles = excluded.roles,
                       muted = excluded.muted,
                       pinned = excluded.pinned,
                       attributes = excluded.attributes,
                       joined_at = excluded.joined_at,
                       nickname = excluded.nickname,
                       participant_version = excluded.participant_version,
                       updated_at = excluded.updated_at"#,
            )
            .bind(conversation_id)
            .bind(&p.user_id)
            .bind(roles)
            .bind(if p.muted { 1i32 } else { 0 })
            .bind(if p.pinned { 1i32 } else { 0 })
            .bind(attributes)
            .bind(p.joined_at as i64)
            .bind(&p.nickname)
            .bind(participant_version as i64)
            .bind(crate::util::id::now_millis() as i64)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }

        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn list(
        &self,
        conversation_id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<ConversationParticipant>> {
        let rows = sqlx::query(
            r#"SELECT user_id, roles, muted, pinned, attributes, joined_at, nickname
               FROM conversation_participants
               WHERE conversation_id = ?
               ORDER BY joined_at ASC, user_id ASC
               LIMIT ? OFFSET ?"#,
        )
        .bind(conversation_id)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                let roles: String = row.try_get("roles").map_err(sqlx_err)?;
                let attributes: String = row.try_get("attributes").map_err(sqlx_err)?;
                let joined_at: i64 = row.try_get("joined_at").map_err(sqlx_err)?;
                let muted: i32 = row.try_get("muted").map_err(sqlx_err)?;
                let pinned: i32 = row.try_get("pinned").map_err(sqlx_err)?;
                Ok(ConversationParticipant {
                    user_id: row.try_get("user_id").map_err(sqlx_err)?,
                    roles: parse_roles(&roles),
                    muted: muted != 0,
                    pinned: pinned != 0,
                    attributes: parse_attributes(&attributes),
                    joined_at: joined_at.max(0) as u64,
                    nickname: row.try_get("nickname").map_err(sqlx_err)?,
                })
            })
            .collect()
    }

    async fn version(&self, conversation_id: &str) -> Result<u64> {
        let value: Option<i64> = sqlx::query_scalar(
            r#"SELECT MAX(participant_version)
               FROM conversation_participants
               WHERE conversation_id = ?"#,
        )
        .bind(conversation_id)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(value.unwrap_or_default().max(0) as u64)
    }

    async fn patch_user_display(
        &self,
        user_id: &str,
        nickname: &str,
        avatar_url: &str,
    ) -> Result<()> {
        let rows = sqlx::query(
            r#"SELECT conversation_id, attributes
               FROM conversation_participants
               WHERE user_id = ?"#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let now = crate::util::id::now_millis() as i64;
        for row in rows {
            let conversation_id: String = row.try_get("conversation_id").map_err(sqlx_err)?;
            let attributes_raw: String = row.try_get("attributes").map_err(sqlx_err)?;
            let mut attributes = parse_attributes(&attributes_raw);
            if !avatar_url.trim().is_empty() {
                attributes.insert("avatar_url".to_string(), avatar_url.to_string());
            }
            attributes
                .entry("nickname".to_string())
                .or_insert_with(|| nickname.to_string());
            let attributes_json = serde_json::to_string(&attributes).unwrap_or_default();
            sqlx::query(
                r#"UPDATE conversation_participants
                   SET nickname = ?, attributes = ?, updated_at = ?
                   WHERE conversation_id = ? AND user_id = ?"#,
            )
            .bind(nickname)
            .bind(attributes_json)
            .bind(now)
            .bind(conversation_id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        }
        Ok(())
    }
}
