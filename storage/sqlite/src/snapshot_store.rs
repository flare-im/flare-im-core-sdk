//! SQLite 快照存储实现

use async_trait::async_trait;
use flare_im_core_sdk::domain::repository::SnapshotStore;
use sqlx::SqlitePool;
use std::sync::Arc;
use anyhow::Result;
use tracing::{debug, info};

/// SQLite 快照存储实现
pub struct SqliteSnapshotStore {
    pool: Arc<SqlitePool>,
}

impl SqliteSnapshotStore {
    /// 创建新的快照存储
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }
    
    /// 初始化数据库表
    pub async fn init(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS snapshots (
                aggregate_id TEXT NOT NULL,
                aggregate_type TEXT NOT NULL,
                version INTEGER NOT NULL,
                data TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (aggregate_id, aggregate_type)
            )
            "#,
        )
        .execute(&*self.pool)
        .await?;
        
        // 创建索引
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_snapshots_aggregate ON snapshots(aggregate_id, aggregate_type)"
        )
        .execute(&*self.pool)
        .await?;
        
        info!("Snapshot store tables initialized");
        Ok(())
    }
}

#[async_trait]
impl<A> SnapshotStore<A> for SqliteSnapshotStore
where
    A: Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned,
{
    async fn load(&self, aggregate_id: &str) -> Result<Option<A>> {
        // 从 aggregate_id 推断 aggregate_type（简化实现）
        // 实际应用中可能需要更复杂的类型推断
        let aggregate_type = "default";
        
        let row = sqlx::query_as::<_, SnapshotRow>(
            r#"
            SELECT aggregate_id, aggregate_type, version, data
            FROM snapshots
            WHERE aggregate_id = ?1 AND aggregate_type = ?2
            ORDER BY version DESC
            LIMIT 1
            "#,
        )
        .bind(aggregate_id)
        .bind(aggregate_type)
        .fetch_optional(&*self.pool)
        .await?;
        
        if let Some(row) = row {
            let aggregate: A = serde_json::from_str(&row.data)?;
            debug!(
                aggregate_id = %row.aggregate_id,
                version = row.version,
                "Snapshot loaded"
            );
            Ok(Some(aggregate))
        } else {
            Ok(None)
        }
    }
    
    async fn save(&self, aggregate_id: &str, aggregate: &A, version: u64) -> Result<()> {
        let aggregate_type = "default";
        let data = serde_json::to_string(aggregate)?;
        
        sqlx::query(
            r#"
            INSERT INTO snapshots (aggregate_id, aggregate_type, version, data)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(aggregate_id, aggregate_type) DO UPDATE SET
                version = excluded.version,
                data = excluded.data,
                created_at = datetime('now')
            "#,
        )
        .bind(aggregate_id)
        .bind(aggregate_type)
        .bind(version as i64)
        .bind(&data)
        .execute(&*self.pool)
        .await?;
        
        debug!(
            aggregate_id,
            version,
            "Snapshot saved"
        );
        
        Ok(())
    }
}

/// 数据库行结构
#[derive(sqlx::FromRow)]
struct SnapshotRow {
    aggregate_id: String,
    aggregate_type: String,
    version: i64,
    data: String,
}
