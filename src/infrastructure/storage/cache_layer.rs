//! 存储缓存层
//!
//! 在存储后端之上提供缓存层，减少数据库查询，提升性能

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

    fn value(&self) -> &T {
        &self.value
    }
}

/// 查询缓存
///
/// 提供消息和会话的查询缓存，减少数据库查询
pub struct QueryCache {
    /// 消息缓存（message_id -> Message）
    message_cache: DashMap<String, CachedItem<Message>>,

    /// 会话缓存（session_id -> SessionSummary）
    session_cache: DashMap<String, CachedItem<SessionSummary>>,

    /// 消息列表缓存（session_id -> (cursor, messages)）
    message_list_cache: DashMap<String, CachedItem<(Option<String>, Vec<Message>)>>,

    /// 会话列表缓存
    session_list_cache: DashMap<String, CachedItem<Vec<SessionSummary>>>,

    /// 消息 TTL（默认 5 分钟）
    message_ttl: Duration,

    /// 会话 TTL（默认 3 分钟）
    session_ttl: Duration,

    /// 消息列表 TTL（默认 1 分钟）
    message_list_ttl: Duration,

    /// 会话列表 TTL（默认 30 秒）
    session_list_ttl: Duration,
}

impl QueryCache {
    /// 创建新的查询缓存
    pub fn new() -> Self {
        Self {
            message_cache: DashMap::new(),
            session_cache: DashMap::new(),
            message_list_cache: DashMap::new(),
            session_list_cache: DashMap::new(),
            message_ttl: Duration::from_secs(300),     // 5 分钟
            session_ttl: Duration::from_secs(180),     // 3 分钟
            message_list_ttl: Duration::from_secs(60), // 1 分钟
            session_list_ttl: Duration::from_secs(30), // 30 秒
        }
    }

    /// 创建带自定义 TTL 的缓存
    pub fn with_ttl(
        message_ttl: Duration,
        session_ttl: Duration,
        message_list_ttl: Duration,
        session_list_ttl: Duration,
    ) -> Self {
        Self {
            message_cache: DashMap::new(),
            session_cache: DashMap::new(),
            message_list_cache: DashMap::new(),
            session_list_cache: DashMap::new(),
            message_ttl,
            session_ttl,
            message_list_ttl,
            session_list_ttl,
        }
    }

    /// 获取消息（带缓存）
    pub fn get_message(&self, message_id: &str) -> Option<Message> {
        self.message_cache.get(message_id).and_then(|entry| {
            if entry.is_expired() {
                self.message_cache.remove(message_id);
                None
            } else {
                Some(entry.value().value().clone())
            }
        })
    }

    /// 缓存消息
    pub fn cache_message(&self, message_id: String, message: Message) {
        self.message_cache
            .insert(message_id, CachedItem::new(message, self.message_ttl));
    }

    /// 获取会话（带缓存）
    pub fn get_session(&self, session_id: &str) -> Option<SessionSummary> {
        self.session_cache.get(session_id).and_then(|entry| {
            if entry.is_expired() {
                self.session_cache.remove(session_id);
                None
            } else {
                Some(entry.value().value().clone())
            }
        })
    }

    /// 缓存会话
    pub fn cache_session(&self, session_id: String, session: SessionSummary) {
        self.session_cache
            .insert(session_id, CachedItem::new(session, self.session_ttl));
    }

    /// 获取消息列表（带缓存）
    pub fn get_message_list(
        &self,
        session_id: &str,
        cursor: Option<&String>,
    ) -> Option<Vec<Message>> {
        let cache_key = format!(
            "{}:{}",
            session_id,
            cursor.as_ref().map(|c| c.as_str()).unwrap_or("")
        );
        self.message_list_cache.get(&cache_key).and_then(|entry| {
            if entry.is_expired() {
                self.message_list_cache.remove(&cache_key);
                None
            } else {
                let cached_value = entry.value().value();
                Some(cached_value.1.clone())
            }
        })
    }

    /// 缓存消息列表
    pub fn cache_message_list(
        &self,
        session_id: String,
        cursor: Option<String>,
        messages: Vec<Message>,
    ) {
        let cache_key = format!(
            "{}:{}",
            session_id,
            cursor.as_ref().map(|c| c.as_str()).unwrap_or("")
        );
        self.message_list_cache.insert(
            cache_key,
            CachedItem::new((cursor, messages), self.message_list_ttl),
        );
    }

    /// 获取会话列表（带缓存）
    pub fn get_session_list(&self, filter_key: &str) -> Option<Vec<SessionSummary>> {
        self.session_list_cache.get(filter_key).and_then(|entry| {
            if entry.is_expired() {
                self.session_list_cache.remove(filter_key);
                None
            } else {
                Some(entry.value().value().clone())
            }
        })
    }

    /// 缓存会话列表
    pub fn cache_session_list(&self, filter_key: String, sessions: Vec<SessionSummary>) {
        self.session_list_cache
            .insert(filter_key, CachedItem::new(sessions, self.session_list_ttl));
    }

    /// 清除消息缓存
    pub fn invalidate_message(&self, message_id: &str) {
        self.message_cache.remove(message_id);
        // 同时清除相关的消息列表缓存
        self.message_list_cache
            .retain(|key, _| !key.starts_with(&format!("{}:", message_id)));
    }

    /// 清除会话缓存
    pub fn invalidate_session(&self, session_id: &str) {
        self.session_cache.remove(session_id);
        // 同时清除相关的消息列表缓存
        self.message_list_cache
            .retain(|key, _| !key.starts_with(&format!("{}:", session_id)));
        // 清除会话列表缓存
        self.session_list_cache.clear();
    }

    /// 清除所有缓存
    pub fn clear(&self) {
        self.message_cache.clear();
        self.session_cache.clear();
        self.message_list_cache.clear();
        self.session_list_cache.clear();
    }

    /// 清理过期缓存
    pub fn cleanup_expired(&self) {
        let now = Instant::now();

        self.message_cache.retain(|_, item| now < item.expires_at);

        self.session_cache.retain(|_, item| now < item.expires_at);

        self.message_list_cache
            .retain(|_, item| now < item.expires_at);

        self.session_list_cache
            .retain(|_, item| now < item.expires_at);
    }

    /// 获取缓存统计信息
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            message_count: self.message_cache.len(),
            session_count: self.session_cache.len(),
            message_list_count: self.message_list_cache.len(),
            session_list_count: self.session_list_cache.len(),
        }
    }
}

impl Default for QueryCache {
    fn default() -> Self {
        Self::new()
    }
}

/// 缓存统计信息
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub message_count: usize,
    pub session_count: usize,
    pub message_list_count: usize,
    pub session_list_count: usize,
}

/// 带缓存的存储后端包装器
///
/// 在存储后端之上提供缓存层，自动处理缓存逻辑
pub struct CachedStorageBackend {
    storage: Arc<dyn StorageBackend>,
    cache: Arc<QueryCache>,
}

impl CachedStorageBackend {
    /// 创建带缓存的存储后端
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self {
            storage,
            cache: Arc::new(QueryCache::new()),
        }
    }

    /// 创建带自定义 TTL 的缓存存储后端
    pub fn with_ttl(
        storage: Arc<dyn StorageBackend>,
        message_ttl: Duration,
        session_ttl: Duration,
        message_list_ttl: Duration,
        session_list_ttl: Duration,
    ) -> Self {
        Self {
            storage,
            cache: Arc::new(QueryCache::with_ttl(
                message_ttl,
                session_ttl,
                message_list_ttl,
                session_list_ttl,
            )),
        }
    }

    /// 获取底层存储后端
    pub fn inner(&self) -> Arc<dyn StorageBackend> {
        Arc::clone(&self.storage)
    }

    /// 获取缓存统计信息
    pub fn cache_stats(&self) -> CacheStats {
        self.cache.stats()
    }

    /// 清除所有缓存
    pub fn clear_cache(&self) {
        self.cache.clear();
    }

    /// 清理过期缓存
    pub fn cleanup_cache(&self) {
        self.cache.cleanup_expired();
    }
}

#[async_trait]
impl StorageBackend for CachedStorageBackend {
    async fn save_message(&self, message: &Message) -> Result<()> {
        self.storage.save_message(message).await?;
        // 更新缓存
        self.cache
            .cache_message(message.id.clone(), message.clone());
        Ok(())
    }

    async fn get_message(&self, message_id: &str) -> Result<Option<Message>> {
        // 先检查缓存
        if let Some(message) = self.cache.get_message(message_id) {
            return Ok(Some(message));
        }

        // 缓存未命中，从存储查询
        if let Some(message) = self.storage.get_message(message_id).await? {
            // 更新缓存
            self.cache
                .cache_message(message_id.to_string(), message.clone());
            Ok(Some(message))
        } else {
            Ok(None)
        }
    }

    async fn get_messages(
        &self,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<Message>> {
        // 先检查缓存
        if let Some(messages) = self.cache.get_message_list(session_id, cursor.as_ref()) {
            // 如果缓存的消息数量足够，直接返回
            if messages.len() >= limit {
                return Ok(messages.into_iter().take(limit).collect());
            }
        }

        // 缓存未命中或数量不足，从存储查询
        let messages = self
            .storage
            .get_messages(session_id, limit, cursor.clone())
            .await?;

        // 更新缓存
        if !messages.is_empty() {
            self.cache
                .cache_message_list(session_id.to_string(), cursor, messages.clone());
        }

        Ok(messages)
    }

    async fn save_session(&self, session: &SessionSummary) -> Result<()> {
        self.storage.save_session(session).await?;
        // 更新缓存
        self.cache
            .cache_session(session.session_id.clone(), session.clone());
        // 清除会话列表缓存（因为列表可能已变化）
        self.cache.session_list_cache.clear();
        Ok(())
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<SessionSummary>> {
        // 先检查缓存
        if let Some(session) = self.cache.get_session(session_id) {
            return Ok(Some(session));
        }

        // 缓存未命中，从存储查询
        if let Some(session) = self.storage.get_session(session_id).await? {
            // 更新缓存
            self.cache
                .cache_session(session_id.to_string(), session.clone());
            Ok(Some(session))
        } else {
            Ok(None)
        }
    }

    // 其他方法直接委托给底层存储
    async fn batch_save_messages(&self, messages: &[Message]) -> Result<()> {
        self.storage.batch_save_messages(messages).await?;
        // 更新缓存
        for message in messages {
            self.cache
                .cache_message(message.id.clone(), message.clone());
        }
        Ok(())
    }

    async fn batch_get_messages(&self, message_ids: &[String]) -> Result<Vec<Message>> {
        // 先检查缓存
        let mut cached_messages = Vec::new();
        let mut uncached_ids = Vec::new();

        for message_id in message_ids {
            if let Some(message) = self.cache.get_message(message_id) {
                cached_messages.push(message);
            } else {
                uncached_ids.push(message_id.clone());
            }
        }

        // 从存储查询未缓存的
        if !uncached_ids.is_empty() {
            let storage_messages = self.storage.batch_get_messages(&uncached_ids).await?;
            // 更新缓存
            for message in &storage_messages {
                self.cache
                    .cache_message(message.id.clone(), message.clone());
            }
            cached_messages.extend(storage_messages);
        }

        Ok(cached_messages)
    }

    // 其他方法直接委托（保持接口完整性）
    async fn get_messages_by_seq(
        &self,
        session_id: &str,
        after_seq: i64,
        limit: usize,
    ) -> Result<Vec<Message>> {
        self.storage
            .get_messages_by_seq(session_id, after_seq, limit)
            .await
    }

    async fn get_max_seq(&self, session_id: &str) -> Result<Option<i64>> {
        self.storage.get_max_seq(session_id).await
    }

    async fn delete_message(&self, message_id: &str) -> Result<()> {
        self.storage.delete_message(message_id).await?;
        // 清除缓存
        self.cache.invalidate_message(message_id);
        Ok(())
    }

    async fn get_sessions(
        &self,
        filter: crate::infrastructure::storage::SessionFilter,
    ) -> Result<Vec<SessionSummary>> {
        // 生成缓存键
        let filter_key = format!("{:?}", filter);

        // 先检查缓存
        if let Some(sessions) = self.cache.get_session_list(&filter_key) {
            return Ok(sessions);
        }

        // 缓存未命中，从存储查询
        let sessions = self.storage.get_sessions(filter).await?;

        // 更新缓存
        if !sessions.is_empty() {
            self.cache.cache_session_list(filter_key, sessions.clone());
        }

        Ok(sessions)
    }

    async fn update_session(
        &self,
        session_id: &str,
        updates: crate::infrastructure::storage::SessionUpdate,
    ) -> Result<()> {
        self.storage.update_session(session_id, updates).await?;
        // 清除会话缓存
        self.cache.invalidate_session(session_id);
        Ok(())
    }

    async fn delete_session(&self, session_id: &str) -> Result<()> {
        self.storage.delete_session(session_id).await?;
        // 清除缓存
        self.cache.invalidate_session(session_id);
        Ok(())
    }

    async fn save_sync_cursor(
        &self,
        session_id: &str,
        cursor: &crate::domain::sync::SyncCursor,
    ) -> Result<()> {
        self.storage.save_sync_cursor(session_id, cursor).await
    }

    async fn get_sync_cursor(&self, session_id: &str) -> Result<Option<crate::domain::SyncCursor>> {
        self.storage.get_sync_cursor(session_id).await
    }

    async fn get_all_sync_cursors(&self) -> Result<Vec<crate::domain::SyncCursor>> {
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

impl crate::infrastructure::storage::storage_trait::StorageSyncBounds for CachedStorageBackend {}
