//! SnapshotStore 实现
//!
//! 用于存储聚合根快照，加速恢复

use async_trait::async_trait;
use crate::domain::repository::SnapshotStore as SnapshotStoreTrait;

/// SQLite SnapshotStore 实现
#[cfg(not(target_arch = "wasm32"))]
pub struct SqliteSnapshotStore {
    // TODO: 实现 SQLite 连接
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait::async_trait]
impl<T> SnapshotStoreTrait<T> for SqliteSnapshotStore
where
    T: Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned,
{
    async fn load(&self, aggregate_id: &str) -> anyhow::Result<Option<T>> {
        // TODO: 实现 SQLite 查询
        tracing::info!("Loading snapshot for aggregate: {}", aggregate_id);
        Ok(None)
    }
    
    async fn save(&self, aggregate_id: &str, aggregate: &T, version: u64) -> anyhow::Result<()> {
        // TODO: 实现 SQLite 存储
        tracing::info!(
            "Saving snapshot for aggregate: {} version: {}",
            aggregate_id,
            version
        );
        Ok(())
    }
}

/// IndexedDB SnapshotStore 实现
#[cfg(target_arch = "wasm32")]
pub struct IndexedDbSnapshotStore {
    // TODO: 实现 IndexedDB 连接
}

#[cfg(target_arch = "wasm32")]
#[async_trait::async_trait]
impl<T> SnapshotStoreTrait<T> for IndexedDbSnapshotStore
where
    T: Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned,
{
    async fn load(&self, aggregate_id: &str) -> anyhow::Result<Option<T>> {
        // TODO: 实现 IndexedDB 查询
        tracing::info!("Loading snapshot for aggregate: {}", aggregate_id);
        Ok(None)
    }
    
    async fn save(&self, aggregate_id: &str, aggregate: &T, version: u64) -> anyhow::Result<()> {
        // TODO: 实现 IndexedDB 存储
        tracing::info!(
            "Saving snapshot for aggregate: {} version: {}",
            aggregate_id,
            version
        );
        Ok(())
    }
}

/// 内存 SnapshotStore 实现（用于测试）
pub struct MemorySnapshotStore<T> {
    snapshots: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, (T, u64)>>>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> MemorySnapshotStore<T>
where
    T: Send + Sync + Clone,
{
    pub fn new() -> Self {
        Self {
            snapshots: std::sync::Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<T> SnapshotStoreTrait<T> for MemorySnapshotStore<T>
where
    T: Send + Sync + Clone,
{
    async fn load(&self, aggregate_id: &str) -> anyhow::Result<Option<T>> {
        let snapshots = self.snapshots.read().await;
        Ok(snapshots.get(aggregate_id).map(|(agg, _)| agg.clone()))
    }
    
    async fn save(&self, aggregate_id: &str, aggregate: &T, version: u64) -> anyhow::Result<()> {
        let mut snapshots = self.snapshots.write().await;
        snapshots.insert(aggregate_id.to_string(), (aggregate.clone(), version));
        Ok(())
    }
}

impl<T> Default for MemorySnapshotStore<T>
where
    T: Send + Sync + Clone,
{
    fn default() -> Self {
        Self::new()
    }
}
