//! 纯内存仓储 — 用于未配置 SQLite 时的 fallback（如 StoreProvider::user_profiles_or_memory）。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::{
    PendingSendReader, PendingSendVo, PendingSendWriter, UserProfile, UserReader, UserWriter,
};
use crate::shared::error::Result;

/// 内存用户资料存储（未配置 SQLite 时由 StoreProvider 提供）
pub struct MemoryUserProfileStore {
    data: Arc<RwLock<HashMap<String, UserProfile>>>,
}

impl MemoryUserProfileStore {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for MemoryUserProfileStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UserReader for MemoryUserProfileStore {
    async fn get(&self, user_id: &str) -> Result<Option<UserProfile>> {
        let data = self.data.read().await;
        Ok(data.get(user_id).cloned())
    }
}

#[async_trait]
impl UserWriter for MemoryUserProfileStore {
    async fn save_batch(&self, profiles: &[UserProfile]) -> Result<()> {
        let mut data = self.data.write().await;
        for p in profiles {
            data.insert(p.user_id.clone(), p.clone());
        }
        Ok(())
    }
}

// ---------- PendingSend ----------

/// 内存待发送队列（可选，未配置 SQLite 时使用）
pub struct MemoryPendingSendStore {
    data: Arc<RwLock<HashMap<String, PendingSendVo>>>,
}

impl MemoryPendingSendStore {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for MemoryPendingSendStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PendingSendReader for MemoryPendingSendStore {
    async fn get(&self, client_msg_id: &str) -> Result<Option<PendingSendVo>> {
        let data = self.data.read().await;
        Ok(data.get(client_msg_id).cloned())
    }

    async fn list(&self) -> Result<Vec<PendingSendVo>> {
        let data = self.data.read().await;
        Ok(data.values().cloned().collect::<Vec<_>>())
    }

    async fn take_oldest(&self) -> Result<Option<PendingSendVo>> {
        let data = self.data.read().await;
        Ok(data.values().min_by_key(|e| e.enqueued_at_ms).cloned())
    }
}

#[async_trait]
impl PendingSendWriter for MemoryPendingSendStore {
    async fn push(&self, entry: PendingSendVo) -> Result<()> {
        let mut data = self.data.write().await;
        data.insert(entry.client_msg_id.clone(), entry);
        Ok(())
    }

    async fn pop(&self, client_msg_id: &str) -> Result<Option<PendingSendVo>> {
        let mut data = self.data.write().await;
        Ok(data.remove(client_msg_id))
    }
}
