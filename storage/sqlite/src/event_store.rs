//! SQLite 事件存储实现

use async_trait::async_trait;
use flare_im_core_sdk::domain::repository::EventStore;
use flare_im_core_sdk::domain::event::DomainEvent;
use sqlx::SqlitePool;
use std::sync::Arc;
use anyhow::Result;
use tracing::{debug, error, info};

/// SQLite 事件存储实现
pub struct SqliteEventStore {
    pool: Arc<SqlitePool>,
}

impl SqliteEventStore {
    /// 创建新的事件存储
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }
    
    /// 初始化数据库表
    pub async fn init(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS events (
                event_id TEXT PRIMARY KEY,
                event_type TEXT NOT NULL,
                aggregate_id TEXT NOT NULL,
                version INTEGER NOT NULL,
                timestamp TEXT NOT NULL,
                data TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )
            "#,
        )
        .execute(&*self.pool)
        .await?;
        
        // 创建索引
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_events_aggregate_id ON events(aggregate_id)"
        )
        .execute(&*self.pool)
        .await?;
        
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_events_aggregate_version ON events(aggregate_id, version)"
        )
        .execute(&*self.pool)
        .await?;
        
        info!("Event store tables initialized");
        Ok(())
    }
}

#[async_trait]
impl EventStore for SqliteEventStore {
    async fn append(&self, event: DomainEvent) -> Result<()> {
        let data_json = serde_json::to_string(&event.data)?;
        
        sqlx::query(
            r#"
            INSERT INTO events (event_id, event_type, aggregate_id, version, timestamp, data)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(event_id) DO NOTHING
            "#,
        )
        .bind(&event.event_id)
        .bind(&event.event_type)
        .bind(&event.aggregate_id)
        .bind(event.version as i64)
        .bind(event.timestamp.to_rfc3339())
        .bind(&data_json)
        .execute(&*self.pool)
        .await?;
        
        debug!(
            event_id = %event.event_id,
            aggregate_id = %event.aggregate_id,
            version = event.version,
            "Event appended"
        );
        
        Ok(())
    }
    
    async fn load_stream(&self, aggregate_id: &str) -> Result<Vec<DomainEvent>> {
        let rows = sqlx::query_as::<_, EventRow>(
            r#"
            SELECT event_id, event_type, aggregate_id, version, timestamp, data
            FROM events
            WHERE aggregate_id = ?1
            ORDER BY version ASC
            "#,
        )
        .bind(aggregate_id)
        .fetch_all(&*self.pool)
        .await?;
        
        let events: Result<Vec<DomainEvent>> = rows
            .into_iter()
            .map(|row| row.try_into())
            .collect();
        
        events
    }
    
    async fn load_stream_from_version(
        &self,
        aggregate_id: &str,
        from_version: u64,
    ) -> Result<Vec<DomainEvent>> {
        let rows = sqlx::query_as::<_, EventRow>(
            r#"
            SELECT event_id, event_type, aggregate_id, version, timestamp, data
            FROM events
            WHERE aggregate_id = ?1 AND version > ?2
            ORDER BY version ASC
            "#,
        )
        .bind(aggregate_id)
        .bind(from_version as i64)
        .fetch_all(&*self.pool)
        .await?;
        
        let events: Result<Vec<DomainEvent>> = rows
            .into_iter()
            .map(|row| row.try_into())
            .collect();
        
        events
    }
}

/// 数据库行结构
#[derive(sqlx::FromRow)]
struct EventRow {
    event_id: String,
    event_type: String,
    aggregate_id: String,
    version: i64,
    timestamp: String,
    data: String,
}

impl TryInto<DomainEvent> for EventRow {
    type Error = anyhow::Error;
    
    fn try_into(self) -> Result<DomainEvent> {
        let timestamp = chrono::DateTime::parse_from_rfc3339(&self.timestamp)?
            .with_timezone(&chrono::Utc);
        
        let data: serde_json::Value = serde_json::from_str(&self.data)?;
        
        Ok(DomainEvent {
            event_id: self.event_id,
            event_type: self.event_type,
            aggregate_id: self.aggregate_id,
            version: self.version as u64,
            timestamp,
            data,
        })
    }
}
