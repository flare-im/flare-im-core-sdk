//! 扩展提供者实现
//!
//! 提供多种扩展信息获取方式

use crate::model::extension::{
    ExtensionProvider, ExtensionCache,
    UserExtension, SessionExtension,
};
use crate::storage::StorageBackend;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

/// 基于存储的扩展提供者
/// 
/// 从本地存储（SQLite/IndexedDB）读取扩展信息
pub struct StorageExtensionProvider {
    storage: Arc<dyn StorageBackend>,
}

impl StorageExtensionProvider {
    /// 创建新的存储扩展提供者
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self { storage }
    }
}

#[async_trait]
impl ExtensionProvider for StorageExtensionProvider {
    async fn get_user_extension(&self, user_id: &str) -> Result<Option<UserExtension>> {
        // 存储层不直接存储用户扩展信息
        // 这里可以从消息的扩展信息中提取
        Ok(None)
    }
    
    async fn get_session_extension(&self, session_id: &str) -> Result<Option<SessionExtension>> {
        self.storage.get_session_extension(session_id).await
    }
    
    async fn batch_get_user_extensions(
        &self,
        _user_ids: &[String],
    ) -> Result<Vec<(String, UserExtension)>> {
        Ok(vec![])
    }
}

/// 内存扩展提供者
/// 
/// 用于测试或临时存储扩展信息
pub struct MemoryExtensionProvider {
    user_extensions: Arc<tokio::sync::RwLock<std::collections::HashMap<String, UserExtension>>>,
    session_extensions: Arc<tokio::sync::RwLock<std::collections::HashMap<String, SessionExtension>>>,
}

impl MemoryExtensionProvider {
    /// 创建新的内存扩展提供者
    pub fn new() -> Self {
        Self {
            user_extensions: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            session_extensions: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }
    
    /// 设置用户扩展信息
    pub async fn set_user_extension(&self, user_id: String, extension: UserExtension) {
        let mut extensions = self.user_extensions.write().await;
        extensions.insert(user_id, extension);
    }
    
    /// 设置会话扩展信息
    pub async fn set_session_extension(&self, session_id: String, extension: SessionExtension) {
        let mut extensions = self.session_extensions.write().await;
        extensions.insert(session_id, extension);
    }
    
    /// 清除所有扩展信息
    pub async fn clear(&self) {
        let mut user_exts = self.user_extensions.write().await;
        let mut session_exts = self.session_extensions.write().await;
        user_exts.clear();
        session_exts.clear();
    }
}

impl Default for MemoryExtensionProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExtensionProvider for MemoryExtensionProvider {
    async fn get_user_extension(&self, user_id: &str) -> Result<Option<UserExtension>> {
        let extensions = self.user_extensions.read().await;
        Ok(extensions.get(user_id).cloned())
    }
    
    async fn get_session_extension(&self, session_id: &str) -> Result<Option<SessionExtension>> {
        let extensions = self.session_extensions.read().await;
        Ok(extensions.get(session_id).cloned())
    }
    
    async fn batch_get_user_extensions(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<(String, UserExtension)>> {
        let extensions = self.user_extensions.read().await;
        let mut results = Vec::new();
        for user_id in user_ids {
            if let Some(ext) = extensions.get(user_id) {
                results.push((user_id.clone(), ext.clone()));
            }
        }
        Ok(results)
    }
}

/// 基于存储的扩展缓存实现
/// 
/// 将扩展信息缓存到本地存储
pub struct StorageExtensionCache {
    storage: Arc<dyn StorageBackend>,
}

impl StorageExtensionCache {
    /// 创建新的存储扩展缓存
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self { storage }
    }
}

#[async_trait]
impl ExtensionCache for StorageExtensionCache {
    async fn get_user_extension(&self, user_id: &str) -> Result<Option<UserExtension>> {
        // 存储层不直接存储用户扩展信息
        // 可以从消息扩展信息中提取，这里简化处理
        Ok(None)
    }
    
    async fn save_user_extension(&self, user_id: &str, extension: &UserExtension) -> Result<()> {
        // 存储层不直接存储用户扩展信息
        // 可以存储到消息扩展信息中，这里简化处理
        Ok(())
    }
    
    async fn get_session_extension(&self, session_id: &str) -> Result<Option<SessionExtension>> {
        self.storage.get_session_extension(session_id).await
    }
    
    async fn save_session_extension(
        &self,
        session_id: &str,
        extension: &SessionExtension,
    ) -> Result<()> {
        self.storage.save_session_extension(session_id, extension).await
    }
}

/// 内存扩展缓存实现
/// 
/// 将扩展信息缓存在内存中（带 TTL）
pub struct MemoryExtensionCache {
    user_cache: Arc<tokio::sync::RwLock<std::collections::HashMap<String, (UserExtension, i64)>>>,
    session_cache: Arc<tokio::sync::RwLock<std::collections::HashMap<String, (SessionExtension, i64)>>>,
    ttl_seconds: i64,
}

impl MemoryExtensionCache {
    /// 创建新的内存扩展缓存
    /// 
    /// # 参数
    /// - `ttl_seconds`: 缓存过期时间（秒），默认 300 秒（5分钟）
    pub fn new(ttl_seconds: i64) -> Self {
        Self {
            user_cache: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            session_cache: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            ttl_seconds,
        }
    }
    
    /// 清理过期的缓存项
    #[allow(dead_code)]
    async fn cleanup_expired(&self) {
        let now = chrono::Utc::now().timestamp();
        let ttl = self.ttl_seconds;
        
        // 清理用户缓存
        {
            let mut cache = self.user_cache.write().await;
            cache.retain(|_, (_, timestamp)| now - *timestamp < ttl);
        }
        
        // 清理会话缓存
        {
            let mut cache = self.session_cache.write().await;
            cache.retain(|_, (_, timestamp)| now - *timestamp < ttl);
        }
    }
}

impl Default for MemoryExtensionCache {
    fn default() -> Self {
        Self::new(300) // 默认 5 分钟
    }
}

#[async_trait]
impl ExtensionCache for MemoryExtensionCache {
    async fn get_user_extension(&self, user_id: &str) -> Result<Option<UserExtension>> {
        let now = chrono::Utc::now().timestamp();
        let cache = self.user_cache.read().await;
        
        if let Some((ext, timestamp)) = cache.get(user_id) {
            if now - timestamp < self.ttl_seconds {
                return Ok(Some(ext.clone()));
            }
        }
        
        Ok(None)
    }
    
    async fn save_user_extension(&self, user_id: &str, extension: &UserExtension) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        let mut cache = self.user_cache.write().await;
        cache.insert(user_id.to_string(), (extension.clone(), now));
        Ok(())
    }
    
    async fn get_session_extension(&self, session_id: &str) -> Result<Option<SessionExtension>> {
        let now = chrono::Utc::now().timestamp();
        let cache = self.session_cache.read().await;
        
        if let Some((ext, timestamp)) = cache.get(session_id) {
            if now - timestamp < self.ttl_seconds {
                return Ok(Some(ext.clone()));
            }
        }
        
        Ok(None)
    }
    
    async fn save_session_extension(
        &self,
        session_id: &str,
        extension: &SessionExtension,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        let mut cache = self.session_cache.write().await;
        cache.insert(session_id.to_string(), (extension.clone(), now));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_memory_extension_provider() {
        let provider = MemoryExtensionProvider::new();
        
        let user_ext = UserExtension {
            avatar: Some("https://example.com/avatar.jpg".to_string()),
            name: Some("Test User".to_string()),
            online_status: Some("online".to_string()),
            custom: std::collections::HashMap::new(),
        };
        
        provider.set_user_extension("user_123".to_string(), user_ext.clone()).await;
        
        let result = provider.get_user_extension("user_123").await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, Some("Test User".to_string()));
    }
    
    #[tokio::test]
    async fn test_memory_extension_cache() {
        let cache = MemoryExtensionCache::new(60); // 1分钟TTL
        
        let user_ext = UserExtension {
            avatar: Some("https://example.com/avatar.jpg".to_string()),
            name: Some("Test User".to_string()),
            online_status: None,
            custom: std::collections::HashMap::new(),
        };
        
        cache.save_user_extension("user_123", &user_ext).await.unwrap();
        
        let result = cache.get_user_extension("user_123").await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, Some("Test User".to_string()));
    }
}

