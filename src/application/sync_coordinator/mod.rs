//! 统一同步引擎协调器
//!
//! 职责：协调 Bootstrap Sync 和 Async Sync
//!
//! ## 同步流程
//!
//! 1. **Bootstrap Sync**: 核心同步（会话列表、未读消息等）
//! 2. **Extension Bootstrap Sync**: 扩展的 Bootstrap 同步（好友列表、群组列表等）
//! 3. **Async Sync**: 异步同步（用户状态、群组信息等）

use std::sync::Arc;
use std::time::Duration;
use crate::application::fsm::FsmManager;
use crate::application::extension::{ExtensionRegistry, SyncSpec};
use crate::application::handlers::{ConversationSyncHandler, SyncHandler};
use crate::application::ports::sync_transport::SyncTransport;
use crate::domain::event::sync_events;
use crate::infrastructure::event_bus::EventBus;

/// 同步协调器
#[derive(Clone)]
pub struct SyncCoordinator {
    fsm: Arc<FsmManager>,
    transport: Arc<dyn SyncTransport>,
    conversation_sync_handler: Arc<ConversationSyncHandler>,
    sync_handler: Arc<SyncHandler>,
    event_bus: Arc<EventBus>,
    extension_registry: Option<Arc<ExtensionRegistry>>,
}

impl std::fmt::Debug for SyncCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncCoordinator")
            .field("has_extension_registry", &self.extension_registry.is_some())
            .finish()
    }
}

impl SyncCoordinator {
    pub fn new(
        fsm: Arc<FsmManager>,
        transport: Arc<dyn SyncTransport>,
        conversation_sync_handler: Arc<ConversationSyncHandler>,
        sync_handler: Arc<SyncHandler>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            fsm,
            transport,
            conversation_sync_handler,
            sync_handler,
            event_bus,
            extension_registry: None,
        }
    }
    
    /// 设置 Extension Registry（用于扩展同步）
    pub fn with_extension_registry(mut self, registry: Arc<ExtensionRegistry>) -> Self {
        self.extension_registry = Some(registry);
        self
    }
    
    /// 执行 Bootstrap Sync
    ///
    /// Bootstrap Sync 必须在 SDK Ready 前完成，失败则 SDK 不可用
    ///
    /// 同步流程：
    /// 1. 核心 Bootstrap Sync（会话列表、未读消息等）
    /// 2. Extension Bootstrap Sync（扩展的 Bootstrap 同步）
    pub async fn execute_bootstrap_sync(&self) -> anyhow::Result<()> {
        // 通过 FSM 开始 Bootstrap Sync
        self.fsm.sync_start_bootstrap().await?;
        
        // 发布 Bootstrap Sync 开始事件（与 subscription_manager 的 sync_events 常量一致）
        self.publish_sync_event(sync_events::BOOTSTRAP_STARTED, serde_json::json!({})).await;
        
        // 1. 执行核心 Bootstrap Sync 逻辑
        tracing::info!("Starting core bootstrap sync");

        let current_user_id = self
            .fsm
            .current_user_id()
            .await
            .unwrap_or_default()
            .trim()
            .to_string();

        let conversations_all_req = flare_proto::common::ConversationSyncAllRequest {
            user_id: current_user_id.clone(),
            sync_options: Some(flare_proto::common::ConversationSyncAllOptions {
                include_archived: false,
                include_deleted: false,
                max_batch_size: 200,
            }),
        };

        let conversations_all_resp = match self
            .transport
            .sync_conversations_all(conversations_all_req, Duration::from_secs(10))
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                tracing::error!("Bootstrap SyncConversationsAll failed: {}", e);
                self.fsm.sync_bootstrap_failed().await?;
                return Err(e);
            }
        };

        self.conversation_sync_handler
            .handle_conversation_summaries(conversations_all_resp.conversations.clone())
            .await?;

        let max_conversations_for_bootstrap = 50usize;
        for summary in conversations_all_resp
            .conversations
            .into_iter()
            .take(max_conversations_for_bootstrap)
        {
            let conversation_id = summary.conversation_id;
            if conversation_id.is_empty() {
                continue;
            }
            self.sync_messages_with_pagination(&current_user_id, &conversation_id)
                .await?;
        }
        
        // 2. Extension Bootstrap Sync
        if let Some(registry) = &self.extension_registry {
            tracing::info!("Starting extension bootstrap sync");
            if let Err(e) = registry.execute_extension_bootstrap_sync().await {
                tracing::error!("Extension bootstrap sync failed: {}", e);
                // 扩展同步失败是否应该阻止 SDK 启动？
                // 这里我们选择记录错误但不中断，或者根据策略决定
            }
        }

        // 生成游标（实际应该从服务器响应中获取）
        let core_cursor = format!("bootstrap_{}", chrono::Utc::now().timestamp());
        
        tracing::info!("Core bootstrap sync completed, cursor: {}", core_cursor);
        
        // 完成 Bootstrap Sync
        self.fsm.sync_bootstrap_completed(core_cursor).await?;
        
        Ok(())
    }

    async fn sync_messages_with_pagination(
        &self,
        _user_id: &str,
        conversation_id: &str,
    ) -> anyhow::Result<()> {
        let mut cursor = String::new();
        let limit = 200;
        let max_pages = 50usize;

        for _ in 0..max_pages {
            let req = flare_proto::common::SyncRequest {
                conversation_id: conversation_id.to_string(),
                last_seq: 0,
                cursor: cursor.clone(),
                limit,
            };

            let mut attempt = 0u32;
            let resp = loop {
                attempt += 1;
                match self
                    .transport
                    .sync_messages(req.clone(), Duration::from_secs(12))
                    .await
                {
                    Ok(r) => break r,
                    Err(e) if attempt < 3 => {
                        let backoff_ms = 200u64 * (1u64 << (attempt - 1));
                        tracing::warn!(
                            conversation_id = %conversation_id,
                            attempt = attempt,
                            backoff_ms = backoff_ms,
                            error = %e,
                            "SyncMessages failed, retrying"
                        );
                        tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)).await;
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            };

            let (has_more, next_cursor) = resp
                .envelope
                .as_ref()
                .map(|e| (e.has_more, e.next_cursor.clone()))
                .unwrap_or((false, String::new()));

            self.sync_handler
                .handle_sync_messages_response(resp)
                .await?;

            if !has_more || next_cursor.is_empty() {
                break;
            }
            cursor = next_cursor;
        }

        Ok(())
    }

    /// 发布同步事件
    async fn publish_sync_event(&self, event_name: &str, payload: serde_json::Value) {
        use crate::domain::event::DomainEvent;
        
        let event = DomainEvent::new(
            event_name,              // event_type
            "sync_coordinator",      // aggregate_id
            1,                       // version (使用 1 或其他逻辑)
            payload,                 // data (serde_json::Value)
        );
        let _ = self.event_bus.publish(event).await;
        
        tracing::debug!("Published sync event: {}", event_name);
    }
    
    /// 执行 Async Sync
    ///
    /// Async Sync 在后台执行，可以失败和重试
    ///
    /// # 参数
    /// * `sync_type` - 同步类型（如 "friend_status", "group_info"）
    pub async fn execute_async_sync(&self, sync_type: String) -> anyhow::Result<()> {
        // 通过 FSM 开始 Async Sync
        self.fsm.sync_start_async(sync_type.clone()).await?;
        
        // 检查是否是扩展的同步类型
        if let Some(registry) = &self.extension_registry {
            let async_specs = registry.get_async_sync_specs().await;
            
            // 查找匹配的同步规格
            if let Some(spec) = async_specs.iter().find(|s| s.sync_type == sync_type) {
                // 执行扩展的 Async Sync
                return self.execute_extension_async_sync(spec).await;
            }
        }
        
        // 执行核心 Async Sync 逻辑
        // 对标微信、Telegram、飞书的增量同步机制
        tracing::info!("Starting core async sync: {}", sync_type);
        
        // 核心 Async Sync 流程：
        // 1. 从本地获取上次同步的游标
        // 2. 从服务器获取增量数据（基于游标）
        // 3. 更新本地 ReadStore
        // 4. 更新游标
        // 5. 发布领域事件
        
        // 注意：这里需要实际的网络层调用
        // 实际实现中应该：
        // - 从 Sync 聚合根获取上次游标
        // - 调用网络层 API 获取增量数据
        // - 解析响应数据
        // - 保存到 ReadStore
        // - 更新游标
        
        // 模拟同步延迟（实际应该是网络请求）
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        
        // 生成游标（实际应该从服务器响应中获取）
        let cursor = format!("async_{}_{}", sync_type, chrono::Utc::now().timestamp());
        
        tracing::info!("Core async sync completed: {}, cursor: {}", sync_type, cursor);
        
        // 通过 FSM 标记完成
        self.fsm.sync_async_completed(sync_type, cursor).await?;
        
        Ok(())
    }
    
    /// 执行扩展的 Async Sync
    ///
    /// 对标微信、Telegram、飞书的扩展异步同步机制
    async fn execute_extension_async_sync(&self, spec: &SyncSpec) -> anyhow::Result<()> {
        // 扩展的 Async Sync 逻辑：
        // 1. 通过扩展注册的同步处理器执行同步
        // 2. 调用网络层获取数据
        // 3. 保存到本地存储
        // 4. 发布领域事件
        
        tracing::debug!("Executing extension async sync: {}", spec.sync_type);
        
        // 注意：这里需要扩展提供同步处理器
        // 扩展可以在 register 方法中注册自己的同步处理器
        // 通过 SdkContext 访问网络层和存储层
        
        // 模拟同步延迟（实际应该是网络请求）
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        
        // 生成游标（实际应该从服务器响应中获取）
        let cursor = format!("async_{}_{}", spec.sync_type, chrono::Utc::now().timestamp());
        
        // 通过 FSM 标记完成
        self.fsm.sync_async_completed(spec.sync_type.clone(), cursor).await?;
        
        tracing::debug!("Extension async sync completed: {}", spec.sync_type);
        Ok(())
    }
    
    /// 执行所有扩展的 Async Sync
    ///
    /// 在后台执行所有扩展的异步同步
    pub async fn execute_all_extension_async_sync(&self) -> anyhow::Result<()> {
        if let Some(registry) = &self.extension_registry {
            let async_specs = registry.get_async_sync_specs().await;
            
            // 按优先级排序
            let mut sorted_specs = async_specs;
            sorted_specs.sort_by(|a, b| b.priority.cmp(&a.priority));
            
            // 并发执行所有扩展的 Async Sync
            let mut tasks = Vec::new();
            for spec in sorted_specs {
                let this = self.clone();
                let spec = spec.clone();
                let sync_type = spec.sync_type.clone();
                
                tasks.push(tokio::spawn(async move {
                    // 执行扩展的 Async Sync 逻辑
                    tracing::debug!("Executing extension async sync: {}", sync_type);
                    
                    // 通过 FSM 开始 Async Sync
                    if let Err(e) = this.fsm.sync_start_async(sync_type.clone()).await {
                        tracing::error!("Failed to start async sync {}: {}", sync_type, e);
                        return Err(e);
                    }
                    
                    // 执行实际的同步逻辑
                    if let Err(e) = this.execute_extension_async_sync(&spec).await {
                        tracing::error!("Failed to execute extension async sync {}: {}", sync_type, e);
                        // 尝试标记失败
                        let _ = this.fsm.sync_async_failed().await;
                        return Err(e);
                    }
                    
                    Ok(())
                }));
            }
            
            // 等待所有同步完成（允许部分失败）
            let mut errors = Vec::new();
            for task in tasks {
                match task.await {
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => errors.push(e),
                    Err(e) => errors.push(anyhow::anyhow!("Task join error: {}", e)),
                }
            }
            
            if !errors.is_empty() {
                tracing::warn!("Some extension async syncs failed: {:?}", errors);
                // Async Sync 允许失败，不返回错误
            }
        }
        
        Ok(())
    }
    
    /// 执行 Async Sync（带重试）
    pub async fn execute_async_sync_with_retry(
        &self,
        sync_type: String,
        max_retries: u32,
    ) -> anyhow::Result<()> {
        let mut retries = 0;
        
        while retries < max_retries {
            match self.execute_async_sync(sync_type.clone()).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    retries += 1;
                    if retries >= max_retries {
                        // 最后一次失败，通过 FSM 标记失败
                        self.fsm.sync_async_failed().await?;
                        return Err(e);
                    }
                    // 等待后重试
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
        
        Ok(())
    }
}
