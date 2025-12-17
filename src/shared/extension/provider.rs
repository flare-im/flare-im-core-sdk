//! 扩展提供者实现
//!
//! 提供扩展信息的获取和缓存机制

use crate::domain::extension::{
    ExtensionCache, ExtensionProvider, SessionExtension, UserExtension,
};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 扩展管理器
///
/// 负责管理和填充消息、会话的扩展信息
pub struct ExtensionManager {
    /// 扩展提供者列表（按优先级排序）
    providers: Arc<tokio::sync::RwLock<Vec<Arc<dyn ExtensionProvider>>>>,

    /// 本地缓存（可选）
    cache: Option<Arc<dyn ExtensionCache>>,
}

impl ExtensionManager {
    /// 创建新的扩展管理器
    pub fn new() -> Self {
        Self {
            providers: Arc::new(tokio::sync::RwLock::new(vec![])),
            cache: None,
        }
    }

    /// 设置缓存
    pub fn with_cache(mut self, cache: Arc<dyn ExtensionCache>) -> Self {
        self.cache = Some(cache);
        self
    }

    /// 添加扩展提供者（按优先级顺序添加）
    pub async fn add_provider(&self, provider: Arc<dyn ExtensionProvider>) {
        let mut providers = self.providers.write().await;
        providers.push(provider);
    }

    /// 填充消息扩展信息
    ///
    /// 从缓存或提供者获取用户扩展信息，填充到消息中
    pub async fn enrich_message(&self, message: &mut crate::domain::ExtendedMessage) -> Result<()> {
        let user_id = &message.message.sender_id;

        // 1. 从缓存获取
        if let Some(cache) = &self.cache {
            if let Some(ext) = cache.get_user_extension(user_id).await? {
                message.extension.sender_avatar = ext.avatar;
                message.extension.sender_name = ext.name;
                return Ok(()); // 缓存命中，直接返回
            }
        }

        // 2. 从提供者获取
        let providers = self.providers.read().await;
        for provider in providers.iter() {
            if let Some(ext) = provider.get_user_extension(user_id).await? {
                message.extension.sender_avatar = ext
                    .avatar
                    .clone()
                    .or(message.extension.sender_avatar.clone());
                message.extension.sender_name =
                    ext.name.clone().or(message.extension.sender_name.clone());

                // 更新缓存
                if let Some(cache) = &self.cache {
                    let _ = cache.save_user_extension(user_id, &ext).await;
                }

                break; // 使用第一个提供者的结果
            }
        }

        Ok(())
    }

    /// 填充会话扩展信息
    pub async fn enrich_session(
        &self,
        session: &mut crate::domain::ExtendedSessionSummary,
    ) -> Result<()> {
        let session_id = &session.session.session_id;

        // 1. 从缓存获取
        if let Some(cache) = &self.cache {
            if let Some(ext) = cache.get_session_extension(session_id).await? {
                session.extension.avatar = ext.avatar;
                session.extension.display_name = ext.display_name;
                session.extension.is_pinned = ext.is_pinned;
                session.extension.is_muted = ext.is_muted;
                return Ok(()); // 缓存命中，直接返回
            }
        }

        // 2. 从提供者获取
        let providers = self.providers.read().await;
        for provider in providers.iter() {
            if let Some(ext) = provider.get_session_extension(session_id).await? {
                session.extension.avatar = ext.avatar.clone().or(session.extension.avatar.clone());
                session.extension.display_name = ext
                    .display_name
                    .clone()
                    .or(session.extension.display_name.clone());
                session.extension.is_pinned = ext.is_pinned;
                session.extension.is_muted = ext.is_muted;

                // 更新缓存
                if let Some(cache) = &self.cache {
                    let _ = cache.save_session_extension(session_id, &ext).await;
                }

                break; // 使用第一个提供者的结果
            }
        }

        Ok(())
    }

    /// 批量填充消息扩展信息
    ///
    /// 更高效的方式，批量获取用户扩展信息
    ///
    /// 优化：
    /// - 使用 HashMap 优化查找（O(1) 复杂度）
    /// - 批量查询减少网络/IO 开销
    /// - 缓存优先，减少重复查询
    pub async fn batch_enrich_messages(
        &self,
        messages: &mut [crate::domain::ExtendedMessage],
    ) -> Result<()> {
        // 收集所有需要查询的 user_id（去重，优化：直接使用 HashSet，预分配容量）
        let user_ids_set: std::collections::HashSet<String> = messages
            .iter()
            .map(|m| m.message.sender_id.clone())
            .collect();

        if user_ids_set.is_empty() {
            return Ok(());
        }

        let user_ids: Vec<String> = user_ids_set.into_iter().collect();

        // 1. 从缓存批量获取（优化：并行查询，预分配 HashMap 容量）
        let mut cached_extensions = std::collections::HashMap::with_capacity(user_ids.len());
        if let Some(cache) = &self.cache {
            // 并行查询缓存（提升性能）
            #[cfg(not(target_arch = "wasm32"))]
            use tokio::spawn as tokio_spawn;
            #[cfg(target_arch = "wasm32")]
            use tokio::task::spawn_local as tokio_spawn;

            let cache_tasks: Vec<_> = user_ids
                .iter()
                .map(|user_id| {
                    let cache = Arc::clone(cache);
                    let user_id = user_id.clone();
                    tokio_spawn(async move {
                        cache
                            .get_user_extension(&user_id)
                            .await
                            .ok()
                            .flatten()
                            .map(|ext| (user_id, ext))
                    })
                })
                .collect();

            // 等待所有缓存查询完成
            for task in cache_tasks {
                if let Ok(Some((user_id, ext))) = task.await {
                    cached_extensions.insert(user_id, ext);
                }
            }
        }

        // 2. 找出缓存未命中的 user_id
        let uncached_user_ids: Vec<String> = user_ids
            .into_iter()
            .filter(|id| !cached_extensions.contains_key(id))
            .collect();

        // 3. 从提供者批量获取（优化：使用 HashMap 存储结果，预分配容量）
        let mut provider_extensions =
            std::collections::HashMap::with_capacity(uncached_user_ids.len());
        if !uncached_user_ids.is_empty() {
            let providers = self.providers.read().await;
            for provider in providers.iter() {
                if let Ok(extensions) = provider.batch_get_user_extensions(&uncached_user_ids).await
                {
                    // 直接转换为 HashMap，提升查找性能
                    for (user_id, ext) in extensions {
                        provider_extensions.insert(user_id, ext);
                    }
                    if !provider_extensions.is_empty() {
                        break; // 使用第一个提供者的结果
                    }
                }
            }
        }

        // 4. 更新缓存（优化：并行更新）
        if let Some(cache) = &self.cache {
            let cache_tasks: Vec<_> = provider_extensions
                .iter()
                .map(|(user_id, ext)| {
                    let cache = Arc::clone(cache);
                    let user_id = user_id.clone();
                    let ext = ext.clone();
                    async move {
                        let _ = cache.save_user_extension(&user_id, &ext).await;
                    }
                })
                .collect();

            // 并行更新缓存（不等待结果，fire-and-forget）
            for task in cache_tasks {
                tokio::spawn(task);
            }
        }

        // 5. 合并所有扩展信息（优化：使用 HashMap 合并，预分配容量）
        let total_count = cached_extensions.len() + provider_extensions.len();
        let mut all_extensions = std::collections::HashMap::with_capacity(total_count);
        all_extensions.extend(cached_extensions);
        all_extensions.extend(provider_extensions);

        // 6. 填充到消息中（优化：O(n) 复杂度，使用 HashMap 查找）
        for message in messages {
            if let Some(ext) = all_extensions.get(&message.message.sender_id) {
                message.extension.sender_avatar = ext
                    .avatar
                    .clone()
                    .or(message.extension.sender_avatar.clone());
                message.extension.sender_name =
                    ext.name.clone().or(message.extension.sender_name.clone());
            }
        }

        Ok(())
    }

    /// 批量填充会话扩展信息
    pub async fn batch_enrich_sessions(
        &self,
        sessions: &mut [crate::domain::ExtendedSessionSummary],
    ) -> Result<()> {
        // 类似逻辑...
        for session in sessions {
            self.enrich_session(session).await?;
        }
        Ok(())
    }
}

impl Default for ExtensionManager {
    fn default() -> Self {
        Self::new()
    }
}
