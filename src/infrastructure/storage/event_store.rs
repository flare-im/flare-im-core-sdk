//! EventStore 实现
//!
//! 基于 SQLite（Desktop/Mobile/HarmonyOS）或 IndexedDB（Web）

use async_trait::async_trait;
use crate::domain::event::DomainEvent;
use crate::domain::repository::EventStore as EventStoreTrait;
use serde_json;

/// SQLite EventStore 实现
#[cfg(not(target_arch = "wasm32"))]
pub struct SqliteEventStore {
    pool: sqlx::SqlitePool,
}

#[cfg(not(target_arch = "wasm32"))]
impl SqliteEventStore {
    /// 创建新的 SQLite EventStore
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
        
        // 创建事件表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS events (
                event_id TEXT PRIMARY KEY,
                event_type TEXT NOT NULL,
                aggregate_id TEXT NOT NULL,
                version INTEGER NOT NULL,
                timestamp TEXT NOT NULL,
                data TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await?;
        
        // 创建索引
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_aggregate_id ON events(aggregate_id)")
            .execute(&pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_version ON events(aggregate_id, version)")
            .execute(&pool)
            .await?;
        
        Ok(Self { pool })
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl EventStoreTrait for SqliteEventStore {
    async fn append(&self, event: DomainEvent) -> anyhow::Result<()> {
        let data_json = serde_json::to_string(&event.data)?;
        
        sqlx::query(
            r#"
            INSERT INTO events (event_id, event_type, aggregate_id, version, timestamp, data)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&event.event_id)
        .bind(&event.event_type)
        .bind(&event.aggregate_id)
        .bind(event.version as i64)
        .bind(event.timestamp.to_rfc3339())
        .bind(data_json)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    async fn load_stream(&self, aggregate_id: &str) -> anyhow::Result<Vec<DomainEvent>> {
        let rows = sqlx::query_as::<_, EventRow>(
            r#"
            SELECT event_id, event_type, aggregate_id, version, timestamp, data
            FROM events
            WHERE aggregate_id = ?
            ORDER BY version ASC
            "#,
        )
        .bind(aggregate_id)
        .fetch_all(&self.pool)
        .await?;
        
        let events: Vec<DomainEvent> = rows
            .into_iter()
            .map(|row| row.into())
            .collect::<anyhow::Result<_>>()?;
        
        Ok(events)
    }
    
    async fn load_stream_from_version(
        &self,
        aggregate_id: &str,
        from_version: u64,
    ) -> anyhow::Result<Vec<DomainEvent>> {
        let rows = sqlx::query_as::<_, EventRow>(
            r#"
            SELECT event_id, event_type, aggregate_id, version, timestamp, data
            FROM events
            WHERE aggregate_id = ? AND version >= ?
            ORDER BY version ASC
            "#,
        )
        .bind(aggregate_id)
        .bind(from_version as i64)
        .fetch_all(&self.pool)
        .await?;
        
        let events: Vec<DomainEvent> = rows
            .into_iter()
            .map(|row| row.into())
            .collect::<anyhow::Result<_>>()?;
        
        Ok(events)
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(sqlx::FromRow)]
struct EventRow {
    event_id: String,
    event_type: String,
    aggregate_id: String,
    version: i64,
    timestamp: String,
    data: String,
}

#[cfg(not(target_arch = "wasm32"))]
impl From<EventRow> for anyhow::Result<DomainEvent> {
    fn from(row: EventRow) -> Self {
        Ok(DomainEvent {
            event_id: row.event_id,
            event_type: row.event_type,
            aggregate_id: row.aggregate_id,
            version: row.version as u64,
            timestamp: chrono::DateTime::parse_from_rfc3339(&row.timestamp)
                .map_err(|e| anyhow::anyhow!("Failed to parse timestamp: {}", e))?
                .with_timezone(&chrono::Utc),
            data: serde_json::from_str(&row.data)?,
        })
    }
}

/// IndexedDB EventStore 实现
#[cfg(target_arch = "wasm32")]
pub struct IndexedDbEventStore {
    db_name: String,
    store_name: String,
}

#[cfg(target_arch = "wasm32")]
impl IndexedDbEventStore {
    /// 创建新的 IndexedDB EventStore
    pub async fn new(db_name: &str) -> anyhow::Result<Self> {
        use wasm_bindgen::JsValue;
        use web_sys::{IdbDatabase, IdbOpenDbRequest, IdbTransactionMode};
        
        // 打开数据库
        let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("No window object"))?;
        let idb_factory = window
            .indexed_db()
            .map_err(|_| anyhow::anyhow!("Failed to get IndexedDB"))?;
        
        let open_request = idb_factory
            .open_with_u32(db_name, 1)
            .map_err(|_| anyhow::anyhow!("Failed to open database"))?;
        
        // 等待数据库打开
        let promise = wasm_bindgen_futures::JsFuture::from(
            js_sys::Promise::from(open_request.into())
        );
        let _ = promise.await?;
        
        Ok(Self {
            db_name: db_name.to_string(),
            store_name: "events".to_string(),
        })
    }
    
    async fn get_db(&self) -> anyhow::Result<IdbDatabase> {
        use wasm_bindgen::JsValue;
        use web_sys::IdbOpenDbRequest;
        
        let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("No window object"))?;
        let idb_factory = window
            .indexed_db()
            .map_err(|_| anyhow::anyhow!("Failed to get IndexedDB"))?;
        
        let open_request = idb_factory
            .open_with_u32(&self.db_name, 1)
            .map_err(|_| anyhow::anyhow!("Failed to open database"))?;
        
        let promise = wasm_bindgen_futures::JsFuture::from(
            js_sys::Promise::from(open_request.into())
        );
        let result = promise.await?;
        let db = js_sys::Reflect::get(&result, &JsValue::from_str("result"))
            .and_then(|v| v.dyn_into::<IdbDatabase>())
            .map_err(|_| anyhow::anyhow!("Failed to get database"))?;
        
        Ok(db)
    }
}

#[cfg(target_arch = "wasm32")]
#[async_trait]
impl EventStoreTrait for IndexedDbEventStore {
    async fn append(&self, event: DomainEvent) -> anyhow::Result<()> {
        use wasm_bindgen::JsValue;
        use web_sys::{IdbTransactionMode, IdbObjectStore};
        
        let db = self.get_db().await?;
        let tx = db
            .transaction_with_str_and_mode(&self.store_name, IdbTransactionMode::Readwrite)
            .map_err(|_| anyhow::anyhow!("Failed to create transaction"))?;
        
        let store = tx
            .object_store(&self.store_name)
            .map_err(|_| anyhow::anyhow!("Failed to get object store"))?;
        
        let data_json = serde_json::to_string(&event)?;
        let value = JsValue::from_str(&data_json);
        
        store
            .put_with_key(&JsValue::from_str(&event.event_id), &value)
            .map_err(|_| anyhow::anyhow!("Failed to put event"))?;
        
        let promise = wasm_bindgen_futures::JsFuture::from(
            js_sys::Promise::from(tx.into())
        );
        promise.await?;
        
        Ok(())
    }
    
    async fn load_stream(&self, aggregate_id: &str) -> anyhow::Result<Vec<DomainEvent>> {
        // TODO: 实现 IndexedDB 查询
        tracing::info!("Loading event stream for aggregate: {}", aggregate_id);
        Ok(vec![])
    }
    
    async fn load_stream_from_version(
        &self,
        aggregate_id: &str,
        from_version: u64,
    ) -> anyhow::Result<Vec<DomainEvent>> {
        // TODO: 实现 IndexedDB 查询
        tracing::info!(
            "Loading event stream for aggregate: {} from version: {}",
            aggregate_id,
            from_version
        );
        Ok(vec![])
    }
}

/// 内存 EventStore 实现（用于测试）
pub struct MemoryEventStore {
    events: std::sync::Arc<tokio::sync::RwLock<Vec<DomainEvent>>>,
}

impl MemoryEventStore {
    pub fn new() -> Self {
        Self {
            events: std::sync::Arc::new(tokio::sync::RwLock::new(Vec::new())),
        }
    }
}

#[async_trait]
impl EventStoreTrait for MemoryEventStore {
    async fn append(&self, event: DomainEvent) -> anyhow::Result<()> {
        let mut events = self.events.write().await;
        events.push(event);
        Ok(())
    }
    
    async fn load_stream(&self, aggregate_id: &str) -> anyhow::Result<Vec<DomainEvent>> {
        let events = self.events.read().await;
        Ok(events
            .iter()
            .filter(|e| e.aggregate_id == aggregate_id)
            .cloned()
            .collect())
    }
    
    async fn load_stream_from_version(
        &self,
        aggregate_id: &str,
        from_version: u64,
    ) -> anyhow::Result<Vec<DomainEvent>> {
        let events = self.events.read().await;
        Ok(events
            .iter()
            .filter(|e| e.aggregate_id == aggregate_id && e.version >= from_version)
            .cloned()
            .collect())
    }
}

impl Default for MemoryEventStore {
    fn default() -> Self {
        Self::new()
    }
}
