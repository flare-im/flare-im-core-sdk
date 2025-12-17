//! 存储缓存层
//!
//! 提供消息和会话的缓存，减少数据库查询

use crate::domain::{Message, SessionSummary};
use crate::infrastructure::storage::StorageBackend;
use anyhow::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 缓存项（带过期时间）
#[derive(Clone)]
struct CachedItem<T> {
    value: T,
    expires_at: Instant,
}

impl<T> CachedItem<T> {
    fn new(value: T, ttl: Duration) -> Self {
        Self {
            value,
            expires_at: Instant::now() + ttl,
        }
    }

    fn is_expired(&self) -> bool {
        Instant::now() > self.expires_at
    }
}

/// 带缓存的存储后端
///
/// 在原有存储后端基础上添加缓存层，提升查询性能
pub struct CachedStorage {
    /// 底层存储
    storage: Arc<dyn StorageBackend>,

    /// 消息缓存
    message_cache: Arc<DashMap<String, CachedItem<Message>>>,

    /// 会话缓存
    session_cache: Arc<DashMap<String, CachedItem<SessionSummary>>>,

    /// 缓存 TTL（默认 5 分钟）
    cache_ttl: Duration,

    /// 最大缓存大小（防止内存泄漏）
    max_cache_size: usize,
}

impl CachedStorage {
    /// 创建新的缓存存储
    ///
    /// # 参数
    /// - `storage`: 底层存储后端
    /// - `cache_ttl`: 缓存过期时间（默认 5 分钟）
    /// - `max_cache_size`: 最大缓存大小（默认 10000）
    pub fn new(
        storage: Arc<dyn StorageBackend>,
        cache_ttl: Duration,
        max_cache_size: usize,
    ) -> Self {
        Self {
            storage,
            message_cache: Arc::new(DashMap::new()),
            session_cache: Arc::new(DashMap::new()),
            cache_ttl,
            max_cache_size,
        }
    }

    /// 清理过期的缓存项
    pub fn cleanup_expired(&self) {
        let now = Instant::now();

        // 清理消息缓存
        self.message_cache.retain(|_, item| item.expires_at > now);

        // 清理会话缓存
        self.session_cache.retain(|_, item| item.expires_at > now);
    }

    /// 清理缓存（如果超过最大大小）
    fn evict_if_needed(&self) {
        // 如果消息缓存超过限制，清理最旧的 20%
        if self.message_cache.len() > self.max_cache_size {
            let to_remove = self.max_cache_size / 5;
            let mut keys_to_remove = Vec::with_capacity(to_remove);

            // 收集最旧的 key（按过期时间排序）
            let mut items: Vec<_> = self
                .message_cache
                .iter()
                .map(|entry| (entry.key().clone(), entry.value().expires_at))
                .collect();

            items.sort_by_key(|(_, expires_at)| *expires_at);

            for (key, _) in items.iter().take(to_remove) {
                keys_to_remove.push(key.clone());
            }

            for key in keys_to_remove {
                self.message_cache.remove(&key);
            }
        }

        // 同样处理会话缓存
        if self.session_cache.len() > self.max_cache_size {
            let to_remove = self.max_cache_size / 5;
            let mut keys_to_remove = Vec::with_capacity(to_remove);

            let mut items: Vec<_> = self
                .session_cache
                .iter()
                .map(|entry| (entry.key().clone(), entry.value().expires_at))
                .collect();

            items.sort_by_key(|(_, expires_at)| *expires_at);

            for (key, _) in items.iter().take(to_remove) {
                keys_to_remove.push(key.clone());
            }

            for key in keys_to_remove {
                self.session_cache.remove(&key);
            }
        }
    }
}

impl crate::infrastructure::storage::storage_trait::StorageSyncBounds for CachedStorage {}

#[async_trait]
impl StorageBackend for CachedStorage {
    async fn save_message(&self, message: &Message) -> Result<()> {
        // 保存到底层存储
        self.storage.save_message(message).await?;

        // 更新缓存
        let cached = CachedItem::new(message.clone(), self.cache_ttl);
        self.message_cache.insert(message.id.clone(), cached);

        // 检查是否需要清理
        self.evict_if_needed();

        Ok(())
    }

    async fn get_message(&self, message_id: &str) -> Result<Option<Message>> {
        // 先查缓存
        if let Some(entry) = self.message_cache.get(message_id) {
            if !entry.value().is_expired() {
                return Ok(Some(entry.value().value.clone()));
            } else {
                // 过期，移除
                self.message_cache.remove(message_id);
            }
        }

        // 缓存未命中，查询底层存储
        let message = self.storage.get_message(message_id).await?;

        // 更新缓存
        if let Some(ref msg) = message {
            let cached = CachedItem::new(msg.clone(), self.cache_ttl);
            self.message_cache.insert(message_id.to_string(), cached);
            self.evict_if_needed();
        }

        Ok(message)
    }

    async fn batch_get_messages(&self, message_ids: &[String]) -> Result<Vec<Message>> {
        // 先查缓存
        let mut messages = Vec::new();
        let mut uncached_ids = Vec::new();

        for message_id in message_ids {
            if let Some(entry) = self.message_cache.get(message_id) {
                if !entry.value().is_expired() {
                    messages.push(entry.value().value.clone());
                } else {
                    self.message_cache.remove(message_id);
                    uncached_ids.push(message_id.clone());
                }
            } else {
                uncached_ids.push(message_id.clone());
            }
        }

        // 批量查询未缓存的
        if !uncached_ids.is_empty() {
            let uncached_messages = self.storage.batch_get_messages(&uncached_ids).await?;

            // 更新缓存
            for msg in &uncached_messages {
                let cached = CachedItem::new(msg.clone(), self.cache_ttl);
                self.message_cache.insert(msg.id.clone(), cached);
            }

            messages.extend(uncached_messages);
            self.evict_if_needed();
        }

        Ok(messages)
    }

    async fn get_messages(
        &self,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<Message>> {
        // 消息列表查询不缓存（变化频繁）
        self.storage.get_messages(session_id, limit, cursor).await
    }

    async fn get_messages_by_seq(
        &self,
        session_id: &str,
        after_seq: i64,
        limit: usize,
    ) -> Result<Vec<Message>> {
        // 序列查询不缓存
        self.storage
            .get_messages_by_seq(session_id, after_seq, limit)
            .await
    }

    async fn get_max_seq(&self, session_id: &str) -> Result<Option<i64>> {
        self.storage.get_max_seq(session_id).await
    }

    async fn delete_message(&self, message_id: &str) -> Result<()> {
        // 删除底层存储
        self.storage.delete_message(message_id).await?;

        // 清除缓存
        self.message_cache.remove(message_id);

        Ok(())
    }

    async fn save_session(&self, session: &SessionSummary) -> Result<()> {
        // 保存到底层存储
        self.storage.save_session(session).await?;

        // 更新缓存
        let cached = CachedItem::new(session.clone(), self.cache_ttl);
        self.session_cache
            .insert(session.session_id.clone(), cached);

        // 检查是否需要清理
        self.evict_if_needed();

        Ok(())
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<SessionSummary>> {
        // 先查缓存
        if let Some(entry) = self.session_cache.get(session_id) {
            if !entry.value().is_expired() {
                return Ok(Some(entry.value().value.clone()));
            } else {
                // 过期，移除
                self.session_cache.remove(session_id);
            }
        }

        // 缓存未命中，查询底层存储
        let session = self.storage.get_session(session_id).await?;

        // 更新缓存
        if let Some(ref sess) = session {
            let cached = CachedItem::new(sess.clone(), self.cache_ttl);
            self.session_cache.insert(session_id.to_string(), cached);
            self.evict_if_needed();
        }

        Ok(session)
    }

    async fn batch_get_sessions(&self, session_ids: &[String]) -> Result<Vec<SessionSummary>> {
        // 先查缓存
        let mut sessions = Vec::new();
        let mut uncached_ids = Vec::new();

        for session_id in session_ids {
            if let Some(entry) = self.session_cache.get(session_id) {
                if !entry.value().is_expired() {
                    sessions.push(entry.value().value.clone());
                } else {
                    self.session_cache.remove(session_id);
                    uncached_ids.push(session_id.clone());
                }
            } else {
                uncached_ids.push(session_id.clone());
            }
        }

        // 批量查询未缓存的
        if !uncached_ids.is_empty() {
            let uncached_sessions = self.storage.batch_get_sessions(&uncached_ids).await?;

            // 更新缓存
            for sess in &uncached_sessions {
                let cached = CachedItem::new(sess.clone(), self.cache_ttl);
                self.session_cache.insert(sess.session_id.clone(), cached);
            }

            sessions.extend(uncached_sessions);
            self.evict_if_needed();
        }

        Ok(sessions)
    }

    async fn get_sessions(
        &self,
        filter: crate::infrastructure::storage::SessionFilter,
    ) -> Result<Vec<SessionSummary>> {
        // 会话列表查询不缓存（变化频繁）
        self.storage.get_sessions(filter).await
    }

    async fn update_session(
        &self,
        session_id: &str,
        updates: crate::infrastructure::storage::SessionUpdate,
    ) -> Result<()> {
        // 更新底层存储
        self.storage.update_session(session_id, updates).await?;

        // 清除缓存（需要重新查询）
        self.session_cache.remove(session_id);

        Ok(())
    }

    async fn delete_session(&self, session_id: &str) -> Result<()> {
        // 删除底层存储
        self.storage.delete_session(session_id).await?;

        // 清除缓存
        self.session_cache.remove(session_id);

        Ok(())
    }

    async fn save_sync_cursor(
        &self,
        session_id: &str,
        cursor: &crate::domain::sync::SyncCursor,
    ) -> Result<()> {
        self.storage.save_sync_cursor(session_id, cursor).await
    }

    async fn get_sync_cursor(
        &self,
        session_id: &str,
    ) -> Result<Option<crate::domain::sync::SyncCursor>> {
        self.storage.get_sync_cursor(session_id).await
    }

    async fn get_all_sync_cursors(&self) -> Result<Vec<crate::domain::sync::SyncCursor>> {
        self.storage.get_all_sync_cursors().await
    }

    async fn save_message_state(
        &self,
        user_id: &str,
        message_id: &str,
        state: crate::infrastructure::storage::MessageState,
    ) -> Result<()> {
        self.storage
            .save_message_state(user_id, message_id, state)
            .await
    }

    async fn get_message_state(
        &self,
        user_id: &str,
        message_id: &str,
    ) -> Result<Option<crate::infrastructure::storage::MessageState>> {
        self.storage.get_message_state(user_id, message_id).await
    }

    async fn batch_check_deleted(
        &self,
        user_id: &str,
        message_ids: &[String],
    ) -> Result<Vec<String>> {
        self.storage.batch_check_deleted(user_id, message_ids).await
    }

    #[cfg(feature = "extensions")]
    async fn save_message_extension(
        &self,
        message_id: &str,
        extension: &crate::domain::extension::MessageExtension,
    ) -> Result<()> {
        self.storage
            .save_message_extension(message_id, extension)
            .await
    }

    #[cfg(feature = "extensions")]
    async fn get_message_extension(
        &self,
        message_id: &str,
    ) -> Result<Option<crate::domain::extension::MessageExtension>> {
        self.storage.get_message_extension(message_id).await
    }

    #[cfg(feature = "extensions")]
    async fn save_session_extension(
        &self,
        session_id: &str,
        extension: &crate::domain::extension::SessionExtension,
    ) -> Result<()> {
        self.storage
            .save_session_extension(session_id, extension)
            .await
    }

    #[cfg(feature = "extensions")]
    async fn get_session_extension(
        &self,
        session_id: &str,
    ) -> Result<Option<crate::domain::extension::SessionExtension>> {
        self.storage.get_session_extension(session_id).await
    }

    #[cfg(feature = "extensions")]
    async fn batch_get_message_extensions(
        &self,
        message_ids: &[String],
    ) -> Result<Vec<(String, crate::domain::extension::MessageExtension)>> {
        self.storage.batch_get_message_extensions(message_ids).await
    }

    #[cfg(feature = "extensions")]
    async fn batch_get_session_extensions(
        &self,
        session_ids: &[String],
    ) -> Result<Vec<(String, crate::domain::extension::SessionExtension)>> {
        self.storage.batch_get_session_extensions(session_ids).await
    }
}

#[cfg(test)]
mod tests {
    use super::CachedStorage;

    #[tokio::test]
    async fn test_cached_storage() {
        // 测试缓存功能
        // 注意：需要 mock StorageBackend
    }
}
