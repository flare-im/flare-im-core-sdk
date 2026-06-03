//! 待发送队列：内存缓存 + 后端分层 — 读走缓存、写透传，兼顾效率与稳定性。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::{PendingSendReader, PendingSendVo, PendingSendWriter};
use crate::shared::error::Result;

/// 待发送队列：内存缓存 + 后端（读/写均使用 domain trait）
pub struct LayeredPendingSendStore {
    cache: Arc<RwLock<HashMap<String, PendingSendVo>>>,
    reader: Arc<dyn PendingSendReader>,
    writer: Arc<dyn PendingSendWriter>,
}

impl LayeredPendingSendStore {
    /// 使用同一实现体的 Reader/Writer（保证读写同一后端）
    pub fn new(reader: Arc<dyn PendingSendReader>, writer: Arc<dyn PendingSendWriter>) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            reader,
            writer,
        }
    }
}

#[async_trait]
impl PendingSendReader for LayeredPendingSendStore {
    async fn get(&self, client_msg_id: &str) -> Result<Option<PendingSendVo>> {
        {
            let cache = self.cache.read().await;
            if let Some(e) = cache.get(client_msg_id) {
                return Ok(Some(e.clone()));
            }
        }
        let e = self.reader.get(client_msg_id).await?;
        if let Some(ref entry) = e {
            let mut cache = self.cache.write().await;
            cache.insert(entry.client_msg_id.clone(), entry.clone());
        }
        Ok(e)
    }

    async fn list(&self) -> Result<Vec<PendingSendVo>> {
        self.reader.list().await
    }

    async fn take_oldest(&self) -> Result<Option<PendingSendVo>> {
        self.reader.take_oldest().await
    }
}

#[async_trait]
impl PendingSendWriter for LayeredPendingSendStore {
    async fn push(&self, entry: PendingSendVo) -> Result<()> {
        self.writer.push(entry.clone()).await?;
        let mut cache = self.cache.write().await;
        cache.insert(entry.client_msg_id.clone(), entry);
        Ok(())
    }

    async fn pop(&self, client_msg_id: &str) -> Result<Option<PendingSendVo>> {
        let e = self.writer.pop(client_msg_id).await?;
        if e.is_some() {
            let mut cache = self.cache.write().await;
            cache.remove(client_msg_id);
        }
        Ok(e)
    }
}
