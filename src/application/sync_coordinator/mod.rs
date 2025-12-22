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
use crate::application::fsm::FsmManager;
use crate::application::extension::{ExtensionRegistry, SyncSpec, ExtensionSyncMode};
use crate::domain::repository::EventStore;

/// 同步协调器
#[derive(Clone)]
pub struct SyncCoordinator {
    fsm: Arc<FsmManager>,
    event_store: Arc<dyn EventStore>,
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
        event_store: Arc<dyn EventStore>,
    ) -> Self {
        Self {
            fsm,
            event_store,
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
        
        // 1. 执行核心 Bootstrap Sync 逻辑
        // 对标微信、Telegram、飞书的 Bootstrap Sync 机制
        // 1.1 同步会话列表
        // 1.2 同步未读消息
        // 1.3 同步用户信息等
        
        tracing::info!("Starting core bootstrap sync");
        
        // 核心 Bootstrap Sync 流程：
        // 1. 从服务器获取会话列表（全量）
        // 2. 从服务器获取未读消息列表
        // 3. 从服务器获取用户信息
        // 4. 保存到本地 ReadStore
        // 5. 发布领域事件
        
        // 注意：这里需要实际的网络层调用
        // 由于当前没有真实的网络层实现，这里使用占位逻辑
        // 实际实现中应该：
        // - 调用网络层 API 获取数据
        // - 解析响应数据
        // - 保存到 ReadStore
        // - 发布领域事件
        
        // 模拟同步延迟（实际应该是网络请求）
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        // 生成游标（实际应该从服务器响应中获取）
        let core_cursor = format!("bootstrap_{}", chrono::Utc::now().timestamp());
        
        tracing::info!("Core bootstrap sync completed, cursor: {}", core_cursor);
        
        // 2. 执行 Extension Bootstrap Sync
        if let Some(registry) = &self.extension_registry {
            let bootstrap_specs = registry.get_bootstrap_sync_specs().await;
            
            // 按优先级排序
            let mut sorted_specs = bootstrap_specs;
            sorted_specs.sort_by(|a, b| b.priority.cmp(&a.priority));
            
            // 执行每个扩展的 Bootstrap Sync
            for spec in sorted_specs {
                tracing::info!("Executing extension bootstrap sync: {}", spec.sync_type);
                
                // TODO: 调用扩展的同步逻辑
                // 这里应该通过扩展注册的回调或处理器来执行同步
                
                match self.execute_extension_bootstrap_sync(&spec).await {
                    Ok(_) => {
                        tracing::info!("Extension bootstrap sync completed: {}", spec.sync_type);
                    }
                    Err(e) => {
                        // Bootstrap Sync 失败，SDK 不可用
                        tracing::error!("Extension bootstrap sync failed: {} - {}", spec.sync_type, e);
                        self.fsm.sync_bootstrap_failed().await?;
                        return Err(anyhow::anyhow!("Extension bootstrap sync failed: {}", e));
                    }
                }
            }
        }
        
        // 通过 FSM 标记完成
        self.fsm.sync_bootstrap_completed(core_cursor).await?;
        
        Ok(())
    }
    
    /// 执行扩展的 Bootstrap Sync
    ///
    /// 对标微信、Telegram、飞书的扩展同步机制
    async fn execute_extension_bootstrap_sync(&self, spec: &SyncSpec) -> anyhow::Result<()> {
        // 扩展的 Bootstrap Sync 逻辑：
        // 1. 通过扩展注册的同步处理器执行同步
        // 2. 调用网络层获取数据
        // 3. 保存到本地存储
        // 4. 发布领域事件
        
        tracing::debug!("Executing extension bootstrap sync: {}", spec.sync_type);
        
        // 注意：这里需要扩展提供同步处理器
        // 当前实现中，扩展通过 SdkContext 访问核心能力
        // 扩展可以在 register 方法中注册自己的同步处理器
        
        // 模拟同步延迟（实际应该是网络请求）
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        
        tracing::debug!("Extension bootstrap sync completed: {}", spec.sync_type);
        Ok(())
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
                let sync_type = spec.sync_type.clone();
                let fsm = self.fsm.clone();
                let event_store = self.event_store.clone();
                
                tasks.push(tokio::spawn(async move {
                    // 执行扩展的 Async Sync 逻辑
                    tracing::debug!("Executing extension async sync: {}", sync_type);
                    
                    // 通过 FSM 开始 Async Sync
                    if let Err(e) = fsm.sync_start_async(sync_type.clone()).await {
                        tracing::error!("Failed to start async sync {}: {}", sync_type, e);
                        return Err(e);
                    }
                    
                    // 执行实际的同步逻辑
                    // 注意：这里需要扩展提供同步处理器
                    // 扩展可以通过 SdkContext 访问网络层和存储层
                    
                    // 模拟同步延迟（实际应该是网络请求）
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                    
                    // 生成游标（实际应该从服务器响应中获取）
                    let cursor = format!("async_{}_{}", sync_type, chrono::Utc::now().timestamp());
                    
                    if let Err(e) = fsm.sync_async_completed(sync_type, cursor).await {
                        tracing::error!("Failed to complete async sync: {}", e);
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
