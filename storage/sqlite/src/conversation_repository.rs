//! SQLite 会话仓储实现

use async_trait::async_trait;
use flare_im_core_sdk::domain::repository::{ConversationRepository, ConversationListResult};
use flare_im_core_sdk::domain::conversation::Conversation;
use sqlx::SqlitePool;
use std::sync::Arc;
use anyhow::Result;
use tracing::{debug, error, info};

/// SQLite 会话仓储实现
pub struct SqliteConversationRepository {
    pool: Arc<SqlitePool>,
}

impl SqliteConversationRepository {
    /// 创建新的会话仓储
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }
    
    /// 初始化数据库表
    pub async fn init(&self) -> Result<()> {
        // 为了确保表结构正确，总是删除并重新创建表
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='conversations'"
        )
        .fetch_one(&*self.pool)
        .await?;
        
        if count > 0 {
            sqlx::query("DROP TABLE conversations")
                .execute(&*self.pool)
                .await?;
            info!("Dropped existing conversations table");
        }
        
        sqlx::query(
            r#"
            CREATE TABLE conversations (
                conversation_id TEXT PRIMARY KEY,
                conversation_type TEXT NOT NULL,
                business_type TEXT,
                display_name TEXT NOT NULL,
                avatar_url TEXT,
                unread_count INTEGER NOT NULL DEFAULT 0,
                max_seq INTEGER NOT NULL DEFAULT 0,
                last_read_seq INTEGER NOT NULL DEFAULT 0,
                last_message TEXT,
                is_muted INTEGER NOT NULL DEFAULT 0,
                is_pinned INTEGER NOT NULL DEFAULT 0,
                is_muted_detail INTEGER NOT NULL DEFAULT 0,
                mute_until TEXT,
                visibility TEXT NOT NULL,
                lifecycle_state TEXT NOT NULL,
                attributes TEXT NOT NULL,
                participants TEXT NOT NULL,
                policy TEXT,
                presence TEXT,
                announcement TEXT,
                announcement_updated_at TEXT,
                announcement_updated_by TEXT,
                description TEXT,
                extended_config TEXT NOT NULL,
                ext TEXT NOT NULL,
                labels TEXT NOT NULL,
                draft TEXT,
                input_state TEXT,
                archived_at TEXT,
                deleted_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&*self.pool)
        .await?;
        
        info!("Conversation repository table created with correct schema");
        
        // 创建索引
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_conversations_updated_at ON conversations(updated_at DESC)"
        )
        .execute(&*self.pool)
        .await?;
        
        info!("Conversation repository tables initialized");
        Ok(())
    }
}

#[async_trait]
impl ConversationRepository for SqliteConversationRepository {
    async fn save(&self, conversation: &Conversation) -> Result<()> {
        let last_message_json = conversation.last_message.as_ref()
            .map(|m| serde_json::to_string(m))
            .transpose()?;
        let attributes_json = serde_json::to_string(&conversation.attributes)?;
        let participants_json = serde_json::to_string(&conversation.participants)?;
        let policy_json = conversation.policy.as_ref()
            .map(|p| serde_json::to_string(p))
            .transpose()?;
        let presence_json = conversation.presence.as_ref()
            .map(|p| serde_json::to_string(p))
            .transpose()?;
        let extended_config_json = serde_json::to_string(&conversation.extended_config)?;
        let ext_json = serde_json::to_string(&conversation.ext)?;
        let labels_json = serde_json::to_string(&conversation.labels)?;
        let draft_json = conversation.draft.as_ref()
            .map(|d| serde_json::to_string(d))
            .transpose()?;
        let input_state_json = conversation.input_state.as_ref()
            .map(|s| serde_json::to_string(s))
            .transpose()?;
        
        sqlx::query(
            r#"
            INSERT INTO conversations (
                conversation_id, conversation_type, business_type,
                display_name, avatar_url,
                unread_count, max_seq, last_read_seq, last_message,
                is_muted, is_pinned, is_muted_detail, mute_until,
                visibility, lifecycle_state,
                attributes, participants, policy, presence,
                announcement, announcement_updated_at, announcement_updated_by,
                description, extended_config, ext, labels,
                draft, input_state,
                archived_at, deleted_at,
                created_at, updated_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24,
                ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32
            )
            ON CONFLICT(conversation_id) DO UPDATE SET
                conversation_type = excluded.conversation_type,
                business_type = excluded.business_type,
                display_name = excluded.display_name,
                avatar_url = excluded.avatar_url,
                unread_count = excluded.unread_count,
                max_seq = excluded.max_seq,
                last_read_seq = excluded.last_read_seq,
                last_message = excluded.last_message,
                is_muted = excluded.is_muted,
                is_pinned = excluded.is_pinned,
                is_muted_detail = excluded.is_muted_detail,
                mute_until = excluded.mute_until,
                visibility = excluded.visibility,
                lifecycle_state = excluded.lifecycle_state,
                attributes = excluded.attributes,
                participants = excluded.participants,
                policy = excluded.policy,
                presence = excluded.presence,
                announcement = excluded.announcement,
                announcement_updated_at = excluded.announcement_updated_at,
                announcement_updated_by = excluded.announcement_updated_by,
                description = excluded.description,
                extended_config = excluded.extended_config,
                ext = excluded.ext,
                labels = excluded.labels,
                draft = excluded.draft,
                input_state = excluded.input_state,
                archived_at = excluded.archived_at,
                deleted_at = excluded.deleted_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&conversation.conversation_id)
        .bind(&conversation.conversation_type)
        .bind(conversation.business_type.as_ref())
        .bind(&conversation.display_name)
        .bind(conversation.avatar_url.as_ref())
        .bind(conversation.unread_count as i64)
        .bind(conversation.max_seq as i64)
        .bind(conversation.last_read_seq as i64)
        .bind(last_message_json.as_ref())
        .bind(if conversation.is_muted { 1 } else { 0 })
        .bind(if conversation.is_pinned { 1 } else { 0 })
        .bind(if conversation.is_muted_detail { 1 } else { 0 })
        .bind(conversation.mute_until.map(|t| t.to_rfc3339()))
        .bind(format!("{:?}", conversation.visibility))
        .bind(format!("{:?}", conversation.lifecycle_state))
        .bind(&attributes_json)
        .bind(&participants_json)
        .bind(policy_json.as_ref())
        .bind(presence_json.as_ref())
        .bind(conversation.announcement.as_ref())
        .bind(conversation.announcement_updated_at.map(|t| t.to_rfc3339()))
        .bind(conversation.announcement_updated_by.as_ref())
        .bind(conversation.description.as_ref())
        .bind(&extended_config_json)
        .bind(&ext_json)
        .bind(&labels_json)
        .bind(draft_json.as_ref())
        .bind(input_state_json.as_ref())
        .bind(<Option<String>>::None) // archived_at - 根据 lifecycle_state 设置
        .bind(<Option<String>>::None) // deleted_at - 根据 lifecycle_state 设置
        .bind(conversation.created_at.to_rfc3339())
        .bind(conversation.updated_at.to_rfc3339())
        .execute(&*self.pool)
        .await?;
        
        debug!(
            conversation_id = %conversation.conversation_id,
            "Conversation saved"
        );
        
        Ok(())
    }
    
    async fn update(&self, conversation: &Conversation) -> Result<()> {
        // 使用 INSERT ... ON CONFLICT 实现更新逻辑
        self.save(conversation).await
    }
    
    async fn find_by_id(&self, conversation_id: &str) -> Result<Option<Conversation>> {
        let row = sqlx::query_as::<_, ConversationRow>(
            "SELECT * FROM conversations WHERE conversation_id = ?1"
        )
        .bind(conversation_id)
        .fetch_optional(&*self.pool)
        .await?;
        
        if let Some(row) = row {
            Ok(Some(row.try_into()?))
        } else {
            Ok(None)
        }
    }
    
    async fn find_all(
        &self,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<ConversationListResult> {
        let limit = limit.unwrap_or(50);
        let limit_plus_one = limit + 1;
        
        let query = if let Some(cursor) = cursor {
            sqlx::query_as::<_, ConversationRow>(
                r#"
                SELECT * FROM conversations
                WHERE updated_at < ?1
                ORDER BY updated_at DESC
                LIMIT ?2
                "#,
            )
            .bind(cursor)
            .bind(limit_plus_one as i64)
        } else {
            sqlx::query_as::<_, ConversationRow>(
                r#"
                SELECT * FROM conversations
                ORDER BY updated_at DESC
                LIMIT ?1
                "#,
            )
            .bind(limit_plus_one as i64)
        };
        
        let rows = query.fetch_all(&*self.pool).await?;
        
        let has_more = rows.len() > limit;
        let conversations: Vec<Conversation> = rows
            .into_iter()
            .take(limit)
            .map(|row| row.try_into())
            .collect::<Result<Vec<_>>>()?;
        
        let next_cursor = if has_more && !conversations.is_empty() {
            Some(conversations.last().unwrap().updated_at.to_rfc3339())
        } else {
            None
        };
        
        Ok(ConversationListResult {
            conversations,
            next_cursor,
        })
    }
    
    async fn find_by_participant(
        &self,
        user_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<Conversation>> {
        let limit = limit.unwrap_or(50);
        
        // 从 participants JSON 中查找包含 user_id 的会话
        // SQLite 的 JSON 支持有限，这里使用 LIKE 查询（简单但不够精确）
        let user_id_pattern = format!("%\"{}\"%", user_id);
        
        let rows = sqlx::query_as::<_, ConversationRow>(
            r#"
            SELECT * FROM conversations
            WHERE participants LIKE ?1
            ORDER BY updated_at DESC
            LIMIT ?2
            "#,
        )
        .bind(&user_id_pattern)
        .bind(limit as i64)
        .fetch_all(&*self.pool)
        .await?;
        
        let conversations: Vec<Conversation> = rows
            .into_iter()
            .map(|row| row.try_into())
            .collect::<Result<Vec<_>>>()?;
        
        // 进一步过滤（确保精确匹配）
        let conversations: Vec<Conversation> = conversations
            .into_iter()
            .filter(|conv| {
                conv.participants.iter().any(|p| p.user_id == user_id)
            })
            .collect();
        
        Ok(conversations)
    }
    
    async fn delete(&self, conversation_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM conversations WHERE conversation_id = ?1")
            .bind(conversation_id)
            .execute(&*self.pool)
            .await?;
        
        info!(conversation_id, "Conversation deleted");
        Ok(())
    }
}

/// 数据库行结构
#[derive(sqlx::FromRow)]
struct ConversationRow {
    conversation_id: String,
    conversation_type: String,
    business_type: Option<String>,
    display_name: String,
    avatar_url: Option<String>,
    unread_count: i64,
    max_seq: i64,
    last_read_seq: i64,
    last_message: Option<String>,
    is_muted: i64,
    is_pinned: i64,
    is_muted_detail: i64,
    mute_until: Option<String>,
    visibility: String,
    lifecycle_state: String,
    attributes: String,
    participants: String,
    policy: Option<String>,
    presence: Option<String>,
    announcement: Option<String>,
    announcement_updated_at: Option<String>,
    announcement_updated_by: Option<String>,
    description: Option<String>,
    extended_config: String,
    ext: String,
    labels: String,
    draft: Option<String>,
    input_state: Option<String>,
    archived_at: Option<String>,
    deleted_at: Option<String>,
    created_at: String,
    updated_at: String,
}

impl TryInto<Conversation> for ConversationRow {
    type Error = anyhow::Error;
    
    fn try_into(self) -> Result<Conversation> {
        use flare_im_core_sdk::domain::conversation::*;
        
        let created_at = chrono::DateTime::parse_from_rfc3339(&self.created_at)?
            .with_timezone(&chrono::Utc);
        let updated_at = chrono::DateTime::parse_from_rfc3339(&self.updated_at)?
            .with_timezone(&chrono::Utc);
        
        let visibility = serde_json::from_str(&format!("\"{}\"", self.visibility))?;
        let lifecycle_state = serde_json::from_str(&format!("\"{}\"", self.lifecycle_state))?;
        
        let last_message = self.last_message.as_ref()
            .map(|m| serde_json::from_str(m))
            .transpose()?;
        let attributes: std::collections::HashMap<String, String> = serde_json::from_str(&self.attributes)?;
        let participants: Vec<ConversationParticipant> = serde_json::from_str(&self.participants)?;
        let policy = self.policy.as_ref()
            .map(|p| serde_json::from_str(p))
            .transpose()?;
        let presence = self.presence.as_ref()
            .map(|p| serde_json::from_str(p))
            .transpose()?;
        let extended_config: std::collections::HashMap<String, String> = serde_json::from_str(&self.extended_config)?;
        let ext: std::collections::HashMap<String, String> = serde_json::from_str(&self.ext)?;
        let labels: Vec<String> = serde_json::from_str(&self.labels)?;
        let draft = self.draft.as_ref()
            .map(|d| serde_json::from_str(d))
            .transpose()?;
        let input_state = self.input_state.as_ref()
            .map(|s| serde_json::from_str(s))
            .transpose()?;
        
        Ok(Conversation {
            conversation_id: self.conversation_id,
            conversation_type: self.conversation_type,
            business_type: self.business_type,
            display_name: self.display_name,
            avatar_url: self.avatar_url,
            unread_count: self.unread_count as u32,
            max_seq: self.max_seq as u64,
            last_read_seq: self.last_read_seq as u64,
            last_message,
            is_muted: self.is_muted != 0,
            is_pinned: self.is_pinned != 0,
            is_muted_detail: self.is_muted_detail != 0,
            mute_until: self.mute_until
                .map(|t| chrono::DateTime::parse_from_rfc3339(&t))
                .transpose()?
                .map(|t| t.with_timezone(&chrono::Utc)),
            visibility,
            lifecycle_state,
            attributes,
            participants,
            policy,
            presence,
            announcement: self.announcement,
            announcement_updated_at: self.announcement_updated_at
                .map(|t| chrono::DateTime::parse_from_rfc3339(&t))
                .transpose()?
                .map(|t| t.with_timezone(&chrono::Utc)),
            announcement_updated_by: self.announcement_updated_by,
            description: self.description,
            extended_config,
            ext,
            labels,
            draft,
            input_state,
            version: 0, // 从数据库读取时，版本号需要单独处理
            created_at,
            updated_at,
        })
    }
}
