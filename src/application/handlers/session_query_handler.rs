//! 会话查询处理器

use crate::application::queries::session::*;
use crate::domain::session::SessionSummary;
use crate::domain::session::repository::SessionRepository;
use anyhow::{Context, Result};
use std::sync::Arc;

/// 会话查询处理器
///
/// 处理会话相关的查询（获取会话列表、查找会话等）
///
/// 生产级特性：
/// - 查询结果缓存（减少数据库查询）
/// - 批量查询优化
pub struct SessionQueryHandler {
    repository: Arc<dyn SessionRepository>,
    /// 会话缓存（用于快速访问，减少数据库查询）
    cache: Arc<
        tokio::sync::RwLock<
            std::collections::HashMap<String, (SessionSummary, std::time::Instant)>,
        >,
    >,
    /// 缓存 TTL（默认 5 分钟）
    cache_ttl: std::time::Duration,
}

impl SessionQueryHandler {
    pub fn new(repository: Arc<dyn SessionRepository>) -> Self {
        Self {
            repository,
            cache: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            cache_ttl: std::time::Duration::from_secs(300), // 5 分钟
        }
    }

    /// 从缓存获取会话（如果存在且未过期）
    async fn get_from_cache(&self, session_id: &str) -> Option<SessionSummary> {
        let cache = self.cache.read().await;
        if let Some((summary, cached_at)) = cache.get(session_id) {
            if cached_at.elapsed() < self.cache_ttl {
                return Some(summary.clone());
            }
        }
        None
    }

    /// 更新缓存
    async fn update_cache(&self, session_id: String, summary: SessionSummary) {
        let mut cache = self.cache.write().await;
        cache.insert(session_id, (summary, std::time::Instant::now()));

        // 清理过期缓存（如果缓存太大）
        if cache.len() > 1000 {
            cache.retain(|_, (_, cached_at)| cached_at.elapsed() < self.cache_ttl);
        }
    }

    /// 处理获取会话列表查询
    pub async fn handle_get_sessions(
        &self,
        query: GetSessionsQuery,
    ) -> Result<Vec<SessionSummary>> {
        // TODO: SessionRepository 需要支持 SessionFilter
        // 当前暂时使用 find_all，后续需要扩展 Repository 接口
        let sessions = self
            .repository
            .find_all(None)
            .await
            .context("Failed to get sessions")?;

        // 转换为 SessionSummary（从 ProtoSessionSummary 转换）
        Ok(sessions
            .into_iter()
            .map(|s| SessionSummary::from(s.to_proto()))
            .collect())
    }

    /// 处理分页获取会话列表查询
    pub async fn handle_get_sessions_paginated(
        &self,
        query: GetSessionsPaginatedQuery,
    ) -> Result<(Vec<SessionSummary>, Option<String>)> {
        // TODO: 实现分页逻辑
        let filter = query.filter.unwrap_or_default();
        let all_sessions = self
            .handle_get_sessions(GetSessionsQuery { filter })
            .await?;

        // 简化实现：基于游标分页
        let start_index = if let Some(cursor_str) = query.cursor {
            if let Some(colon_pos) = cursor_str.find(':') {
                all_sessions
                    .iter()
                    .position(|s| s.session_id == cursor_str[colon_pos + 1..])
                    .unwrap_or(0)
            } else {
                0
            }
        } else {
            0
        };

        let end_index = (start_index + query.limit).min(all_sessions.len());
        let page_sessions = all_sessions[start_index..end_index].to_vec();

        let next_cursor = if end_index < all_sessions.len() {
            if let Some(last_session) = page_sessions.last() {
                let timestamp = last_session.updated_at.unwrap_or(0);
                Some(format!(
                    "timestamp:{}:{}",
                    timestamp, last_session.session_id
                ))
            } else {
                None
            }
        } else {
            None
        };

        Ok((page_sessions, next_cursor))
    }

    /// 处理获取会话查询（生产级实现：带缓存）
    pub async fn handle_get_session(
        &self,
        query: GetSessionQuery,
    ) -> Result<Option<SessionSummary>> {
        let session_id_str = query.session_id.to_string();

        // 1. 先检查缓存
        if let Some(cached_summary) = self.get_from_cache(&session_id_str).await {
            return Ok(Some(cached_summary));
        }

        // 2. 从数据库查询
        let session = self
            .repository
            .find_by_id(&query.session_id)
            .await
            .context("Failed to get session")?;

        if let Some(session) = session {
            let summary = SessionSummary::from(session.to_proto());

            // 3. 更新缓存
            self.update_cache(session_id_str, summary.clone()).await;

            Ok(Some(summary))
        } else {
            Ok(None)
        }
    }

    /// 处理批量获取会话查询
    pub async fn handle_get_sessions_batch(
        &self,
        query: GetSessionsBatchQuery,
    ) -> Result<Vec<SessionSummary>> {
        let mut results = Vec::new();
        for session_id in query.session_ids {
            if let Ok(Some(session)) = self.repository.find_by_id(&session_id).await {
                results.push(SessionSummary::from(session.to_proto()));
            }
        }
        Ok(results)
    }

    /// 处理查找会话 ID 查询
    pub async fn handle_find_session_id(
        &self,
        query: FindSessionIdQuery,
    ) -> Result<Option<String>> {
        // 根据规则生成会话 ID
        let expected_session_id = format!(
            "{}:{}:{}",
            query.session_type, query.business_type, query.target_id
        );

        // 查询是否存在
        let filter = crate::infrastructure::storage::SessionFilter::default();
        let sessions = self
            .handle_get_sessions(GetSessionsQuery { filter })
            .await?;

        if sessions.iter().any(|s| s.session_id == expected_session_id) {
            Ok(Some(expected_session_id))
        } else {
            Ok(None)
        }
    }

    /// 处理获取总未读数查询
    pub async fn handle_get_total_unread_count(
        &self,
        _query: GetTotalUnreadCountQuery,
    ) -> Result<u32> {
        let filter = crate::infrastructure::storage::SessionFilter::default();
        let sessions = self
            .handle_get_sessions(GetSessionsQuery { filter })
            .await?;
        let total_unread: u32 = sessions.iter().map(|s| s.unread_count).sum();
        Ok(total_unread)
    }

    /// 处理获取草稿查询
    ///
    /// 按照微信/Telegram/飞书标准：草稿存储在会话的 metadata 中
    pub async fn handle_get_draft(&self, query: GetDraftQuery) -> Result<Option<String>> {
        // 从 SessionSummary 的 metadata 中提取 draft
        let session = self
            .repository
            .find_by_id(&query.session_id)
            .await
            .context("Failed to find session")?;

        if let Some(session) = session {
            let summary = session.to_summary();
            if let Some(draft) = summary.metadata.get("draft") {
                if !draft.is_empty() {
                    return Ok(Some(draft.clone()));
                }
            }
        }

        Ok(None)
    }

    /// 处理获取输入状态查询
    ///
    /// 按照微信/Telegram/飞书标准：输入状态是临时状态，不持久化
    /// 这里返回 false，实际状态应该通过事件总线监听 SessionTypingSent 事件
    pub async fn handle_get_typing_status(&self, _query: GetTypingStatusQuery) -> Result<bool> {
        // 输入状态是临时状态，不持久化到存储
        // 客户端应该通过事件总线监听 SessionTypingSent 事件来获取实时状态
        // 这里返回 false 作为默认值
        Ok(false)
    }
}
