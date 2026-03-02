//! SQLite 消息仓储实现

use async_trait::async_trait;
use flare_im_core_sdk::domain::repository::{MessageRepository, MessageListResult};
use flare_im_core_sdk::domain::message::Message;
use sqlx::SqlitePool;
use std::sync::Arc;
use anyhow::Result;
use tracing::{debug, info};

/// SQLite 消息仓储实现
pub struct SqliteMessageRepository {
    pool: Arc<SqlitePool>,
}

impl SqliteMessageRepository {
    /// 创建新的消息仓储
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }
    
    /// 初始化数据库表
    pub async fn init(&self) -> Result<()> {
        // 为了确保表结构正确，总是删除并重新创建表
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='messages'"
        )
        .fetch_one(&*self.pool)
        .await?;
        
        if count > 0 {
            sqlx::query("DROP TABLE messages")
                .execute(&*self.pool)
                .await?;
            info!("Dropped existing messages table");
        }
        
        // 创建具有正确结构的新表
        sqlx::query(
            r#"
            CREATE TABLE messages (
                server_id TEXT,
                client_msg_id TEXT PRIMARY KEY,
                conversation_id TEXT,
                sender_id TEXT NOT NULL,
                source TEXT NOT NULL,
                seq INTEGER,
                timestamp TEXT NOT NULL,
                conversation_type TEXT NOT NULL,
                message_type TEXT NOT NULL,
                business_type TEXT,
                receiver_id TEXT,
                channel_id TEXT,
                content BLOB NOT NULL,
                content_type TEXT NOT NULL,
                attachments TEXT NOT NULL,
                quote TEXT,
                extra TEXT NOT NULL,
                attributes TEXT NOT NULL,
                state TEXT NOT NULL,
                is_recalled INTEGER NOT NULL DEFAULT 0,
                recalled_at TEXT,
                recall_reason TEXT,
                is_burn_after_read INTEGER NOT NULL DEFAULT 0,
                burn_after_seconds INTEGER,
                timeline TEXT NOT NULL,
                visibility TEXT NOT NULL,
                read_by TEXT NOT NULL,
                reactions TEXT NOT NULL,
                edit_history TEXT NOT NULL,
                audit TEXT,
                tags TEXT NOT NULL,
                offline_push_info TEXT,
                version INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&*self.pool)
        .await?;
        
        info!("Message repository table created with correct schema");
        
        // 创建索引
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_messages_server_id ON messages(server_id)"
        )
        .execute(&*self.pool)
        .await?;
        
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_messages_conversation_id ON messages(conversation_id)"
        )
        .execute(&*self.pool)
        .await?;
        
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON messages(timestamp DESC)"
        )
        .execute(&*self.pool)
        .await?;
        
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_messages_conversation_timestamp ON messages(conversation_id, timestamp DESC)"
        )
        .execute(&*self.pool)
        .await?;
        
        info!("Message repository tables initialized");
        Ok(())
    }
}

#[async_trait]
impl MessageRepository for SqliteMessageRepository {
    async fn save(&self, message: &Message) -> Result<()> {
        let attachments_json = serde_json::to_string(&message.attachments)?;
        let quote_json = message.quote.as_ref()
            .map(|q| serde_json::to_string(q))
            .transpose()?;
        let extra_json = serde_json::to_string(&message.extra)?;
        let attributes_json = serde_json::to_string(&message.attributes)?;
        let timeline_json = serde_json::to_string(&message.timeline)?;
        let visibility_json = serde_json::to_string(&message.visibility)?;
        let read_by_json = serde_json::to_string(&message.read_by)?;
        let reactions_json = serde_json::to_string(&message.reactions)?;
        let edit_history_json = serde_json::to_string(&message.edit_history)?;
        let audit_json = message.audit.as_ref()
            .map(|a| serde_json::to_string(a))
            .transpose()?;
        let tags_json = serde_json::to_string(&message.tags)?;
        let offline_push_info_json = message.offline_push_info.as_ref()
            .map(|i| serde_json::to_string(i))
            .transpose()?;
        
        sqlx::query(
            r#"
            INSERT INTO messages (
                server_id, client_msg_id, conversation_id, sender_id, source, seq,
                timestamp, conversation_type, message_type, business_type, receiver_id, channel_id,
                content, content_type, attachments, quote, extra, attributes,
                state, is_recalled, recalled_at, recall_reason,
                is_burn_after_read, burn_after_seconds,
                timeline, visibility, read_by, reactions, edit_history,
                audit, tags, offline_push_info, version,
                created_at, updated_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22,
                ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32,
                ?33, ?34, ?35
            )
            ON CONFLICT(client_msg_id) DO UPDATE SET
                server_id = excluded.server_id,
                conversation_id = excluded.conversation_id,
                sender_id = excluded.sender_id,
                source = excluded.source,
                seq = excluded.seq,
                timestamp = excluded.timestamp,
                conversation_type = excluded.conversation_type,
                message_type = excluded.message_type,
                business_type = excluded.business_type,
                receiver_id = excluded.receiver_id,
                channel_id = excluded.channel_id,
                content = excluded.content,
                content_type = excluded.content_type,
                attachments = excluded.attachments,
                quote = excluded.quote,
                extra = excluded.extra,
                attributes = excluded.attributes,
                state = excluded.state,
                is_recalled = excluded.is_recalled,
                recalled_at = excluded.recalled_at,
                recall_reason = excluded.recall_reason,
                is_burn_after_read = excluded.is_burn_after_read,
                burn_after_seconds = excluded.burn_after_seconds,
                timeline = excluded.timeline,
                visibility = excluded.visibility,
                read_by = excluded.read_by,
                reactions = excluded.reactions,
                edit_history = excluded.edit_history,
                audit = excluded.audit,
                tags = excluded.tags,
                offline_push_info = excluded.offline_push_info,
                version = excluded.version,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(message.server_id.as_ref())
        .bind(&message.client_msg_id)
        .bind(message.conversation_id.as_ref())
        .bind(&message.sender_id)
        .bind(format!("{:?}", message.source))
        .bind(message.seq.map(|s| s as i64))
        .bind(message.timestamp.to_rfc3339())
        .bind(format!("{:?}", message.conversation_type))
        .bind(format!("{:?}", message.message_type))
        .bind(message.business_type.as_ref())
        .bind(message.receiver_id.as_ref())
        .bind(message.channel_id.as_ref())
        .bind(&message.content)
        .bind(format!("{:?}", message.content_type))
        .bind(&attachments_json)
        .bind(quote_json.as_ref())
        .bind(&extra_json)
        .bind(&attributes_json)
        .bind(format!("{:?}", message.state))
        .bind(if message.is_recalled { 1 } else { 0 })
        .bind(message.recalled_at.map(|t| t.to_rfc3339()))
        .bind(message.recall_reason.as_ref())
        .bind(if message.is_burn_after_read { 1 } else { 0 })
        .bind(message.burn_after_seconds.map(|s| s as i64))
        .bind(&timeline_json)
        .bind(&visibility_json)
        .bind(&read_by_json)
        .bind(&reactions_json)
        .bind(&edit_history_json)
        .bind(audit_json.as_ref())
        .bind(&tags_json)
        .bind(offline_push_info_json.as_ref())
        .bind(message.version as i64)
        .bind(message.created_at.to_rfc3339())
        .bind(message.updated_at.to_rfc3339())
        .execute(&*self.pool)
        .await?;
        
        debug!(
            client_msg_id = %message.client_msg_id,
            conversation_id = message.conversation_id.as_ref().map(|s| s.as_str()).unwrap_or("<none>"),
            "Message saved"
        );
        
        Ok(())
    }
    
    async fn save_batch(&self, messages: &[Message]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        
        for message in messages {
            // 复用 save 逻辑（使用 INSERT ... ON CONFLICT），但使用事务
            let attachments_json = serde_json::to_string(&message.attachments)?;
            let quote_json = message.quote.as_ref()
                .map(|q| serde_json::to_string(q))
                .transpose()?;
            let extra_json = serde_json::to_string(&message.extra)?;
            let attributes_json = serde_json::to_string(&message.attributes)?;
            let timeline_json = serde_json::to_string(&message.timeline)?;
            let visibility_json = serde_json::to_string(&message.visibility)?;
            let read_by_json = serde_json::to_string(&message.read_by)?;
            let reactions_json = serde_json::to_string(&message.reactions)?;
            let edit_history_json = serde_json::to_string(&message.edit_history)?;
            let audit_json = message.audit.as_ref()
                .map(|a| serde_json::to_string(a))
                .transpose()?;
            let tags_json = serde_json::to_string(&message.tags)?;
            let offline_push_info_json = message.offline_push_info.as_ref()
                .map(|i| serde_json::to_string(i))
                .transpose()?;
            
            sqlx::query(
                r#"
                INSERT INTO messages (
                    server_id, client_msg_id, conversation_id, sender_id, source, seq,
                    timestamp, conversation_type, message_type, business_type, receiver_id, channel_id,
                    content, content_type, attachments, quote, extra, attributes,
                    state, is_recalled, recalled_at, recall_reason,
                    is_burn_after_read, burn_after_seconds,
                    timeline, visibility, read_by, reactions, edit_history,
                    audit, tags, offline_push_info, version,
                    created_at, updated_at
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                    ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22,
                    ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32,
                    ?33, ?34, ?35
                )
                ON CONFLICT(client_msg_id) DO UPDATE SET
                    server_id = excluded.server_id,
                    conversation_id = excluded.conversation_id,
                    sender_id = excluded.sender_id,
                    source = excluded.source,
                    seq = excluded.seq,
                    timestamp = excluded.timestamp,
                    conversation_type = excluded.conversation_type,
                    message_type = excluded.message_type,
                    business_type = excluded.business_type,
                    receiver_id = excluded.receiver_id,
                    channel_id = excluded.channel_id,
                    content = excluded.content,
                    content_type = excluded.content_type,
                    attachments = excluded.attachments,
                    quote = excluded.quote,
                    extra = excluded.extra,
                    attributes = excluded.attributes,
                    state = excluded.state,
                    is_recalled = excluded.is_recalled,
                    recalled_at = excluded.recalled_at,
                    recall_reason = excluded.recall_reason,
                    is_burn_after_read = excluded.is_burn_after_read,
                    burn_after_seconds = excluded.burn_after_seconds,
                    timeline = excluded.timeline,
                    visibility = excluded.visibility,
                    read_by = excluded.read_by,
                    reactions = excluded.reactions,
                    edit_history = excluded.edit_history,
                    audit = excluded.audit,
                    tags = excluded.tags,
                    offline_push_info = excluded.offline_push_info,
                    version = excluded.version,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(message.server_id.as_ref())
            .bind(&message.client_msg_id)
            .bind(message.conversation_id.as_ref())
            .bind(&message.sender_id)
            .bind(format!("{:?}", message.source))
            .bind(message.seq.map(|s| s as i64))
            .bind(message.timestamp.to_rfc3339())
            .bind(format!("{:?}", message.conversation_type))
            .bind(format!("{:?}", message.message_type))
            .bind(message.business_type.as_ref())
            .bind(message.receiver_id.as_ref())
            .bind(message.channel_id.as_ref())
            .bind(&message.content)
            .bind(format!("{:?}", message.content_type))
            .bind(&attachments_json)
            .bind(quote_json.as_ref())
            .bind(&extra_json)
            .bind(&attributes_json)
            .bind(format!("{:?}", message.state))
            .bind(if message.is_recalled { 1 } else { 0 })
            .bind(message.recalled_at.map(|t| t.to_rfc3339()))
            .bind(message.recall_reason.as_ref())
            .bind(if message.is_burn_after_read { 1 } else { 0 })
            .bind(message.burn_after_seconds.map(|s| s as i64))
            .bind(&timeline_json)
            .bind(&visibility_json)
            .bind(&read_by_json)
            .bind(&reactions_json)
            .bind(&edit_history_json)
            .bind(audit_json.as_ref())
            .bind(&tags_json)
            .bind(offline_push_info_json.as_ref())
            .bind(message.version as i64)
            .bind(message.created_at.to_rfc3339())
            .bind(message.updated_at.to_rfc3339())
            .execute(&mut *tx)
            .await?;
        }
        
        tx.commit().await?;
        
        info!(
            count = messages.len(),
            "Messages saved in batch"
        );
        
        Ok(())
    }
    
    async fn find_by_id(&self, message_id: &str) -> Result<Option<Message>> {
        // 先尝试用 client_msg_id 查找
        let row = sqlx::query_as::<_, MessageRow>(
            "SELECT * FROM messages WHERE client_msg_id = ?1"
        )
        .bind(message_id)
        .fetch_optional(&*self.pool)
        .await?;
        
        if let Some(msg) = row {
            return Ok(Some(msg.try_into()?));
        }
        
        // 再尝试用 server_id 查找
        let row = sqlx::query_as::<_, MessageRow>(
            "SELECT * FROM messages WHERE server_id = ?1"
        )
        .bind(message_id)
        .fetch_optional(&*self.pool)
        .await?;
        
        if let Some(msg) = row {
            return Ok(Some(msg.try_into()?));
        }
        
        Ok(None)
    }
    
    async fn find_by_conversation(
        &self,
        conversation_id: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<MessageListResult> {
        let limit = limit.unwrap_or(50);
        let limit_plus_one = limit + 1;
        
        let query = if let Some(cursor) = cursor {
            // 使用游标分页（基于 timestamp）
            sqlx::query_as::<_, MessageRow>(
                r#"
                SELECT * FROM messages
                WHERE conversation_id = ?1 AND timestamp < ?2
                ORDER BY timestamp DESC
                LIMIT ?3
                "#,
            )
            .bind(conversation_id)
            .bind(cursor)
            .bind(limit_plus_one as i64)
        } else {
            // 第一页
            sqlx::query_as::<_, MessageRow>(
                r#"
                SELECT * FROM messages
                WHERE conversation_id = ?1
                ORDER BY timestamp DESC
                LIMIT ?2
                "#,
            )
            .bind(conversation_id)
            .bind(limit_plus_one as i64)
        };
        
        let rows = query.fetch_all(&*self.pool).await?;
        
        let has_more = rows.len() > limit;
        let messages: Vec<Message> = rows
            .into_iter()
            .take(limit)
            .map(|row| row.try_into())
            .collect::<Result<Vec<_>>>()?;
        
        let next_cursor = if has_more && !messages.is_empty() {
            Some(messages.last().unwrap().timestamp.to_rfc3339())
        } else {
            None
        };
        
        Ok(MessageListResult {
            messages,
            next_cursor,
        })
    }
    
    async fn search(
        &self,
        conversation_id: Option<&str>,
        keyword: &str,
        limit: Option<usize>,
    ) -> Result<Vec<Message>> {
        let limit = limit.unwrap_or(50);
        let keyword_pattern = format!("%{}%", keyword);
        
        let query = if let Some(conv_id) = conversation_id {
            sqlx::query_as::<_, MessageRow>(
                r#"
                SELECT * FROM messages
                WHERE conversation_id = ?1 AND content LIKE ?2
                ORDER BY timestamp DESC
                LIMIT ?3
                "#,
            )
            .bind(conv_id)
            .bind(&keyword_pattern)
            .bind(limit as i64)
        } else {
            sqlx::query_as::<_, MessageRow>(
                r#"
                SELECT * FROM messages
                WHERE content LIKE ?1
                ORDER BY timestamp DESC
                LIMIT ?2
                "#,
            )
            .bind(&keyword_pattern)
            .bind(limit as i64)
        };
        
        let rows = query.fetch_all(&*self.pool).await?;
        
        let messages: Vec<Message> = rows
            .into_iter()
            .map(|row| row.try_into())
            .collect::<Result<Vec<_>>>()?;
        
        Ok(messages)
    }
    
    async fn find_by_time_range(
        &self,
        conversation_id: Option<&str>,
        start_time: Option<chrono::DateTime<chrono::Utc>>,
        end_time: Option<chrono::DateTime<chrono::Utc>>,
        limit: Option<usize>,
    ) -> Result<Vec<Message>> {
        let limit = limit.unwrap_or(50);
        
        let query = if let Some(conv_id) = conversation_id {
            if let (Some(start), Some(end)) = (start_time, end_time) {
                sqlx::query_as::<_, MessageRow>(
                    r#"
                    SELECT * FROM messages
                    WHERE conversation_id = ?1 AND timestamp >= ?2 AND timestamp <= ?3
                    ORDER BY timestamp DESC
                    LIMIT ?4
                    "#,
                )
                .bind(conv_id)
                .bind(start.to_rfc3339())
                .bind(end.to_rfc3339())
                .bind(limit as i64)
            } else if let Some(start) = start_time {
                sqlx::query_as::<_, MessageRow>(
                    r#"
                    SELECT * FROM messages
                    WHERE conversation_id = ?1 AND timestamp >= ?2
                    ORDER BY timestamp DESC
                    LIMIT ?3
                    "#,
                )
                .bind(conv_id)
                .bind(start.to_rfc3339())
                .bind(limit as i64)
            } else if let Some(end) = end_time {
                sqlx::query_as::<_, MessageRow>(
                    r#"
                    SELECT * FROM messages
                    WHERE conversation_id = ?1 AND timestamp <= ?2
                    ORDER BY timestamp DESC
                    LIMIT ?3
                    "#,
                )
                .bind(conv_id)
                .bind(end.to_rfc3339())
                .bind(limit as i64)
            } else {
                sqlx::query_as::<_, MessageRow>(
                    r#"
                    SELECT * FROM messages
                    WHERE conversation_id = ?1
                    ORDER BY timestamp DESC
                    LIMIT ?2
                    "#,
                )
                .bind(conv_id)
                .bind(limit as i64)
            }
        } else {
            if let (Some(start), Some(end)) = (start_time, end_time) {
                sqlx::query_as::<_, MessageRow>(
                    r#"
                    SELECT * FROM messages
                    WHERE timestamp >= ?1 AND timestamp <= ?2
                    ORDER BY timestamp DESC
                    LIMIT ?3
                    "#,
                )
                .bind(start.to_rfc3339())
                .bind(end.to_rfc3339())
                .bind(limit as i64)
            } else if let Some(start) = start_time {
                sqlx::query_as::<_, MessageRow>(
                    r#"
                    SELECT * FROM messages
                    WHERE timestamp >= ?1
                    ORDER BY timestamp DESC
                    LIMIT ?2
                    "#,
                )
                .bind(start.to_rfc3339())
                .bind(limit as i64)
            } else if let Some(end) = end_time {
                sqlx::query_as::<_, MessageRow>(
                    r#"
                    SELECT * FROM messages
                    WHERE timestamp <= ?1
                    ORDER BY timestamp DESC
                    LIMIT ?2
                    "#,
                )
                .bind(end.to_rfc3339())
                .bind(limit as i64)
            } else {
                sqlx::query_as::<_, MessageRow>(
                    r#"
                    SELECT * FROM messages
                    ORDER BY timestamp DESC
                    LIMIT ?1
                    "#,
                )
                .bind(limit as i64)
            }
        };
        
        let rows = query.fetch_all(&*self.pool).await?;
        
        let messages: Vec<Message> = rows
            .into_iter()
            .map(|row| row.try_into())
            .collect::<Result<Vec<_>>>()?;
        
        Ok(messages)
    }
    
    async fn delete(&self, message_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM messages WHERE client_msg_id = ?1 OR server_id = ?1")
            .bind(message_id)
            .execute(&*self.pool)
            .await?;
        
        debug!(message_id, "Message deleted");
        Ok(())
    }
    
    async fn delete_by_conversation(&self, conversation_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM messages WHERE conversation_id = ?1")
            .bind(conversation_id)
            .execute(&*self.pool)
            .await?;
        
        info!(conversation_id, "All messages in conversation deleted");
        Ok(())
    }
}

/// 数据库行结构
#[derive(sqlx::FromRow)]
struct MessageRow {
    server_id: Option<String>,
    client_msg_id: String,
    conversation_id: Option<String>,
    sender_id: String,
    source: String,
    seq: Option<i64>,
    timestamp: String,
    conversation_type: String,
    message_type: String,
    business_type: Option<String>,
    receiver_id: Option<String>,
    channel_id: Option<String>,
    content: Vec<u8>,
    content_type: String,
    attachments: String,
    quote: Option<String>,
    extra: String,
    attributes: String,
    state: String,
    is_recalled: i64,
    recalled_at: Option<String>,
    recall_reason: Option<String>,
    is_burn_after_read: i64,
    burn_after_seconds: Option<i64>,
    timeline: String,
    visibility: String,
    read_by: String,
    reactions: String,
    edit_history: String,
    audit: Option<String>,
    tags: String,
    offline_push_info: Option<String>,
    version: i64,
    created_at: String,
    updated_at: String,
}

impl TryInto<Message> for MessageRow {
    type Error = anyhow::Error;
    
    fn try_into(self) -> Result<Message> {
        use flare_im_core_sdk::domain::message::*;
        
        let timestamp = chrono::DateTime::parse_from_rfc3339(&self.timestamp)?
            .with_timezone(&chrono::Utc);
        let created_at = chrono::DateTime::parse_from_rfc3339(&self.created_at)?
            .with_timezone(&chrono::Utc);
        let updated_at = chrono::DateTime::parse_from_rfc3339(&self.updated_at)?
            .with_timezone(&chrono::Utc);
        
        let source = match self.source.as_str() {
            "User" => MessageSource::User,
            "System" => MessageSource::System,
            "Bot" => MessageSource::Bot,
            _ => MessageSource::User,
        };
        
        let conversation_type = serde_json::from_str(&format!("\"{}\"", self.conversation_type))?;
        let message_type = serde_json::from_str(&format!("\"{}\"", self.message_type))?;
        let content_type = serde_json::from_str(&format!("\"{}\"", self.content_type))?;
        let state = serde_json::from_str(&format!("\"{}\"", self.state))?;
        
        let attachments: Vec<MediaAttachment> = serde_json::from_str(&self.attachments)?;
        let quote = self.quote.as_ref()
            .map(|q| serde_json::from_str(q))
            .transpose()?;
        let extra: std::collections::HashMap<String, String> = serde_json::from_str(&self.extra)?;
        let attributes: std::collections::HashMap<String, String> = serde_json::from_str(&self.attributes)?;
        let timeline: MessageTimeline = serde_json::from_str(&self.timeline)?;
        let visibility: std::collections::HashMap<String, VisibilityStatus> = serde_json::from_str(&self.visibility)?;
        let read_by: Vec<MessageReadRecord> = serde_json::from_str(&self.read_by)?;
        let reactions: Vec<Reaction> = serde_json::from_str(&self.reactions)?;
        let edit_history: Vec<EditHistory> = serde_json::from_str(&self.edit_history)?;
        let audit = self.audit.as_ref()
            .map(|a| serde_json::from_str(a))
            .transpose()?;
        let tags: Vec<String> = serde_json::from_str(&self.tags)?;
        let offline_push_info = self.offline_push_info.as_ref()
            .map(|i| serde_json::from_str(i))
            .transpose()?;
        
        Ok(Message {
            server_id: self.server_id,
            conversation_id: self.conversation_id,
            client_msg_id: self.client_msg_id,
            sender_id: self.sender_id,
            source,
            seq: self.seq.map(|s| s as u64),
            timestamp,
            conversation_type,
            message_type,
            business_type: self.business_type,
            receiver_id: self.receiver_id,
            channel_id: self.channel_id,
            content: self.content,
            content_type,
            attachments,
            quote,
            extra,
            attributes,
            state,
            is_recalled: self.is_recalled != 0,
            recalled_at: self.recalled_at
                .map(|t| chrono::DateTime::parse_from_rfc3339(&t))
                .transpose()?
                .map(|t| t.with_timezone(&chrono::Utc)),
            recall_reason: self.recall_reason,
            is_burn_after_read: self.is_burn_after_read != 0,
            burn_after_seconds: self.burn_after_seconds.map(|s| s as i32),
            timeline,
            visibility,
            read_by,
            reactions,
            edit_history,
            audit,
            tags,
            offline_push_info,
            version: self.version as u64,
            created_at,
            updated_at,
        })
    }
}
