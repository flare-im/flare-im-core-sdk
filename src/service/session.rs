//! 会话服务
//!
//! 提供会话列表查询、会话详情查询、未读数计算等核心功能

use crate::connection::ConnectionManager;
use crate::event::{Event, EventBus, SessionEvent};
use crate::model::{SessionSummary, ExtendedSessionSummary, SyncCursor};
#[cfg(feature = "extensions")]
use crate::extension::ExtensionInfoManager as ExtensionManager;
#[cfg(not(feature = "extensions"))]
type ExtensionManager = ();
use crate::storage::{StorageBackend, SessionFilter, SessionUpdate};
use crate::service::sync::SyncService;
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::RwLock;
#[cfg(target_arch = "wasm32")]
use tokio::task::spawn_local as tokio_spawn;
#[cfg(not(target_arch = "wasm32"))]
use tokio::spawn as tokio_spawn;
use tracing::{debug, info};

/// 会话同步结果
#[derive(Debug, Clone)]
pub struct SessionSyncResult {
    /// 同步的会话列表
    pub sessions: Vec<SessionSummary>,
    
    /// 是否有更多会话
    pub has_more: bool,
    
    /// 下一个游标
    pub next_cursor: Option<String>,
    
    /// 同步的会话数量
    pub count: usize,
}

/// 会话服务
/// 
/// 负责会话的查询、更新、未读数管理等
pub struct SessionService {
    /// 连接管理器（通过长连接发送请求）
    connection: Arc<ConnectionManager>,
    
    /// 本地存储
    storage: Arc<dyn StorageBackend>,
    
    /// 同步服务（用于同步会话列表）
    sync_service: Arc<SyncService>,
    
    /// 事件总线
    event_bus: Arc<EventBus>,
    
    /// 当前用户 ID
    user_id: Arc<RwLock<String>>,
    
    /// 扩展管理器（用于填充扩展信息）
    /// 如果启用了 extensions feature，则必需；否则使用空实现
    #[cfg(feature = "extensions")]
    extension_manager: Arc<ExtensionManager>,
}

impl SessionService {
    /// 创建新的会话服务实例
    pub fn new(
        connection: Arc<ConnectionManager>,
        storage: Arc<dyn StorageBackend>,
        sync_service: Arc<SyncService>,
        event_bus: Arc<EventBus>,
        user_id: Arc<RwLock<String>>,
    ) -> Self {
        Self {
            connection,
            storage,
            sync_service,
            event_bus,
            user_id,
            #[cfg(feature = "extensions")]
            extension_manager: Arc::new(ExtensionManager::new()),
        }
    }
    
    /// 设置扩展管理器（用于填充扩展信息）
    #[cfg(feature = "extensions")]
    pub fn with_extension_manager(mut self, extension_manager: Arc<ExtensionManager>) -> Self {
        self.extension_manager = extension_manager;
        self
    }

    /// 获取会话列表（返回基础 SessionSummary）
    /// 
    /// # 参数
    /// - `filter`: 会话过滤条件
    /// 
    /// # 返回
    /// - `Result<Vec<SessionSummary>>`: 会话列表
    pub async fn get_sessions(&self, filter: SessionFilter) -> Result<Vec<SessionSummary>> {
        debug!(
            session_type = ?filter.session_type,
            business_type = ?filter.business_type,
            unread_only = filter.unread_only,
            "Getting sessions with filter"
        );
        
        let sessions = self.storage.get_sessions(filter).await
            .context("Failed to get sessions from storage")?;
        
        info!(count = sessions.len(), "Retrieved sessions from storage");
        Ok(sessions)
    }
    
    /// 获取会话列表（返回带扩展信息的 ExtendedSessionSummary）
    /// 
    /// # 参数
    /// - `filter`: 会话过滤条件
    /// 
    /// # 返回
    /// - `Result<Vec<ExtendedSessionSummary>>`: 带扩展信息的会话列表
    pub async fn get_sessions_extended(&self, filter: SessionFilter) -> Result<Vec<ExtendedSessionSummary>> {
        // 1. 获取基础会话
        let sessions = self.get_sessions(filter).await?;
        
        // 2. 转换为 ExtendedSessionSummary，并加载扩展信息
        let mut extended_sessions: Vec<ExtendedSessionSummary> = Vec::with_capacity(sessions.len());
        for session in sessions {
            // 尝试从存储加载扩展信息
            let extension = self.storage.get_session_extension(&session.session_id).await
                .unwrap_or_else(|_| None)
                .unwrap_or_default();
            extended_sessions.push(ExtendedSessionSummary::new(session, extension));
        }
        
        // 3. 批量填充扩展信息
        #[cfg(feature = "extensions")]
        {
            self.extension_manager.batch_enrich_sessions(&mut extended_sessions).await
                .context("Failed to enrich sessions with extension info")?;
            
            // 4. 保存填充后的扩展信息到存储（优化：并行保存，不阻塞）
            let storage_clone = Arc::clone(&self.storage);
            for session in extended_sessions.iter() {
                let session_id = session.session.session_id.clone();
                let ext = session.extension.clone();
                let storage = Arc::clone(&storage_clone);
                tokio_spawn(async move {
                    if let Err(e) = storage.save_session_extension(&session_id, &ext).await {
                        tracing::warn!(error = %e, session_id = %session_id, "Failed to save session extension");
                    }
                });
            }
        }
        
        Ok(extended_sessions)
    }

    /// 获取会话详情（返回基础 SessionSummary）
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID
    /// 
    /// # 返回
    /// - `Result<SessionSummary>`: 会话详情
    pub async fn get_session(&self, session_id: &str) -> Result<SessionSummary> {
        debug!(session_id = %session_id, "Getting session details");
        
        // 优化：使用 with_context 提供更好的错误信息
        let session = self.storage.get_session(session_id).await?
            .with_context(|| format!("Session not found: {}", session_id))?;
        
        Ok(session)
    }
    
    /// 获取会话详情（返回带扩展信息的 ExtendedSessionSummary）
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID
    /// 
    /// # 返回
    /// - `Result<ExtendedSessionSummary>`: 带扩展信息的会话详情
    pub async fn get_session_extended(&self, session_id: &str) -> Result<ExtendedSessionSummary> {
        // 1. 获取基础会话
        let session = self.get_session(session_id).await?;
        
        // 2. 加载扩展信息
        let extension = self.storage.get_session_extension(session_id).await
            .unwrap_or_else(|_| None)
            .unwrap_or_default();
        
        let mut extended_session = ExtendedSessionSummary::new(session, extension);
        
        // 3. 填充扩展信息
        #[cfg(feature = "extensions")]
        {
            self.extension_manager.enrich_session(&mut extended_session).await
                .context("Failed to enrich session with extension info")?;
            
            // 4. 保存扩展信息（优化：异步保存，不阻塞）
            let storage_clone = Arc::clone(&self.storage);
            let session_id_owned = session_id.to_string();
            let ext = extended_session.extension.clone();
            tokio_spawn(async move {
                if let Err(e) = storage_clone.save_session_extension(&session_id_owned, &ext).await {
                    tracing::warn!(error = %e, session_id = %session_id_owned, "Failed to save session extension");
                }
            });
        }
        
        Ok(extended_session)
    }

    /// 创建会话（本地）
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID（如果为空则自动生成）
    /// - `session_type`: 会话类型
    /// - `business_type`: 业务类型
    /// - `display_name`: 显示名称
    /// 
    /// # 返回
    /// - `Result<String>`: 会话 ID
    pub async fn create_session(
        &self,
        session_id: Option<String>,
        session_type: String,
        business_type: String,
        display_name: Option<String>,
    ) -> Result<String> {
        let session_id = session_id.unwrap_or_else(|| {
            // 生成临时会话 ID（实际应该由服务端生成）
            {
                #[cfg(target_arch = "wasm32")]
                {
                    use std::sync::atomic::{AtomicU64, Ordering};
                    static COUNTER: AtomicU64 = AtomicU64::new(0);
                    // 使用 chrono 获取时间戳，避免 unwrap
                    let ts = chrono::Utc::now().timestamp_millis();
                    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
                    format!("temp-{}-{}", ts, c)
                }
                #[cfg(not(target_arch = "wasm32"))]
                { format!("temp-{}", uuid::Uuid::new_v4()) }
            }
        });
        
        debug!(
            session_id = %session_id,
            session_type = %session_type,
            "Creating local session"
        );
        
        let mut session = SessionSummary {
            session_id: session_id.clone(),
            session_type: session_type.clone(),
            business_type,
            display_name,
            last_message_id: None,
            last_message_time: None,
            last_sender_id: None,
            last_message_type: 0,
            last_content_type: String::new(),
            unread_count: 0,
            metadata: std::collections::HashMap::new(),
            server_cursor_ts: None,
        };
        if let Some(ref name) = session.display_name {
            // 将显示名作为单聊对端ID的来源写入元数据，便于路由与推送
            session.metadata.insert("peer_id".to_string(), name.clone());
        }
        // 单聊场景：确保类型为 single
        if session_type == "single" {
            session.metadata.insert("participants".to_string(), format!("{},{}", self.user_id.read().await.clone(), session.metadata.get("peer_id").cloned().unwrap_or_default()));
        }
        
        self.storage.save_session(&session).await
            .context("Failed to save session to storage")?;
        
        // 发布会话创建事件
        self.event_bus.publish(Event::Session(SessionEvent::SessionCreated {
            session_id: session_id.clone(),
        }));
        
        info!(session_id = %session_id, "Session created");
        Ok(session_id)
    }

    /// 更新会话信息
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `updates`: 更新内容
    pub async fn update_session(
        &self,
        session_id: &str,
        updates: SessionUpdate,
    ) -> Result<()> {
        debug!(session_id = %session_id, "Updating session");
        
        self.storage.update_session(session_id, updates).await
            .context("Failed to update session in storage")?;
        
        // 发布会话更新事件
        self.event_bus.publish(Event::Session(SessionEvent::SessionUpdated {
            session_id: session_id.to_string(),
        }));
        
        info!(session_id = %session_id, "Session updated");
        Ok(())
    }

    /// 获取未读数
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID
    /// 
    /// # 返回
    /// - `Result<i32>`: 未读数
    pub async fn get_unread_count(&self, session_id: &str) -> Result<i32> {
        let session = self.get_session(session_id).await?;
        Ok(session.unread_count)
    }

    pub async fn is_pinned(&self, session_id: &str) -> Result<bool> {
        let session = self.get_session(session_id).await?;
        Ok(session.metadata.get("pinned").map(|v| v == "1").unwrap_or(false))
    }

    pub async fn is_muted(&self, session_id: &str) -> Result<bool> {
        let session = self.get_session(session_id).await?;
        Ok(session.metadata.get("mute").map(|v| v == "1").unwrap_or(false))
    }

    pub async fn alert_mode(&self, session_id: &str) -> Result<Option<String>> {
        let session = self.get_session(session_id).await?;
        Ok(session.metadata.get("alert").cloned())
    }

    pub async fn increment_unread(&self, session_id: &str) -> Result<()> {
        // 优化：直接更新，避免先查询（如果存储层支持原子操作）
        // 当前实现需要先查询，但我们可以优化为直接更新
        let session = self.get_session(session_id).await?;
        let new_count = session.unread_count.saturating_add(1);
        let updates = SessionUpdate::new().with_unread_count(new_count);
        
        // 优化：并行执行存储更新和事件发布
        let event_bus_clone = Arc::clone(&self.event_bus);
        let session_id_clone = session_id.to_string();
        
        let (update_result, _) = tokio::join!(
            self.storage.update_session(session_id, updates),
            async move {
                event_bus_clone.publish(Event::Session(SessionEvent::UnreadCountChanged {
                    session_id: session_id_clone,
                    count: new_count,
                }));
            }
        );
        
        update_result?;
        Ok(())
    }

    /// 更新会话元数据（优化：合并多个元数据更新）
    async fn update_session_metadata(
        &self,
        session_id: &str,
        metadata_updates: std::collections::HashMap<String, String>,
    ) -> Result<()> {
        // 优化：先获取现有元数据，然后合并更新
        let session = self.get_session(session_id).await?;
        let mut meta = session.metadata.clone();
        meta.extend(metadata_updates);
        
        let updates = SessionUpdate::new().with_metadata(meta);
        self.storage.update_session(session_id, updates).await?;
        Ok(())
    }

    pub async fn set_pinned(&self, session_id: &str, pinned: bool) -> Result<()> {
        let mut meta = std::collections::HashMap::new();
        meta.insert("pinned".to_string(), if pinned { "1".to_string() } else { "0".to_string() });
        self.update_session_metadata(session_id, meta).await
    }

    pub async fn set_muted(&self, session_id: &str, muted: bool) -> Result<()> {
        let mut meta = std::collections::HashMap::new();
        meta.insert("mute".to_string(), if muted { "1".to_string() } else { "0".to_string() });
        self.update_session_metadata(session_id, meta).await
    }

    pub async fn set_alert_mode(&self, session_id: &str, mode: &str) -> Result<()> {
        let mut meta = std::collections::HashMap::new();
        meta.insert("alert".to_string(), mode.to_string());
        self.update_session_metadata(session_id, meta).await
    }
}

impl SessionService {
    pub async fn set_user_id(&self, user_id: String) {
        let mut guard = self.user_id.write().await;
        *guard = user_id;
    }

    pub async fn mark_as_read(
        &self,
        session_id: &str,
        message_seq: Option<i64>,
    ) -> Result<()> {
        let session = self.get_session(session_id).await?;
        if let Some(seq) = message_seq {
            let mut cursor = self.storage.get_sync_cursor(session_id).await?
                .unwrap_or_else(|| SyncCursor::new(session_id.to_string()));
            cursor.update(Some(seq), None, None);
            self.storage.save_sync_cursor(session_id, &cursor).await?;
        }
        let old_unread_count = session.unread_count;
        let updates = SessionUpdate::new().with_unread_count(0);
        self.storage.update_session(session_id, updates).await?;
        if old_unread_count != 0 {
            self.event_bus.publish(Event::Session(SessionEvent::UnreadCountChanged { session_id: session_id.to_string(), count: 0 }));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::storage_trait::StorageBackend;
    use async_trait::async_trait;
    
    // Mock storage for testing
    struct MockStorage;
    impl crate::storage::storage_trait::StorageSyncBounds for MockStorage {}
    
    #[async_trait]
    impl StorageBackend for MockStorage {
        async fn save_message(&self, _message: &crate::model::Message) -> Result<()> {
            Ok(())
        }
        
        async fn get_message(&self, _message_id: &str) -> Result<Option<crate::model::Message>> {
            Ok(None)
        }
        
        async fn get_messages(
            &self,
            _session_id: &str,
            _limit: usize,
            _cursor: Option<String>,
        ) -> Result<Vec<crate::model::Message>> {
            Ok(Vec::new())
        }
        
        async fn get_messages_by_seq(
            &self,
            _session_id: &str,
            _after_seq: i64,
            _limit: usize,
        ) -> Result<Vec<crate::model::Message>> {
            Ok(Vec::new())
        }
        
        async fn get_max_seq(&self, _session_id: &str) -> Result<Option<i64>> {
            Ok(None)
        }
        
        async fn delete_message(&self, _message_id: &str) -> Result<()> {
            Ok(())
        }
        
        async fn save_session(&self, _session: &SessionSummary) -> Result<()> {
            Ok(())
        }
        
        async fn get_session(&self, session_id: &str) -> Result<Option<SessionSummary>> {
            if session_id == "test-session" {
                Ok(Some(SessionSummary {
                    session_id: "test-session".to_string(),
                    session_type: "single".to_string(),
                    business_type: "chat".to_string(),
                    display_name: Some("Test Session".to_string()),
                    last_message_id: None,
                    last_message_time: None,
                    last_sender_id: None,
                    last_message_type: 0,
                    last_content_type: String::new(),
                    unread_count: 5,
                    metadata: std::collections::HashMap::new(),
                    server_cursor_ts: None,
                }))
            } else {
                Ok(None)
            }
        }
        
        async fn get_sessions(&self, _filter: SessionFilter) -> Result<Vec<SessionSummary>> {
            Ok(vec![
                SessionSummary {
                    session_id: "session-1".to_string(),
                    session_type: "single".to_string(),
                    business_type: "chat".to_string(),
                    display_name: None,
                    last_message_id: None,
                    last_message_time: None,
                    last_sender_id: None,
                    last_message_type: 0,
                    last_content_type: String::new(),
                    unread_count: 0,
                    metadata: std::collections::HashMap::new(),
                    server_cursor_ts: None,
                },
            ])
        }
        
        async fn update_session(
            &self,
            _session_id: &str,
            _updates: SessionUpdate,
        ) -> Result<()> {
            Ok(())
        }
        
        async fn delete_session(&self, _session_id: &str) -> Result<()> {
            Ok(())
        }
        
        async fn save_sync_cursor(&self, _session_id: &str, _cursor: &SyncCursor) -> Result<()> {
            Ok(())
        }
        
        async fn get_sync_cursor(&self, _session_id: &str) -> Result<Option<SyncCursor>> {
            Ok(None)
        }
        
        async fn get_all_sync_cursors(&self) -> Result<Vec<SyncCursor>> {
            Ok(Vec::new())
        }
        
        async fn save_message_state(
            &self,
            _user_id: &str,
            _message_id: &str,
            _state: crate::storage::MessageState,
        ) -> Result<()> {
            Ok(())
        }
        
        async fn get_message_state(
            &self,
            _user_id: &str,
            _message_id: &str,
        ) -> Result<Option<crate::storage::MessageState>> {
            Ok(None)
        }
        
        async fn batch_check_deleted(
            &self,
            _user_id: &str,
            _message_ids: &[String],
        ) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
        
        // 扩展信息方法
        async fn save_message_extension(
            &self,
            _message_id: &str,
            _extension: &crate::model::MessageExtension,
        ) -> Result<()> {
            Ok(())
        }
        
        async fn get_message_extension(
            &self,
            _message_id: &str,
        ) -> Result<Option<crate::model::MessageExtension>> {
            Ok(None)
        }
        
        async fn save_session_extension(
            &self,
            _session_id: &str,
            _extension: &crate::model::SessionExtension,
        ) -> Result<()> {
            Ok(())
        }
        
        async fn get_session_extension(
            &self,
            _session_id: &str,
        ) -> Result<Option<crate::model::SessionExtension>> {
            Ok(None)
        }
        
        async fn batch_get_message_extensions(
            &self,
            _message_ids: &[String],
        ) -> Result<Vec<(String, crate::model::MessageExtension)>> {
            Ok(Vec::new())
        }
        
        async fn batch_get_session_extensions(
            &self,
            _session_ids: &[String],
        ) -> Result<Vec<(String, crate::model::SessionExtension)>> {
            Ok(Vec::new())
        }
    }
    
    #[tokio::test]
    async fn test_get_unread_count() {
        // 这个测试需要实际的 ConnectionManager、SyncService 和 EventBus
        // 暂时跳过，后续可以添加集成测试
    }
}
