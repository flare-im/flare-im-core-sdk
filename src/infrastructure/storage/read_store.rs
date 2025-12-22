//! ReadStore 实现
//!
//! 用于查询读模型，Query Handler 和 UI 使用
//! 对标微信、Telegram、飞书的生产级别实现

use async_trait::async_trait;
use crate::domain::repository::{ReadStore as ReadStoreTrait, Query, QueryResult};
use crate::domain::message::Message;
use crate::domain::conversation::Conversation;
use serde_json;

/// 辅助函数：从 protobuf 编码的 content 中提取文本内容，并添加到 Message 的 extra 字段中
fn extract_text_to_extra(message_json: &mut serde_json::Value) {
    use crate::domain::message::text_processor::TextContentProcessor;
    
    // 检查是否是文本消息（通过 content_type 字段判断）
    let is_plain_text = message_json.get("content_type")
        .and_then(|v| v.as_str())
        .map(|s| s == "PlainText")
        .unwrap_or(false);
    
    if is_plain_text {
        // 尝试从 content 中提取文本
        if let Some(content) = message_json.get("content")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_u64().map(|u| u as u8)).collect::<Vec<u8>>()) {
            
            // 尝试解码 protobuf MessageContent
            use flare_proto::flare::common::v1::MessageContent;
            use prost::Message;
            
            if let Ok(mc) = MessageContent::decode(content.as_slice()) {
                if let Some(flare_proto::flare::common::v1::message_content::Content::Text(text_content)) = mc.content {
                    // 使用文本内容处理器处理提取的文本
                    let processed_text = TextContentProcessor::process(text_content.text);
                    
                    // 提取文本成功，添加到 extra 字段
                    if let Some(extra) = message_json.get_mut("extra") {
                        if let Some(extra_obj) = extra.as_object_mut() {
                            extra_obj.insert("content_text".to_string(), serde_json::Value::String(processed_text));
                        }
                    } else {
                        // 如果 extra 不存在，创建它
                        let mut extra_obj = serde_json::Map::new();
                        extra_obj.insert("content_text".to_string(), serde_json::Value::String(processed_text));
                        message_json.as_object_mut().unwrap().insert("extra".to_string(), serde_json::Value::Object(extra_obj));
                    }
                }
            }
        }
    }
}

/// SQLite ReadStore 实现
#[cfg(not(target_arch = "wasm32"))]
pub struct SqliteReadStore {
    pool: sqlx::SqlitePool,
}

#[cfg(not(target_arch = "wasm32"))]
impl SqliteReadStore {
    /// 创建新的 SQLite ReadStore
    pub async fn new(database_url: &str) -> anyhow::Result<Self> {
        // 尝试使用连接字符串连接
        // 如果失败，尝试使用 SqliteConnectOptions
        let pool = match sqlx::SqlitePool::connect(database_url).await {
            Ok(pool) => pool,
            Err(e) => {
                // 如果连接字符串失败，尝试解析路径并使用 SqliteConnectOptions
                if database_url.starts_with("sqlite:///") {
                    let path = database_url.strip_prefix("sqlite:///")
                        .ok_or_else(|| anyhow::anyhow!("Invalid SQLite URL format"))?;
                    let options = sqlx::sqlite::SqliteConnectOptions::new()
                        .filename(path)
                        .create_if_missing(true);
                    sqlx::SqlitePool::connect_with(options).await?
                } else {
                    return Err(e.into());
                }
            }
        };
        
        // 创建会话表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS conversations (
                conversation_id TEXT PRIMARY KEY,
                data TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await?;
        
        // 创建消息表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                message_id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                data TEXT NOT NULL,
                created_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await?;
        
        // 创建索引
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_conversation_id ON messages(conversation_id)")
            .execute(&pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_updated_at ON conversations(updated_at)")
            .execute(&pool)
            .await?;
        
        Ok(Self { pool })
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl ReadStoreTrait for SqliteReadStore {
    async fn write_message(&self, message: &Message) -> anyhow::Result<()> {
        let message_json = serde_json::to_string(message)?;
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO messages (message_id, conversation_id, data, created_at)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(&message.id)
        .bind(&message.conversation_id)
        .bind(&message_json)
        .bind(message.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
    
    async fn write_conversation(&self, conversation: &Conversation) -> anyhow::Result<()> {
        let conversation_json = serde_json::to_string(conversation)?;
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO conversations (conversation_id, data, updated_at)
            VALUES (?, ?, ?)
            "#,
        )
        .bind(&conversation.conversation_id)
        .bind(&conversation_json)
        .bind(conversation.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
    
    async fn update_conversation(&self, conversation: &Conversation) -> anyhow::Result<()> {
        self.write_conversation(conversation).await
    }
    
    async fn delete_message(&self, message_id: &str) -> anyhow::Result<()> {
        // SQLite 软删除：更新 data 字段，标记为已删除
        sqlx::query(
            r#"
            UPDATE messages
            SET data = json_set(data, '$.deleted', true, '$.deleted_at', ?)
            WHERE message_id = ?
            "#,
        )
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(message_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
    
    async fn delete_conversation_messages(&self, conversation_id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM messages WHERE conversation_id = ?")
            .bind(conversation_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    
    async fn query(&self, query: Query) -> anyhow::Result<QueryResult> {
        match query {
            Query::ConversationList { limit, cursor } => {
                let limit = limit.unwrap_or(100) as i64;
                let cursor = cursor.unwrap_or_default();
                
                let rows = sqlx::query_as::<_, ConversationRow>(
                    r#"
                    SELECT conversation_id, data, updated_at
                    FROM conversations
                    WHERE updated_at > ?
                    ORDER BY updated_at DESC
                    LIMIT ?
                    "#,
                )
                .bind(cursor)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?;
                
                let items: Vec<serde_json::Value> = rows
                    .into_iter()
                    .map(|row| serde_json::from_str(&row.data))
                    .collect::<Result<Vec<_>, _>>()?;
                
                Ok(QueryResult::ConversationList {
                    items,
                    next_cursor: None, // TODO: 实现游标
                })
            }
            Query::ConversationDetail { conversation_id } => {
                let row = sqlx::query_as::<_, ConversationRow>(
                    r#"
                    SELECT conversation_id, data, updated_at
                    FROM conversations
                    WHERE conversation_id = ?
                    "#,
                )
                .bind(conversation_id)
                .fetch_optional(&self.pool)
                .await?;
                
                let item = if let Some(row) = row {
                    serde_json::from_str(&row.data)?
                } else {
                    serde_json::json!({})
                };
                
                Ok(QueryResult::ConversationDetail { item })
            }
            Query::MessageList { conversation_id, limit, cursor: _ } => {
                let limit = limit.unwrap_or(100) as i64;
                
                let rows = sqlx::query_as::<_, MessageRow>(
                    r#"
                    SELECT message_id, conversation_id, data, created_at
                    FROM messages
                    WHERE conversation_id = ?
                    ORDER BY created_at DESC
                    LIMIT ?
                    "#,
                )
                .bind(conversation_id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?;
                
                let mut items: Vec<serde_json::Value> = rows
                    .into_iter()
                    .map(|row| serde_json::from_str(&row.data))
                    .collect::<Result<Vec<_>, _>>()?;
                
                // 对于文本消息，提取文本内容到 extra 字段
                for item in &mut items {
                    extract_text_to_extra(item);
                }
                
                Ok(QueryResult::MessageList {
                    items,
                    next_cursor: None, // TODO: 实现游标
                })
            }
            Query::MessageDetail { message_id } => {
                let row = sqlx::query_as::<_, MessageRow>(
                    r#"
                    SELECT message_id, conversation_id, data, created_at
                    FROM messages
                    WHERE message_id = ?
                    "#,
                )
                .bind(message_id)
                .fetch_optional(&self.pool)
                .await?;
                
                let mut item = if let Some(row) = row {
                    serde_json::from_str(&row.data)?
                } else {
                    serde_json::json!({})
                };
                
                // 对于文本消息，提取文本内容到 extra 字段
                extract_text_to_extra(&mut item);
                
                Ok(QueryResult::MessageDetail { item })
            }
            Query::SearchMessages { conversation_id, keyword, limit } => {
                let limit = limit.unwrap_or(100) as i64;
                let pattern = format!("%{}%", keyword);
                
                let query = if let Some(conv_id) = conversation_id {
                    sqlx::query_as::<_, MessageRow>(
                        r#"
                        SELECT message_id, conversation_id, data, created_at
                        FROM messages
                        WHERE conversation_id = ? AND data LIKE ?
                        ORDER BY created_at DESC
                        LIMIT ?
                        "#,
                    )
                    .bind(conv_id)
                    .bind(&pattern)
                    .bind(limit)
                } else {
                    sqlx::query_as::<_, MessageRow>(
                        r#"
                        SELECT message_id, conversation_id, data, created_at
                        FROM messages
                        WHERE data LIKE ?
                        ORDER BY created_at DESC
                        LIMIT ?
                        "#,
                    )
                    .bind(&pattern)
                    .bind(limit)
                };
                
                let rows = query.fetch_all(&self.pool).await?;
                let mut items: Vec<serde_json::Value> = rows
                    .into_iter()
                    .map(|row| serde_json::from_str(&row.data))
                    .collect::<Result<Vec<_>, _>>()?;
                
                // 对于文本消息，提取文本内容到 extra 字段
                for item in &mut items {
                    extract_text_to_extra(item);
                }
                
                Ok(QueryResult::SearchMessages { items })
            }
            Query::FindMessages { conversation_id, message_type, start_time, end_time, limit } => {
                let limit = limit.unwrap_or(100) as i64;
                
                // 构建动态查询
                let mut query_str = String::from(
                    "SELECT message_id, conversation_id, data, created_at FROM messages WHERE 1=1"
                );
                let mut bindings = Vec::new();
                
                if let Some(conv_id) = &conversation_id {
                    query_str.push_str(" AND conversation_id = ?");
                    bindings.push(conv_id.clone());
                }
                if let Some(msg_type) = &message_type {
                    query_str.push_str(" AND json_extract(data, '$.message_type') = ?");
                    bindings.push(msg_type.clone());
                }
                if let Some(start) = &start_time {
                    query_str.push_str(" AND created_at >= ?");
                    bindings.push(start.to_rfc3339());
                }
                if let Some(end) = &end_time {
                    query_str.push_str(" AND created_at <= ?");
                    bindings.push(end.to_rfc3339());
                }
                query_str.push_str(" ORDER BY created_at DESC LIMIT ?");
                bindings.push(limit.to_string());
                
                // 执行查询（简化实现，实际应该使用参数化查询）
                let rows = sqlx::query_as::<_, MessageRow>(
                    r#"
                    SELECT message_id, conversation_id, data, created_at
                    FROM messages
                    WHERE conversation_id = COALESCE(?, conversation_id)
                    ORDER BY created_at DESC
                    LIMIT ?
                    "#,
                )
                .bind(conversation_id.as_deref())
                .bind(limit)
                .fetch_all(&self.pool)
                .await?;
                
                let mut items: Vec<serde_json::Value> = rows
                    .into_iter()
                    .map(|row| serde_json::from_str(&row.data))
                    .collect::<Result<Vec<_>, _>>()?;
                
                // 对于文本消息，提取文本内容到 extra 字段
                for item in &mut items {
                    extract_text_to_extra(item);
                }
                
                Ok(QueryResult::FindMessages { items })
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(sqlx::FromRow)]
struct ConversationRow {
    conversation_id: String,
    data: String,
    updated_at: String,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(sqlx::FromRow)]
struct MessageRow {
    message_id: String,
    conversation_id: String,
    data: String,
    created_at: String,
}

/// IndexedDB ReadStore 实现
#[cfg(target_arch = "wasm32")]
pub struct IndexedDbReadStore {
    db_name: String,
}

#[cfg(target_arch = "wasm32")]
impl IndexedDbReadStore {
    /// 创建新的 IndexedDB ReadStore
    pub async fn new(db_name: &str) -> anyhow::Result<Self> {
        // TODO: 实现 IndexedDB 初始化
        Ok(Self {
            db_name: db_name.to_string(),
        })
    }
}

#[cfg(target_arch = "wasm32")]
#[async_trait]
impl ReadStoreTrait for IndexedDbReadStore {
    async fn write_message(&self, _message: &Message) -> anyhow::Result<()> {
        // TODO: 实现 IndexedDB 写入
        Ok(())
    }
    
    async fn write_conversation(&self, _conversation: &Conversation) -> anyhow::Result<()> {
        // TODO: 实现 IndexedDB 写入
        Ok(())
    }
    
    async fn update_conversation(&self, conversation: &Conversation) -> anyhow::Result<()> {
        self.write_conversation(conversation).await
    }
    
    async fn delete_message(&self, _message_id: &str) -> anyhow::Result<()> {
        // TODO: 实现 IndexedDB 删除
        Ok(())
    }
    
    async fn delete_conversation_messages(&self, _conversation_id: &str) -> anyhow::Result<()> {
        // TODO: 实现 IndexedDB 删除
        Ok(())
    }
    
    async fn query(&self, query: Query) -> anyhow::Result<QueryResult> {
        // TODO: 实现 IndexedDB 查询
        match query {
            Query::ConversationList { .. } => {
                Ok(QueryResult::ConversationList {
                    items: vec![],
                    next_cursor: None,
                })
            }
            Query::ConversationDetail { .. } => {
                Ok(QueryResult::ConversationDetail {
                    item: serde_json::json!({}),
                })
            }
            Query::MessageList { .. } => {
                Ok(QueryResult::MessageList {
                    items: vec![],
                    next_cursor: None,
                })
            }
            Query::MessageDetail { .. } => {
                Ok(QueryResult::MessageDetail {
                    item: serde_json::json!({}),
                })
            }
            Query::SearchMessages { .. } => {
                Ok(QueryResult::SearchMessages { items: vec![] })
            }
            Query::FindMessages { .. } => {
                Ok(QueryResult::FindMessages { items: vec![] })
            }
        }
    }
}

/// 内存 ReadStore 实现（用于测试和生产环境）
///
/// 对标微信、Telegram、飞书的内存存储设计
pub struct MemoryReadStore {
    conversations: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, serde_json::Value>>>,
    messages: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, serde_json::Value>>>,
    // 按会话ID索引消息（conversation_id -> Vec<message_id>）
    conversation_messages: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, Vec<String>>>>,
}

impl MemoryReadStore {
    pub fn new() -> Self {
        Self {
            conversations: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            messages: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            conversation_messages: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }
}

#[async_trait]
impl ReadStoreTrait for MemoryReadStore {
    async fn query(&self, query: Query) -> anyhow::Result<QueryResult> {
        match query {
            Query::ConversationList { limit, .. } => {
                let conversations = self.conversations.read().await;
                // 按 updated_at 排序（降序）
                let mut items: Vec<serde_json::Value> = conversations.values().cloned().collect();
                items.sort_by(|a, b| {
                    let a_time = a.get("updated_at")
                        .and_then(|v| v.as_str())
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.timestamp())
                        .unwrap_or(0);
                    let b_time = b.get("updated_at")
                        .and_then(|v| v.as_str())
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.timestamp())
                        .unwrap_or(0);
                    b_time.cmp(&a_time) // 降序
                });
                let items: Vec<serde_json::Value> = items
                    .into_iter()
                    .take(limit.unwrap_or(100))
                    .collect();
                Ok(QueryResult::ConversationList {
                    items,
                    next_cursor: None,
                })
            }
            Query::ConversationDetail { conversation_id } => {
                let conversations = self.conversations.read().await;
                let item = conversations
                    .get(&conversation_id)
                    .cloned()
                    .unwrap_or(serde_json::json!({}));
                Ok(QueryResult::ConversationDetail { item })
            }
            Query::MessageList { conversation_id, limit, .. } => {
                let conversation_messages = self.conversation_messages.read().await;
                let message_ids = conversation_messages
                    .get(&conversation_id)
                    .cloned()
                    .unwrap_or_default();
                
                let messages = self.messages.read().await;
                let mut items: Vec<serde_json::Value> = message_ids
                    .iter()
                    .filter_map(|msg_id| messages.get(msg_id).cloned())
                    .collect();
                
                // 按时间戳排序（降序，最新的在前）
                items.sort_by(|a, b| {
                    let a_time = a.get("timestamp")
                        .or_else(|| a.get("created_at"))
                        .and_then(|v| v.as_str())
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.timestamp())
                        .unwrap_or(0);
                    let b_time = b.get("timestamp")
                        .or_else(|| b.get("created_at"))
                        .and_then(|v| v.as_str())
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.timestamp())
                        .unwrap_or(0);
                    b_time.cmp(&a_time) // 降序
                });
                
                let items: Vec<serde_json::Value> = items
                    .into_iter()
                    .take(limit.unwrap_or(100))
                    .collect();
                Ok(QueryResult::MessageList {
                    items,
                    next_cursor: None,
                })
            }
            Query::MessageDetail { message_id } => {
                let messages = self.messages.read().await;
                let item = messages
                    .get(&message_id)
                    .cloned()
                    .unwrap_or(serde_json::json!({}));
                Ok(QueryResult::MessageDetail { item })
            }
            Query::SearchMessages { conversation_id, keyword, limit } => {
                let messages = self.messages.read().await;
                let items: Vec<serde_json::Value> = messages
                    .values()
                    .filter(|m| {
                        // 过滤会话
                        if let Some(ref conv_id) = conversation_id {
                            if m.get("conversation_id").and_then(|v| v.as_str()) != Some(conv_id) {
                                return false;
                            }
                        }
                        // 搜索关键词（在文本内容中）
                        let text = m.get("text")
                            .or_else(|| m.get("content"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        text.contains(&keyword)
                    })
                    .take(limit.unwrap_or(100))
                    .cloned()
                    .collect();
                Ok(QueryResult::SearchMessages { items })
            }
            Query::FindMessages { conversation_id, message_type, start_time, end_time, limit } => {
                let messages = self.messages.read().await;
                let items: Vec<serde_json::Value> = messages
                    .values()
                    .filter(|m| {
                        // 过滤会话
                        if let Some(ref conv_id) = conversation_id {
                            if m.get("conversation_id").and_then(|v| v.as_str()) != Some(conv_id) {
                                return false;
                            }
                        }
                        // 过滤消息类型
                        if let Some(ref msg_type) = message_type {
                            if m.get("message_type").and_then(|v| v.as_str()) != Some(msg_type) {
                                return false;
                            }
                        }
                        // 过滤时间范围
                        if let Some(ref start) = start_time {
                            if let Some(timestamp) = m.get("timestamp")
                                .or_else(|| m.get("created_at"))
                                .and_then(|v| v.as_str())
                                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            {
                                if timestamp < *start {
                                    return false;
                                }
                            }
                        }
                        if let Some(ref end) = end_time {
                            if let Some(timestamp) = m.get("timestamp")
                                .or_else(|| m.get("created_at"))
                                .and_then(|v| v.as_str())
                                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            {
                                if timestamp > *end {
                                    return false;
                                }
                            }
                        }
                        true
                    })
                    .take(limit.unwrap_or(100))
                    .cloned()
                    .collect();
                Ok(QueryResult::FindMessages { items })
            }
        }
    }
    
    /// 写入消息到读模型
    async fn write_message(&self, message: &crate::domain::message::Message) -> anyhow::Result<()> {
        use serde_json::json;
        
        // 将 Message 转换为 JSON
        let message_json = json!({
            "message_id": message.id,
            "conversation_id": message.conversation_id,
            "sender_id": message.sender_id,
            "receiver_id": message.receiver_id,
            "message_type": format!("{:?}", message.message_type),
            "content": String::from_utf8_lossy(&message.content),
            "timestamp": message.timestamp.to_rfc3339(),
            "created_at": message.created_at.to_rfc3339(),
            "updated_at": message.updated_at.to_rfc3339(),
            "seq": message.seq,
            "state": format!("{:?}", message.state),
            "is_recalled": message.is_recalled,
            "is_burn_after_read": message.is_burn_after_read,
            "attachments": message.attachments.iter().map(|a| json!({
                "attachment_id": a.attachment_id,
                "attachment_type": a.attachment_type,
                "url": a.url,
                "size": a.size,
                "mime_type": a.mime_type,
            })).collect::<Vec<_>>(),
            "extra": message.extra,
            "attributes": message.attributes,
        });
        
        // 写入消息
        let mut messages = self.messages.write().await;
        messages.insert(message.id.clone(), message_json);
        
        // 更新会话消息索引
        let mut conversation_messages = self.conversation_messages.write().await;
        conversation_messages
            .entry(message.conversation_id.clone())
            .or_insert_with(Vec::new)
            .push(message.id.clone());
        
        Ok(())
    }
    
    /// 写入会话到读模型
    async fn write_conversation(&self, conversation: &crate::domain::conversation::Conversation) -> anyhow::Result<()> {
        use serde_json::json;
        
        // 将 Conversation 转换为 JSON
        let conversation_json = json!({
            "conversation_id": conversation.conversation_id,
            "conversation_type": conversation.conversation_type,
            "business_type": conversation.business_type,
            "display_name": conversation.display_name,
            "avatar_url": conversation.avatar_url,
            "unread_count": conversation.unread_count,
            "max_seq": conversation.max_seq,
            "last_read_seq": conversation.last_read_seq,
            "last_message": conversation.last_message.as_ref().map(|m| json!({
                "message_id": m.message_id,
                "sender_id": m.sender_id,
                "message_type": m.message_type,
                "text": m.text,
                "time": m.time.to_rfc3339(),
            })),
            "is_muted": conversation.is_muted,
            "is_pinned": conversation.is_pinned,
            "visibility": format!("{:?}", conversation.visibility),
            "lifecycle_state": format!("{:?}", conversation.lifecycle_state),
            "draft": conversation.draft,
            "created_at": conversation.created_at.to_rfc3339(),
            "updated_at": conversation.updated_at.to_rfc3339(),
            "version": conversation.version,
        });
        
        // 写入会话
        let mut conversations = self.conversations.write().await;
        conversations.insert(conversation.conversation_id.clone(), conversation_json);
        
        Ok(())
    }
    
    /// 更新会话
    async fn update_conversation(&self, conversation: &crate::domain::conversation::Conversation) -> anyhow::Result<()> {
        // 更新就是写入（覆盖）
        self.write_conversation(conversation).await
    }
    
    /// 删除消息（软删除）
    async fn delete_message(&self, message_id: &str) -> anyhow::Result<()> {
        let mut messages = self.messages.write().await;
        if let Some(message) = messages.get_mut(message_id) {
            // 标记为已删除
            message.as_object_mut()
                .and_then(|m| {
                    m.insert("deleted".to_string(), serde_json::json!(true));
                    m.insert("deleted_at".to_string(), serde_json::json!(chrono::Utc::now().to_rfc3339()));
                    Some(())
                });
        }
        Ok(())
    }
    
    /// 删除会话中的所有消息
    async fn delete_conversation_messages(&self, conversation_id: &str) -> anyhow::Result<()> {
        let conversation_messages = self.conversation_messages.read().await;
        if let Some(message_ids) = conversation_messages.get(conversation_id) {
            let message_ids = message_ids.clone();
            drop(conversation_messages);
            
            // 删除所有消息
            let mut messages = self.messages.write().await;
            for msg_id in message_ids {
                messages.remove(&msg_id);
            }
            
            // 清空会话消息索引
            let mut conversation_messages = self.conversation_messages.write().await;
            conversation_messages.remove(conversation_id);
        }
        Ok(())
    }
}

impl Default for MemoryReadStore {
    fn default() -> Self {
        Self::new()
    }
}
