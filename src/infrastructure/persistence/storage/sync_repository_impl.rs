//! 同步仓储实现
//!
//! 实现 domain::sync::repository::SyncRepository 接口

use crate::domain::message::model::SessionId;
use crate::domain::sync::model::Sync as DomainSync;
use crate::domain::sync::repository::SyncRepository;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

/// 同步仓储实现
///
/// 注意：当前使用内存存储，后续可以改为持久化存储
pub struct SyncRepositoryImpl {
    /// 内存存储（临时实现）
    syncs: Arc<tokio::sync::RwLock<std::collections::HashMap<String, DomainSync>>>,
}

impl SyncRepositoryImpl {
    pub fn new() -> Self {
        Self {
            syncs: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }
}

#[async_trait]
impl SyncRepository for SyncRepositoryImpl {
    async fn save(&self, sync: &DomainSync) -> Result<()> {
        let key = sync
            .session_id()
            .map(|id| id.to_string())
            .unwrap_or_else(|| "global".to_string());
        let mut syncs = self.syncs.write().await;
        syncs.insert(key, sync.clone());
        Ok(())
    }

    async fn find_by_session(&self, session_id: &SessionId) -> Result<Option<DomainSync>> {
        let syncs = self.syncs.read().await;
        Ok(syncs.get(session_id.as_str()).cloned())
    }

    async fn find_all(&self) -> Result<Vec<DomainSync>> {
        let syncs = self.syncs.read().await;
        Ok(syncs.values().cloned().collect())
    }
}
