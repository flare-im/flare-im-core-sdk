//! 同步服务
//!
//! 提供消息和会话的同步功能

use crate::connection::ConnectionManager;
use crate::event::{Event, EventBus, ConnectionEvent, SyncEvent};
use crate::model::{Message, SessionSummary, SyncCursor, SyncResult};
use crate::protocol::{FrameBuilder, RequestManager};
use crate::storage::StorageBackend;
use anyhow::{Context, Result};
use flare_core::common::protocol::{CustomCommand, Reliability};
use flare_proto::session::{
    SessionBootstrapRequest, SessionBootstrapResponse,
};
use prost::Message as ProstMessage;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration, Instant};
#[cfg(target_arch = "wasm32")]
use tokio::task::spawn_local as tokio_spawn;
#[cfg(not(target_arch = "wasm32"))]
use tokio::spawn as tokio_spawn;
use tracing::{debug, error, info, warn};

/// 同步配置（简化版）
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// 消息同步批次大小（默认 100）
    pub message_batch_size: usize,
    
    /// 会话同步批次大小（默认 50）
    pub session_batch_size: usize,
    
    /// 请求超时时间（秒，默认 30）
    pub request_timeout: u64,
    
    /// 每个会话同步的最近消息数（默认 50，用于快速显示）
    pub recent_message_limit: usize,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            message_batch_size: 100,
            session_batch_size: 50,
            request_timeout: 30,
            recent_message_limit: 50,
        }
    }
}

/// 重连同步策略
#[derive(Debug, Clone)]
pub struct ReconnectSyncStrategy {
    /// 是否自动同步（默认 true）
    pub auto_sync: bool,
    
    /// 同步延迟（重连成功后等待多久开始同步，默认 1 秒）
    pub sync_delay: Duration,
    
    /// 同步超时（默认 60 秒）
    pub sync_timeout: Duration,
    
    /// 同步模式
    pub sync_mode: ReconnectSyncMode,
    
    /// 最大重试次数（默认 3）
    pub max_retries: u32,
}

impl Default for ReconnectSyncStrategy {
    fn default() -> Self {
        Self {
            auto_sync: true,
            sync_delay: Duration::from_secs(1),
            sync_timeout: Duration::from_secs(60),
            sync_mode: ReconnectSyncMode::Smart {
                full_sync_threshold: Duration::from_secs(5 * 60), // 5分钟
            },
            max_retries: 3,
        }
    }
}

/// 重连同步模式
#[derive(Debug, Clone)]
pub enum ReconnectSyncMode {
    /// 智能模式：根据离线时间决定同步策略
    Smart {
        /// 离线时间阈值（超过此时间执行全量同步）
        full_sync_threshold: Duration,
    },
    
    /// 增量同步：只同步增量消息
    Incremental,
    
    /// 全量同步：同步所有数据
    Full,
    
    /// 不同步：重连后不自动同步
    None,
}

/// 分层同步模式
#[derive(Debug, Clone)]
pub enum LayeredSyncMode {
    /// 只同步最近消息（层级1）
    RecentOnly,
    
    /// 只同步游标信息（层级2）
    CursorOnly,
    
    /// 按需加载历史消息（层级3）
    OnDemand {
        /// 起始序列号
        start_seq: Option<i64>,
        /// 加载数量限制
        limit: Option<usize>,
    },
    
    /// 全量同步（不推荐，用于特殊情况）
    Full,
}

// 使用任务模块
use crate::task::{
    SyncTaskExecutor, SyncContext, TaskResult, TaskStatus, TaskType, TaskExecutionMode,
    FullSyncTask, MessageSyncTask, SessionSyncTask, SyncTask,
};


/// 同步服务
/// 
/// 负责消息和会话的同步
pub struct SyncService {
    /// 连接管理器（通过长连接发送同步请求）
    connection: Arc<ConnectionManager>,
    
    /// 本地存储
    storage: Arc<dyn StorageBackend>,
    
    /// 事件总线
    event_bus: Arc<EventBus>,
    
    /// 请求管理器（用于管理请求/响应）
    request_manager: Arc<RequestManager>,
    
    /// 同步配置
    config: SyncConfig,
    
    /// 当前用户 ID
    user_id: Arc<RwLock<String>>,
    
    /// 同步任务列表（用于跟踪同步进度，可选功能）
    tasks: Arc<Mutex<HashMap<String, SyncTask>>>,
    
    /// 任务执行器列表（用户注册的任务）
    task_executors: Arc<Mutex<Vec<Arc<dyn SyncTaskExecutor>>>>,
    
    /// 运行中的任务（任务ID -> 任务实例）
    running_tasks: Arc<Mutex<HashMap<String, SyncTask>>>,
    
    /// 任务系统是否启用（可选，由客户端决定）
    tasks_enabled: Arc<RwLock<bool>>,
    
    /// 重连同步策略
    reconnect_strategy: Arc<Mutex<Option<ReconnectSyncStrategy>>>,
}

/// 全量同步结果
#[derive(Debug, Clone)]
pub struct FullSyncResult {
    /// 同步的会话数量
    pub session_count: usize,
    
    /// 同步的消息总数
    pub total_message_count: usize,
    
    /// 每个会话的同步结果
    pub session_results: Vec<SyncResult>,
}

impl SyncService {
    /// 创建新的同步服务实例
    pub fn new(
        connection: Arc<ConnectionManager>,
        storage: Arc<dyn StorageBackend>,
        event_bus: Arc<EventBus>,
        user_id: Arc<RwLock<String>>,
    ) -> Self {
        let request_manager = Arc::new(RequestManager::new());
        
        Self {
            connection,
            storage,
            event_bus,
            request_manager,
            config: SyncConfig::default(),
            user_id,
            tasks: Arc::new(Mutex::new(HashMap::new())),
            task_executors: Arc::new(Mutex::new(Vec::new())),
            running_tasks: Arc::new(Mutex::new(HashMap::new())),
            tasks_enabled: Arc::new(RwLock::new(false)),
            reconnect_strategy: Arc::new(Mutex::new(None)),
        }
    }
    
    /// 启用任务系统（可选，由客户端决定）
    /// 
    /// 启用后，所有同步操作将通过任务系统执行
    pub async fn enable_tasks(&self, enabled: bool) {
        let mut flag = self.tasks_enabled.write().await;
        *flag = enabled;
        info!(enabled, "Task system {}", if enabled { "enabled" } else { "disabled" });
    }
    
    /// 检查任务系统是否启用
    pub async fn is_tasks_enabled(&self) -> bool {
        *self.tasks_enabled.read().await
    }
    
    /// 注册同步任务执行器
    /// 
    /// # 参数
    /// - `executor`: 任务执行器
    /// 
    /// # 示例
    /// ```rust,no_run
    /// struct MySyncTask;
    /// 
    /// #[async_trait::async_trait]
    /// impl SyncTaskExecutor for MySyncTask {
    ///     fn name(&self) -> &str {
    ///         "MySyncTask"
    ///     }
    ///     
    ///     fn is_mandatory(&self) -> bool {
    ///         false // 可选任务
    ///     }
    ///     
    ///     async fn execute(&self, context: &SyncContext) -> Result<SyncTaskResult> {
    ///         // 执行同步逻辑
    ///         Ok(SyncTaskResult {
    ///             task_id: "my-task".to_string(),
    ///             success: true,
    ///             session_count: 0,
    ///             message_count: 0,
    ///             error: None,
    ///             duration_ms: 0,
    ///         })
    ///     }
    /// }
    /// 
    /// let executor = Arc::new(MySyncTask);
    /// sync_service.register_task(executor).await;
    /// ```
    pub async fn register_task(&self, executor: Arc<dyn SyncTaskExecutor>) {
        let mut executors = self.task_executors.lock().await;
        executors.push(executor);
        // 按优先级排序（优先级小的在前）
        executors.sort_by_key(|e| e.priority());
        info!("Sync task registered: {}", executors.last().unwrap().name());
    }
    
    /// 取消注册任务执行器
    pub async fn unregister_task(&self, name: &str) {
        let mut executors = self.task_executors.lock().await;
        executors.retain(|e| e.name() != name);
        info!(name, "Sync task unregistered");
    }
    
    /// 获取所有注册的任务执行器
    pub async fn get_registered_tasks(&self) -> Vec<String> {
        let executors = self.task_executors.lock().await;
        executors.iter().map(|e| e.name().to_string()).collect()
    }
    
    /// 执行所有强制任务（如果任务系统启用）
    /// 
    /// 强制任务必须执行，可选任务由客户端决定是否执行
    pub async fn execute_mandatory_tasks(&self) -> Result<Vec<TaskResult>> {
        if !self.is_tasks_enabled().await {
            return Ok(Vec::new());
        }
        
        let executors = self.task_executors.lock().await;
        let mandatory_executors: Vec<_> = executors
            .iter()
            .filter(|e| matches!(e.task_type(), TaskType::Mandatory))
            .map(Arc::clone)
            .collect();
        drop(executors);
        
        let context = self.create_sync_context().await?;
        // 优化：预分配 Vec 容量
        let mut results = Vec::with_capacity(mandatory_executors.len());
        
        for executor in mandatory_executors {
            let task_id = format!("{}-{}", executor.name(), uuid::Uuid::new_v4());
            let mut task = SyncTask::new(task_id.clone(), executor);
            
            // 检查执行模式
            let execution_mode = task.execution_mode();
            if matches!(execution_mode, TaskExecutionMode::Async) {
                // 异步执行
                let context_clone = SyncContext {
                    connection: Arc::clone(&context.connection),
                    storage: Arc::clone(&context.storage),
                    event_bus: Arc::clone(&context.event_bus),
                    request_manager: Arc::clone(&context.request_manager),
                    config: context.config.clone(),
                    user_id: context.user_id.clone(),
                };
                let running_tasks = Arc::clone(&self.running_tasks);
                let task_id_clone = task_id.clone();
                
                tokio_spawn(async move {
                    let _ = task.execute(&context_clone).await;
                    running_tasks.lock().await.insert(task_id_clone, task);
                });
            } else {
                // 同步执行
                task.execute(&context).await?;
                if let Some(result) = task.result.clone() {
                    results.push(result);
                }
                self.running_tasks.lock().await.insert(task_id, task);
            }
        }
        
        Ok(results)
    }
    
    /// 执行指定任务（如果任务系统启用）
    pub async fn execute_task(&self, task_name: &str) -> Result<TaskResult> {
        if !self.is_tasks_enabled().await {
            return Err(anyhow::anyhow!("Task system is not enabled"));
        }
        
        let executors = self.task_executors.lock().await;
        let executor = executors
            .iter()
            .find(|e| e.name() == task_name)
            .ok_or_else(|| anyhow::anyhow!("Task not found: {}", task_name))?;
        let executor = Arc::clone(executor);
        drop(executors);
        
        let context = self.create_sync_context().await?;
        let task_id = format!("{}-{}", executor.name(), uuid::Uuid::new_v4());
        let mut task = SyncTask::new(task_id.clone(), executor);
        
        task.execute(&context).await?;
        
        if let Some(result) = task.result.clone() {
            self.running_tasks.lock().await.insert(task_id, task);
            Ok(result)
        } else {
            Err(anyhow::anyhow!("Task execution did not return result"))
        }
    }
    
    /// 创建同步上下文
    async fn create_sync_context(&self) -> Result<SyncContext> {
        Ok(SyncContext {
            connection: Arc::clone(&self.connection),
            storage: Arc::clone(&self.storage),
            event_bus: Arc::clone(&self.event_bus),
            request_manager: Arc::clone(&self.request_manager),
            config: self.config.clone(),
            user_id: self.user_id.read().await.clone(),
        })
    }
    
    /// 获取运行中的任务
    pub async fn get_running_tasks(&self) -> Vec<SyncTask> {
        let tasks = self.running_tasks.lock().await;
        tasks.values().cloned().collect()
    }
    
    /// 获取指定任务的状态
    pub async fn get_task_status(&self, task_id: &str) -> Option<SyncTask> {
        let tasks = self.running_tasks.lock().await;
        tasks.get(task_id).cloned()
    }
    
    /// 旧版创建方法（保持兼容）
    pub fn new_legacy(
        connection: Arc<ConnectionManager>,
        storage: Arc<dyn StorageBackend>,
        event_bus: Arc<EventBus>,
        user_id: Arc<RwLock<String>>,
    ) -> Self {
        Self {
            connection,
            storage,
            event_bus,
            request_manager: Arc::new(RequestManager::new()),
            config: SyncConfig::default(),
            user_id,
            tasks: Arc::new(Mutex::new(HashMap::new())),
            task_executors: Arc::new(Mutex::new(Vec::new())),
            running_tasks: Arc::new(Mutex::new(HashMap::new())),
            tasks_enabled: Arc::new(RwLock::new(false)),
            reconnect_strategy: Arc::new(Mutex::new(None)),
        }
    }

    /// 设置同步配置
    pub fn with_config(mut self, config: SyncConfig) -> Self {
        self.config = config;
        self
    }

    /// 通过任务系统执行全量同步
    async fn full_sync_via_task(&self) -> Result<FullSyncResult> {
        let executor = Arc::new(FullSyncTask::new(Arc::new(self.clone())));
        let task_id = format!("full-sync-{}", uuid::Uuid::new_v4());
        let mut task = SyncTask::new(task_id.clone(), executor);
        
        let context = self.create_sync_context().await?;
        
        // 执行任务
        task.execute(&context).await?;
        
        // 保存任务到运行列表
        self.running_tasks.lock().await.insert(task_id, task.clone());
        
        // 从任务结果中提取 FullSyncResult
        if let Some(result) = task.result {
            if result.success {
                // 需要从实际执行中获取 FullSyncResult
                // 这里我们需要调用实际的 full_sync_internal
                return self.full_sync_internal().await;
            } else {
                return Err(anyhow::anyhow!("Full sync task failed: {}", result.error.unwrap_or_default()));
            }
        }
        
        // 如果任务没有返回结果，直接调用内部方法
        self.full_sync_internal().await
    }
    
    /// 全量同步内部实现（实际执行逻辑）
    pub(crate) async fn full_sync_internal(&self) -> Result<FullSyncResult> {
        let start_time = Instant::now();
        info!("Starting full sync");
        
        // 发布同步开始事件
        self.event_bus.publish(Event::Sync(SyncEvent::SyncStarted {
            sync_type: "full".to_string(),
            estimated_sessions: None,
        }));
        
        // 1. 构建 SessionBootstrap 请求
        let request = SessionBootstrapRequest {
            user_id: self.user_id.read().await.clone(),
            include_recent_messages: true,
            recent_message_limit: self.config.recent_message_limit as i32,
            ..Default::default()
        };
        
        // 2. 编码请求（优化：使用 encode_to_vec 直接编码）
        let request_bytes = request.encode_to_vec();
        
        // 3. 创建请求并发送
        let (request_id, response_rx) = self.request_manager.create_request().await;
        debug!(%request_id, limit = self.config.message_batch_size, "Created SessionBootstrap request");
        
        // 构造带 metadata 的 Frame，方便响应路径匹配与排查（优化：预分配容量）
        let mut metadata = std::collections::HashMap::with_capacity(2);
        metadata.insert("request_id".to_string(), request_id.as_bytes().to_vec());
        metadata.insert("command".to_string(), b"SessionBootstrap".to_vec());

        let frame = FrameBuilder::new()
            .with_custom_command(CustomCommand {
                name: "SessionBootstrap".to_string(),
                data: request_bytes,
                metadata,
            })
            .with_message_id(request_id.clone())
            .with_reliability(Reliability::AtLeastOnce)
            .build();
        debug!(%request_id, msg_id = %frame.message_id, ts = frame.timestamp, "Sending SessionBootstrap frame");
        
        if let Err(e) = self.connection.send_frame(&frame).await {
            warn!(error = %e, "Failed to send SessionBootstrap request, trying fallback");
            return self.full_sync_fallback(start_time).await;
        }
        
        // 4. 等待响应（带超时）
        let timeout_duration = Duration::from_secs(self.config.request_timeout);
        let response_frame = match timeout(timeout_duration, response_rx).await {
            Ok(Ok(f)) => f,
            Ok(Err(e)) => {
                warn!(error = %e, "Failed to receive SessionBootstrap response, trying fallback");
                return self.full_sync_fallback(start_time).await;
            }
            Err(_) => {
                warn!("SessionBootstrap response timeout, trying fallback");
                return self.full_sync_fallback(start_time).await;
            }
        };
        info!(%request_id, elapsed_ms = %start_time.elapsed().as_millis(), "Received SessionBootstrap response frame");
        
        // 5. 解码响应
        let response_data = {
            let opt = response_frame
                .command
                .and_then(|cmd| {
                    if let flare_core::common::protocol::Command {
                        r#type: Some(flare_core::common::protocol::flare::core::commands::command::Type::Custom(custom_cmd)),
                    } = cmd {
                        Some(custom_cmd.data)
                    } else {
                        None
                    }
                });
            match opt {
                Some(d) => d,
                None => {
                    warn!("Invalid SessionBootstrap response, trying fallback");
                    return self.full_sync_fallback(start_time).await;
                }
            }
        };
        
        let response = match SessionBootstrapResponse::decode(&response_data[..]) {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "Failed to decode SessionBootstrapResponse, trying fallback");
                return self.full_sync_fallback(start_time).await;
            }
        };
        
        // 6. 处理响应数据
        // 优化：预分配容量（假设平均每个会话有 10 条消息）
        let mut session_results = Vec::with_capacity(response.sessions.len());
        let mut total_message_count = 0;
        
        // 6.1 保存会话列表
        for session_proto in &response.sessions {
            let session: SessionSummary = session_proto.clone().into();
            debug!(
                %request_id,
                session_id = %session.session_id,
                last_msg_id = ?session.last_message_id,
                last_msg_ts = ?session.last_message_time,
                unread = session.unread_count,
                "Persisting session"
            );
            
            // 保存会话
            self.storage.save_session(&session).await
                .context(format!("Failed to save session: {}", session.session_id))?;
            
            // 6.2 保存消息（如果有最近消息）
            let mut message_count = 0;
            let mut last_seq = None;
            
            // SessionBootstrapResponse 包含 recent_messages 字段
            // 需要根据 session_id 过滤出属于当前会话的消息
            // 优化：先过滤，再批量处理，预分配容量（假设每个会话最多 recent_message_limit 条）
            let _estimated_capacity = self.config.recent_message_limit.min(response.recent_messages.len());
            let session_messages: Vec<_> = response.recent_messages.iter()
                .filter(|msg| msg.session_id == session.session_id)
                .map(|msg_proto| {
                    let message: Message = msg_proto.clone();
                    message
                })
                .collect();
            
            // 批量保存消息（优化：并行保存）
            if !session_messages.is_empty() {
                if session_messages.len() > 10 {
                    // 优化：预分配容量
                    let mut save_tasks = Vec::with_capacity(session_messages.len());
                    for message in &session_messages {
                        let storage = Arc::clone(&self.storage);
                        let msg = message.clone();
                        save_tasks.push(tokio_spawn(async move {
                            storage.save_message(&msg).await
                        }));
                    }
                    for task in save_tasks {
                        task.await??;
                    }
                } else {
                    for message in &session_messages {
                        self.storage.save_message(message).await
                            .context("Failed to save message")?;
                    }
                }
                
                message_count += session_messages.len();
                
                // 更新 last_seq（使用最后一条消息）
                if let Some(last_msg) = session_messages.last() {
                    if let Some(seq) = Self::extract_seq_from_message(last_msg) {
                        last_seq = Some(seq);
                    }
                    debug!(%request_id, session_id = %session.session_id, message_count = session_messages.len(), "Persisted recent messages");
                }
            }
            
            // 6.3 创建/更新同步游标（分层同步策略）
            // 层级1：已同步最近消息
            // 层级2：更新游标信息（max_seq, cursor_seq, unread_count）
            let mut cursor = self.storage.get_sync_cursor(&session.session_id).await?
                .unwrap_or_else(|| SyncCursor::new(session.session_id.clone()));
            
            // 更新层级1：最近消息同步范围
            if let Some(last_msg_seq) = last_seq {
                // 计算最近消息的起始序列号
                let start_seq = (last_msg_seq as i64 - message_count as i64 + 1).max(1);
                cursor.update_recent_sync_range(start_seq, last_msg_seq);
                cursor.last_seq = Some(last_msg_seq);
                cursor.last_timestamp = session.last_message_time;
                cursor.last_message_id = session.last_message_id.clone();
            }
            
            // 更新层级2：服务器游标信息（如果服务器提供了 max_seq）
            // 注意：SessionBootstrapResponse 可能包含 max_seq 信息
            // 这里假设服务器在 SessionSummary 的 metadata 中提供了 max_seq
            // 实际实现需要根据服务端协议调整
            if let Some(server_max_seq_str) = session.metadata.get("max_seq") {
                if let Ok(server_max_seq) = server_max_seq_str.parse::<i64>() {
                    cursor.update_server_cursor(server_max_seq, Some(session.unread_count as i64));
                }
            }
            
            self.storage.save_sync_cursor(&session.session_id, &cursor).await
                .context("Failed to save sync cursor")?;
            debug!(%request_id, session_id = %session.session_id, cursor_last_seq = ?cursor.last_seq, cursor_last_ts = ?cursor.last_timestamp, "Updated sync cursor");
            
            session_results.push(SyncResult {
                session_id: session.session_id.clone(),
                message_count,
                has_more: false, // TODO: 从响应中获取
                next_cursor: None, // TODO: 从响应中获取
                last_seq,
            });
            
            total_message_count += message_count;
        }
        
        let duration_ms = start_time.elapsed().as_millis() as u64;
        
        info!(
            session_count = session_results.len(),
            total_message_count,
            duration_ms,
            %request_id,
            "Full sync completed"
        );
        
        // 发布同步完成事件
        self.event_bus.publish(Event::Sync(SyncEvent::SyncCompleted {
            sessions: session_results.len(),
            messages: total_message_count,
            duration_ms,
            has_background_sync: false,
        }));
        
        Ok(FullSyncResult {
            session_count: session_results.len(),
            total_message_count,
            session_results,
        })
    }

    async fn full_sync_fallback(&self, start_time: Instant) -> Result<FullSyncResult> {
        info!("Starting full sync fallback: ListSessions + RecentMessages");
        // 1) 增量拉取会话列表
        let sessions_result = self.sync_sessions(None).await
            .context("Fallback: failed to sync sessions")?;
        // 优化：预分配 Vec 容量
        let mut session_results = Vec::with_capacity(sessions_result.sessions.len());
        let mut total_message_count = 0usize;

        // 2) 为每个会话拉取最近消息并更新游标
        for session in &sessions_result.sessions {
            let recent = self.sync_recent_messages(&session.session_id).await
                .context("Fallback: failed to sync recent messages")?;
            total_message_count += recent.message_count;
            session_results.push(recent);
        }

        let duration_ms = start_time.elapsed().as_millis() as u64;
        info!(
            session_count = session_results.len(),
            total_message_count,
            duration_ms,
            "Full sync fallback completed"
        );

        self.event_bus.publish(Event::Sync(SyncEvent::SyncCompleted {
            sessions: session_results.len(),
            messages: total_message_count,
            duration_ms,
            has_background_sync: false,
        }));

        Ok(FullSyncResult {
            session_count: session_results.len(),
            total_message_count,
            session_results,
        })
    }

    /// 分层同步消息（层级化游标策略）
    /// 
    /// 采用三层同步策略：
    /// - 层级1：只同步最近N条消息（给UI使用）
    /// - 层级2：同步游标信息（max_seq, cursor_seq, unread_count），不下载消息实体
    /// - 层级3：按需加载历史消息（用户手动下拉时才加载）
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `sync_mode`: 同步模式
    ///   - `RecentOnly`: 只同步最近消息（层级1）
    ///   - `CursorOnly`: 只同步游标信息（层级2）
    ///   - `OnDemand`: 按需加载历史消息（层级3）
    ///   - `Full`: 全量同步（不推荐，用于特殊情况）
    /// 
    /// # 返回
    /// - `Result<SyncResult>`: 同步结果
    pub async fn sync_messages_layered(
        &self,
        session_id: &str,
        sync_mode: LayeredSyncMode,
    ) -> Result<SyncResult> {
        match sync_mode {
            LayeredSyncMode::RecentOnly => {
                self.sync_recent_messages(session_id).await
            }
            LayeredSyncMode::CursorOnly => {
                self.sync_cursor_info(session_id).await
            }
            LayeredSyncMode::OnDemand { start_seq, limit } => {
                self.load_history_messages(session_id, start_seq, limit).await
            }
            LayeredSyncMode::Full => {
                // 全量同步（不推荐，用于特殊情况）
                self.sync_messages_full(session_id).await
            }
        }
    }

    /// 层级1：同步最近消息（给UI使用）
    /// 
    /// 只同步最近N条消息，用于UI显示
    async fn sync_recent_messages(&self, session_id: &str) -> Result<SyncResult> {
        // 获取会话信息，判断是否为超大群
        // 注意：SessionSummary 可能不包含 member_count，需要从 metadata 或其他地方获取
        let _session = self.storage.get_session(session_id).await?
            .ok_or_else(|| anyhow::anyhow!("Session not found: {}", session_id))?;
        
        // 确定同步的消息数（简化：统一使用 recent_message_limit）
        let message_limit = self.config.recent_message_limit;
        
        // 从服务器获取最近消息
        let messages = self.query_recent_messages(session_id, message_limit).await?;
        
        // 保存消息（使用优化工具函数）
        crate::service::sync_utils::save_messages_optimized(
            Arc::clone(&self.storage),
            &messages,
            10, // 阈值：超过 10 条消息使用并行保存
        ).await
        .context("Failed to save messages")?;
        
        // 更新游标（使用优化工具函数）
        if let Some(last_message) = messages.last() {
            let start_seq = if let Some(last_seq) = Self::extract_seq_from_message(last_message) {
                (last_seq - messages.len() as i64 + 1).max(1)
            } else {
                return Ok(SyncResult {
                    session_id: session_id.to_string(),
                    message_count: messages.len(),
                    has_more: false,
                    next_cursor: None,
                    last_seq: None,
                });
            };
            
            let mut cursor = self.storage.get_sync_cursor(session_id).await?
                .unwrap_or_else(|| SyncCursor::new(session_id.to_string()));
            
            cursor.update_recent_sync_range(start_seq, Self::extract_seq_from_message(last_message).unwrap());
            cursor.last_seq = Self::extract_seq_from_message(last_message);
            cursor.last_timestamp = last_message.timestamp.as_ref().map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000));
            cursor.last_message_id = Some(last_message.id.clone());
            
            self.storage.save_sync_cursor(session_id, &cursor).await?;
        }
        
        Ok(SyncResult {
            session_id: session_id.to_string(),
            message_count: messages.len(),
            has_more: false,
            next_cursor: None,
            last_seq: messages.last().and_then(|m| Self::extract_seq_from_message(m)),
        })
    }

    /// 层级2：同步游标信息（不下载消息实体）
    /// 
    /// 只同步游标信息（max_seq, cursor_seq, unread_count），不下载消息实体
    /// 用于计算未读数，而不需要下载所有未读消息
    /// 
    /// **核心优势**：
    /// - 一个群有5万条未读消息，但只需要同步游标信息（max_seq, cursor_seq, unread_count）
    /// - 不需要下载5万条消息实体
    /// - 客户端可以显示未读数，但不需要存储所有消息
    async fn sync_cursor_info(&self, session_id: &str) -> Result<SyncResult> {
        // 从服务器获取会话信息（包含 max_seq 和 unread_count）
        let session = self.query_session_info(session_id).await?;
        
        // 获取本地游标
        let mut cursor = self.storage.get_sync_cursor(session_id).await?
            .unwrap_or_else(|| SyncCursor::new(session_id.to_string()));
        
        // 从会话的 metadata 或 server_cursor_ts 中获取 max_seq
        // 注意：实际实现需要根据服务端协议调整
        // 这里假设服务端在 metadata 中提供了 max_seq
        let max_seq: Option<i64> = session.metadata
            .get("max_seq")
            .and_then(|s| s.parse().ok());
        
        // 如果服务端提供了 max_seq，更新游标
        if let Some(max_seq) = max_seq {
            cursor.update_server_cursor(max_seq, Some(session.unread_count as i64));
            
            // 保存游标
            self.storage.save_sync_cursor(session_id, &cursor).await?;
            
            // 更新会话的未读数
            self.storage.update_session(session_id, crate::storage::SessionUpdate::new()
                .with_unread_count(session.unread_count))
                .await?;
        } else {
            // 如果服务端未提供 max_seq，使用 unread_count 作为参考
            // 注意：这种情况下无法准确计算未读数，但可以显示服务器提供的未读数
            self.storage.update_session(session_id, crate::storage::SessionUpdate::new()
                .with_unread_count(session.unread_count))
                .await?;
        }
        
        Ok(SyncResult {
            session_id: session_id.to_string(),
            message_count: 0,  // 没有下载消息实体，只同步了游标信息
            has_more: false,
            next_cursor: None,
            last_seq: cursor.last_seq,
        })
    }

    /// 层级3：按需加载历史消息
    /// 
    /// 用户手动下拉时才加载历史消息
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `start_seq`: 起始序列号（从哪个seq开始加载，通常是本地最小seq - 1）
    /// - `limit`: 加载的消息数量（默认 50）
    async fn load_history_messages(
        &self,
        session_id: &str,
        start_seq: Option<i64>,
        limit: Option<usize>,
    ) -> Result<SyncResult> {
        let limit = limit.unwrap_or(self.config.message_batch_size);
        
        // 确定起始序列号
        let after_seq = if let Some(seq) = start_seq {
            seq
        } else {
            // 从本地已同步的消息中找到最小seq（向前加载更早的消息）
            // 获取本地消息，找到最小seq
            let local_messages = self.storage.get_messages_by_seq(session_id, 0, 1000).await?;
            if let Some(first_message) = local_messages.first() {
                if let Some(min_seq) = Self::extract_seq_from_message(first_message) {
                    min_seq - 1  // 从最小seq的前一个开始加载
                } else {
                    0
                }
            } else {
                // 本地没有消息，从游标开始
                if let Some(cursor) = self.storage.get_sync_cursor(session_id).await? {
                    cursor.last_seq.unwrap_or(0)
                } else {
                    0
                }
            }
        };
        
        // 从服务器加载历史消息
        let messages = self.query_messages_by_seq(session_id, after_seq, limit).await?;
        
        // 保存消息（使用优化工具函数）
        crate::service::sync_utils::save_messages_optimized(
            Arc::clone(&self.storage),
            &messages,
            10, // 阈值：超过 10 条消息使用并行保存
        ).await
        .context("Failed to save messages")?;
        
        // 更新游标（使用优化工具函数）
        if let Some(last_message) = messages.last() {
            if let Some(last_seq) = Self::extract_seq_from_message(last_message) {
                let mut cursor = self.storage.get_sync_cursor(session_id).await?
                    .unwrap_or_else(|| SyncCursor::new(session_id.to_string()));
                
                // 扩展最近消息同步范围（如果加载的是最近消息的扩展）
                if let Some((recent_start, recent_end)) = cursor.recent_sync_range {
                    if last_seq < recent_start {
                        // 加载了更早的消息，扩展范围
                        cursor.update_recent_sync_range(last_seq, recent_end);
                    }
                }
                
                cursor.last_seq = Some(last_seq);
                self.storage.save_sync_cursor(session_id, &cursor).await?;
            }
        }
        
        Ok(SyncResult {
            session_id: session_id.to_string(),
            message_count: messages.len(),
            has_more: messages.len() >= limit,
            next_cursor: None,
            last_seq: messages.last().and_then(|m| Self::extract_seq_from_message(m)),
        })
    }

    /// 增量同步消息（基于 seq）
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `after_seq`: 同步此序列号之后的消息（可选，如果不提供则从游标获取）
    /// 
    /// # 返回
    /// - `Result<SyncResult>`: 同步结果
    /// 
    /// 如果任务系统已启用，将通过任务系统执行（可选）
    pub async fn sync_messages(
        &self,
        session_id: &str,
        after_seq: Option<i64>,
    ) -> Result<SyncResult> {
        // 如果任务系统已启用，使用任务系统执行
        if self.is_tasks_enabled().await {
            return self.sync_messages_via_task(session_id, after_seq).await;
        }
        
        // 否则直接执行（保持向后兼容）
        self.sync_messages_internal(session_id, after_seq).await
    }
    
    /// 消息同步内部实现（实际执行逻辑）
    pub(crate) async fn sync_messages_internal(
        &self,
        session_id: &str,
        after_seq: Option<i64>,
    ) -> Result<SyncResult> {
        debug!(
            session_id = %session_id,
            after_seq = ?after_seq,
            "Syncing messages"
        );
        
        // 0. 检查连接状态（快速检查，避免长时间等待）
        if !self.connection.is_connected().await {
            return Err(anyhow::anyhow!("Connection not established. Please ensure the client is connected before syncing messages."))
                .context("Failed to sync messages: connection not available");
        }
        
        // 1. 获取当前同步游标（如果没有提供 after_seq）
        let (start_seq, since_ts, cursor_str) = if let Some(seq) = after_seq {
            (seq, 0, String::new())
        } else {
            let cursor = self.storage.get_sync_cursor(session_id).await?;
            if let Some(c) = cursor {
                let seq = c.last_seq.unwrap_or(0);
                let ts = c.last_timestamp.unwrap_or(0);
                // 构建游标字符串（格式：seq:{seq}:{message_id}）
                let cursor_str = if let Some(msg_id) = c.last_message_id {
                    format!("seq:{}:{}", seq, msg_id)
                } else {
                    format!("seq:{}", seq)
                };
                (seq, ts, cursor_str)
            } else {
                (0, 0, String::new())
            }
        };
        
        // 2. 构建 SyncMessagesRequest
        use flare_proto::session::{SyncMessagesRequest, SyncMessagesResponse};
        
        let request = SyncMessagesRequest {
            user_id: self.user_id.read().await.clone(),
            session_id: session_id.to_string(),
            since_ts,
            cursor: cursor_str,
            limit: self.config.message_batch_size as i32,
            include_ack: true,
            ..Default::default()
        };
        
        // 3. 编码请求（优化：使用 encode_to_vec 直接编码）
        let request_bytes = request.encode_to_vec();
        
        // 4. 创建请求并发送
        let (request_id, response_rx) = self.request_manager.create_request().await;
        
        // 优化：预分配容量
        let mut metadata = std::collections::HashMap::with_capacity(2);
        metadata.insert("request_id".to_string(), request_id.as_bytes().to_vec());
        metadata.insert("command".to_string(), b"SyncMessages".to_vec());
        
        let frame = FrameBuilder::new()
            .with_custom_command(CustomCommand {
                name: "SyncMessages".to_string(),
                data: request_bytes,
                metadata,
            })
            .with_message_id(request_id.clone())
            .with_reliability(Reliability::AtLeastOnce)
            .build();
        
        // 发送请求前再次检查连接状态（防止在检查后连接断开）
        if !self.connection.is_connected().await {
            return Err(anyhow::anyhow!("Connection lost while preparing sync request. Please reconnect and try again."))
                .context("Failed to sync messages: connection lost");
        }
        
        self.connection.send_frame(&frame).await
            .map_err(|e| {
                // 检查是否是连接错误
                let err_msg = format!("{}", e);
                if err_msg.contains("Not connected") || err_msg.contains("connection") {
                    anyhow::anyhow!("Connection lost while sending sync request. Please reconnect and try again.")
                } else {
                    anyhow::anyhow!("Failed to send sync request: {}", e)
                }
            })
            .context("Failed to send SyncMessages request")?;
        
        // 5. 等待响应（带超时，优化超时时间）
        // 使用更短的超时时间，快速失败
        let timeout_duration = Duration::from_secs(self.config.request_timeout.min(10)); // 最多 10 秒
        let response_frame = match timeout(timeout_duration, response_rx).await {
            Ok(Ok(frame)) => frame,
            Ok(Err(e)) => {
                return Err(anyhow::anyhow!("Failed to receive response: {}", e))
                    .context("Failed to sync messages: response channel error");
            }
            Err(_) => {
                return Err(anyhow::anyhow!("Sync request timed out after {} seconds. The server may be busy or the connection may be slow.", timeout_duration.as_secs()))
                    .context("Failed to sync messages: request timeout");
            }
        };
        
        // 6. 解码响应：支持系统错误帧，将错误信息反馈给调用方
        let response = {
            use flare_core::common::protocol::flare::core::commands::command::Type as CommandType;
            match response_frame.command.as_ref().and_then(|c| c.r#type.as_ref()) {
                Some(CommandType::Custom(custom_cmd)) => {
                    let data = &custom_cmd.data;
                    SyncMessagesResponse::decode(&data[..])
                        .context("Failed to decode SyncMessagesResponse")?
                }
                Some(CommandType::System(sys_cmd)) => {
                    use flare_core::common::protocol::flare::core::commands::system_command::Type as SystemType;
                    if sys_cmd.r#type == SystemType::Error as i32 {
                        let msg = sys_cmd.message.clone();
                        return Err(anyhow::anyhow!(msg)).context("Server returned error for SyncMessages");
                    } else {
                        return Err(anyhow::anyhow!("Invalid response: unexpected System command")).context("Invalid response type");
                    }
                }
                _ => {
                    return Err(anyhow::anyhow!("Invalid response: not a CustomCommand")).context("Invalid response type");
                }
            }
        };
        
        // 7. 处理响应：保存消息并解决冲突
        // 优化：批量检查已存在的消息，减少数据库查询
        let message_ids: Vec<String> = response.messages.iter()
            .map(|m| m.id.clone())
            .collect();
        let user_id = self.user_id.read().await.clone();
        let existing_deleted: std::collections::HashSet<String> = self.storage
            .batch_check_deleted(&user_id, &message_ids)
            .await
            .unwrap_or_default()
            .into_iter()
            .collect();
        
        // 批量获取已存在的消息（优化：使用批量查询接口，减少数据库往返）
        // 优化：预分配容量
        let uncached_ids: Vec<String> = message_ids.iter()
            .filter(|id| !existing_deleted.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        
        // 优化：预分配 HashMap 容量
        let existing_messages: std::collections::HashMap<String, Message> = if !uncached_ids.is_empty() {
            // 使用批量查询接口（更高效）
            let messages = self.storage.batch_get_messages(&uncached_ids).await
                .unwrap_or_default();
            let mut map = std::collections::HashMap::with_capacity(messages.len());
            for m in messages {
                map.insert(m.id.clone(), m);
            }
            map
        } else {
            // 优化：即使为空也预分配小容量，避免后续插入时的重新分配
            std::collections::HashMap::with_capacity(0)
        };
        
        let mut message_count = 0;
        let mut last_seq = start_seq;
        
        for msg_proto in &response.messages {
            let message: Message = msg_proto.clone();
            
            // 跳过已删除的消息
            if existing_deleted.contains(&message.id) {
                continue;
            }
            
            // 检查冲突：如果消息已存在且 seq 相同，则比较时间戳
            if let Some(existing_msg) = existing_messages.get(&message.id) {
                if let (Some(existing_seq), Some(new_seq)) = (
                    Self::extract_seq_from_message(&existing_msg),
                    Self::extract_seq_from_message(&message),
                ) {
                    if existing_seq == new_seq {
                        // 冲突：比较时间戳，保留更新的
                        let existing_ts = existing_msg.timestamp
                            .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000))
                            .unwrap_or(0);
                        let new_ts = message.timestamp
                            .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000))
                            .unwrap_or(0);
                        
                        if new_ts > existing_ts {
                            // 新消息时间戳更新，覆盖
                            self.storage.save_message(&message).await
                                .context("Failed to save message")?;
                            message_count += 1;
                        }
                        // 否则保留现有消息
                        continue;
                    }
                }
            }
            
            // 保存新消息
            self.storage.save_message(&message).await
                .context("Failed to save message")?;
            message_count += 1;
            
            // 更新 last_seq
            if let Some(seq) = Self::extract_seq_from_message(&message) {
                if seq > last_seq {
                    last_seq = seq;
                }
            }
        }
        
        // 8. 更新同步游标
        if message_count > 0 {
            let mut cursor = self.storage.get_sync_cursor(session_id).await?
                .unwrap_or_else(|| SyncCursor::new(session_id.to_string()));
            
            // 从最后一条消息获取时间戳和 ID
            let last_message = response.messages.last();
            let last_timestamp = last_message
                .and_then(|msg| msg.timestamp.as_ref())
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000));
            let last_message_id = last_message
                .map(|msg| msg.id.clone());
            
            cursor.update(
                Some(last_seq),
                last_timestamp,
                last_message_id,
            );
            
            self.storage.save_sync_cursor(session_id, &cursor).await
                .context("Failed to save sync cursor")?;
        }
        
        // 判断是否有更多消息（根据返回的消息数量和 next_cursor 判断）
        // 注意：SyncMessagesResponse 没有 has_more 字段，需要根据消息数量和 next_cursor 判断
        let has_more = message_count >= self.config.message_batch_size || !response.next_cursor.is_empty();
        
        // 解析 next_cursor（如果响应中有）
        let next_cursor = if !response.next_cursor.is_empty() {
            // 解析游标字符串（格式可能是 "seq:{seq}:{message_id}" 或时间戳格式）
            let mut cursor = SyncCursor::new(session_id.to_string());
            
            if response.next_cursor.starts_with("seq:") {
                // 格式：seq:{seq}:{message_id}
                let parts: Vec<&str> = response.next_cursor.split(':').collect();
                if parts.len() >= 2 {
                    if let Ok(seq) = parts[1].parse::<i64>() {
                        cursor.last_seq = Some(seq);
                        if parts.len() >= 3 {
                            cursor.last_message_id = Some(parts[2].to_string());
                        }
                    }
                }
            } else if let Ok(ts) = response.next_cursor.parse::<i64>() {
                // 时间戳格式
                cursor.last_timestamp = Some(ts);
            }
            
            Some(cursor)
        } else {
            None
        };
        
        Ok(SyncResult {
            session_id: session_id.to_string(),
            message_count,
            has_more,
            next_cursor,
            last_seq: if last_seq > start_seq { Some(last_seq) } else { None },
        })
    }
    
    /// 通过任务系统执行消息同步
    async fn sync_messages_via_task(&self, session_id: &str, after_seq: Option<i64>) -> Result<SyncResult> {
        let executor = Arc::new(MessageSyncTask::new(
            Arc::new(self.clone()),
            session_id.to_string(),
            after_seq,
        ));
        let task_id = format!("message-sync-{}-{}", session_id, uuid::Uuid::new_v4());
        let mut task = SyncTask::new(task_id.clone(), executor);
        
        let context = self.create_sync_context().await?;
        
        // 执行任务
        task.execute(&context).await?;
        
        // 保存任务到运行列表
        self.running_tasks.lock().await.insert(task_id, task.clone());
        
        // 从任务结果中提取 SyncResult
        if let Some(result) = task.result {
            if result.success {
                // 需要从实际执行中获取 SyncResult
                // 这里我们需要调用实际的 sync_messages_internal
                return self.sync_messages_internal(session_id, after_seq).await;
            } else {
                return Err(anyhow::anyhow!("Message sync task failed: {}", result.error.unwrap_or_default()));
            }
        }
        
        // 如果任务没有返回结果，直接调用内部方法
        self.sync_messages_internal(session_id, after_seq).await
    }
    
    /// 处理收到的响应 Frame
    #[tracing::instrument(name = "sync.handle_response", skip(self, frame), fields(msg_id = %frame.message_id))]
    pub async fn handle_response(&self, frame: flare_core::common::protocol::Frame) -> Result<()> {
        // 从 Frame 的 metadata 中提取 request_id
        let request_id = frame.metadata
            .get("request_id")
            .and_then(|v| String::from_utf8(v.clone()).ok())
            .or_else(|| Some(frame.message_id.clone()))
            .context("No request_id in response")?;
        
        debug!(%request_id, has_command = frame.command.is_some(), meta_len = frame.metadata.len(), "Completing pending request");
        
        // 完成请求
        self.request_manager.complete_request(&request_id, frame).await
            .context("Failed to complete request")?;
        
        Ok(())
    }

    /// 增量同步会话列表
    /// 
    /// # 参数
    /// - `cursor`: 可选游标，用于分页
    /// 
    /// # 返回
    /// - `Result<SessionSyncResult>`: 同步结果
    /// 
    /// 如果任务系统已启用，将通过任务系统执行（可选）
    pub async fn sync_sessions(&self, cursor: Option<String>) -> Result<crate::service::SessionSyncResult> {
        // 如果任务系统已启用，使用任务系统执行
        if self.is_tasks_enabled().await {
            return self.sync_sessions_via_task(cursor).await;
        }
        
        // 否则直接执行（保持向后兼容）
        self.sync_sessions_internal(cursor).await
    }
    
    /// 通过任务系统执行会话同步
    async fn sync_sessions_via_task(&self, cursor: Option<String>) -> Result<crate::service::SessionSyncResult> {
        let executor = Arc::new(SessionSyncTask::new(
            Arc::new(self.clone()),
            cursor.clone(),
        ));
        let task_id = format!("session-sync-{}", uuid::Uuid::new_v4());
        let mut task = SyncTask::new(task_id.clone(), executor);
        
        let context = self.create_sync_context().await?;
        
        // 执行任务（任务执行器内部会调用 sync_sessions_internal）
        task.execute(&context).await?;
        
        // 保存任务到运行列表
        self.running_tasks.lock().await.insert(task_id, task.clone());
        
        // 从任务结果中提取 SessionSyncResult
        if let Some(result) = task.result {
            if result.success {
                // 任务执行器已经调用了 sync_sessions_internal，我们需要再次调用以获取结果
                return self.sync_sessions_internal(cursor).await;
            } else {
                return Err(anyhow::anyhow!("Session sync task failed: {}", result.error.unwrap_or_default()));
            }
        }
        
        // 如果任务没有返回结果，直接调用内部方法
        self.sync_sessions_internal(cursor).await
    }
    
    /// 会话同步内部实现（实际执行逻辑）
    pub(crate) async fn sync_sessions_internal(&self, cursor: Option<String>) -> Result<crate::service::SessionSyncResult> {
        debug!(cursor = ?cursor, "Syncing sessions");
        
        // 检查连接状态（快速检查，避免长时间等待）
        // 如果未连接，快速失败，不等待
        if !self.connection.is_connected().await {
            return Err(anyhow::anyhow!("Connection not established. Please ensure the client is connected before syncing sessions."))
                .context("Failed to sync sessions: connection not available");
        }
        
        use flare_proto::session::{ListSessionsRequest, ListSessionsResponse};
        
        // 1. 构建 ListSessionsRequest
        let request = ListSessionsRequest {
            user_id: self.user_id.read().await.clone(),
            cursor: cursor.unwrap_or_default(),
            limit: self.config.session_batch_size as i32,
            order: 0,
            ..Default::default()
        };
        
        // 2. 编码请求（优化：使用 encode_to_vec 直接编码）
        let request_bytes = request.encode_to_vec();
        debug!(bytes = request_bytes.len(), cursor = %request.cursor, limit = request.limit, "Encoded ListSessionsRequest");
        
        // 3. 创建请求并发送
        let (request_id, response_rx) = self.request_manager.create_request().await;
        
        // 优化：预分配容量
        let mut metadata = std::collections::HashMap::with_capacity(2);
        metadata.insert("request_id".to_string(), request_id.as_bytes().to_vec());
        metadata.insert("command".to_string(), b"ListSessions".to_vec());
        
        let frame = FrameBuilder::new()
            .with_custom_command(CustomCommand {
                name: "ListSessions".to_string(),
                data: request_bytes,
                metadata,
            })
            .with_message_id(request_id.clone())
            .with_reliability(Reliability::AtLeastOnce)
            .build();
        
        debug!(msg_id = %frame.message_id, ts = frame.timestamp, "Sending ListSessions frame");
        self.connection.send_frame(&frame).await
            .context("Failed to send ListSessions request")?;
        
        // 4. 等待响应（带超时）
        let timeout_duration = Duration::from_secs(self.config.request_timeout);
        let response_frame = timeout(timeout_duration, response_rx)
            .await
            .context("Request timeout")?
            .map_err(|e| anyhow::anyhow!("Failed to receive response: {}", e))?;
        let cmd_kind = response_frame.command.as_ref()
            .and_then(|c| c.r#type.as_ref())
            .map(|t| match t {
                flare_core::common::protocol::flare::core::commands::command::Type::Custom(_) => "Custom",
                flare_core::common::protocol::flare::core::commands::command::Type::System(_) => "System",
                flare_core::common::protocol::flare::core::commands::command::Type::Message(_) => "Message",
                flare_core::common::protocol::flare::core::commands::command::Type::Notification(_) => "Notification",
            }).unwrap_or("<none>");
        let req_id_meta = response_frame.metadata.get("request_id").and_then(|v| String::from_utf8(v.clone()).ok()).unwrap_or_default();
        debug!(msg_id = %response_frame.message_id, cmd = %cmd_kind, %req_id_meta, "Received ListSessions response frame");
        
        // 5. 解码响应：支持系统错误帧，将错误信息反馈给调用方
        let response = {
            use flare_core::common::protocol::flare::core::commands::command::Type as CommandType;
            match response_frame.command.as_ref().and_then(|c| c.r#type.as_ref()) {
                Some(CommandType::Custom(custom_cmd)) => {
                    let data = &custom_cmd.data;
                    ListSessionsResponse::decode(&data[..])
                        .context("Failed to decode ListSessionsResponse")?
                }
                Some(CommandType::System(sys_cmd)) => {
                    use flare_core::common::protocol::flare::core::commands::system_command::Type as SystemType;
                    if sys_cmd.r#type == SystemType::Error as i32 {
                        let msg = sys_cmd.message.clone();
                        return Err(anyhow::anyhow!(msg)).context("Server returned error for ListSessions");
                    } else {
                        return Err(anyhow::anyhow!("Invalid response: unexpected System command")).context("Invalid response type");
                    }
                }
                _ => {
                    return Err(anyhow::anyhow!("Invalid response: not a CustomCommand")).context("Invalid response type");
                }
            }
        };
        
        // 6. 处理响应：保存会话并解决冲突
        // 优化：预分配 Vec 容量
        let mut synced_sessions = Vec::with_capacity(response.sessions.len());
        
        // 优化：批量获取已存在的会话，减少数据库查询
        let session_ids: Vec<String> = response.sessions.iter()
            .map(|s| s.session_id.clone())
            .collect();
        
        // 并行获取已存在的会话
        let existing_sessions: std::collections::HashMap<String, SessionSummary> = {
            // 优化：预分配 Vec 容量
            let mut fetch_tasks = Vec::with_capacity(session_ids.len());
            for session_id in &session_ids {
                let storage = Arc::clone(&self.storage);
                let id = session_id.clone();
                fetch_tasks.push(tokio_spawn(async move {
                    storage.get_session(&id).await.map(|opt| opt.map(|s| (id, s)))
                }));
            }
            // 优化：预分配 HashMap 容量
            let mut map = std::collections::HashMap::with_capacity(fetch_tasks.len());
            for task in fetch_tasks {
                if let Ok(Ok(Some((id, session)))) = task.await {
                    map.insert(id, session);
                }
            }
            map
        };
        
        for session_proto in &response.sessions {
            let session: SessionSummary = session_proto.clone().into();
            
            // 检查冲突：如果会话已存在，比较最后消息时间，保留更新的
            if let Some(existing_session) = existing_sessions.get(&session.session_id) {
                let existing_ts = existing_session.last_message_time.unwrap_or(0);
                let new_ts = session.last_message_time.unwrap_or(0);
                
                if new_ts > existing_ts {
                    // 新会话时间戳更新，覆盖
                    self.storage.save_session(&session).await
                        .context("Failed to save session")?;
                    synced_sessions.push(session.clone());
                } else {
                    // 保留现有会话
                    synced_sessions.push(existing_session.clone());
                }
            } else {
                // 新会话，直接保存
                self.storage.save_session(&session).await
                    .context("Failed to save session")?;
                synced_sessions.push(session);
            }
        }
        
        let count = synced_sessions.len();
        info!(count, has_more = response.has_more, next_cursor = %response.next_cursor, "ListSessions decoded and saved");
        
        Ok(crate::service::SessionSyncResult {
            sessions: synced_sessions,
            has_more: response.has_more,
            next_cursor: if response.next_cursor.is_empty() {
                None
            } else {
                Some(response.next_cursor)
            },
            count,
        })
    }

    /// 启动重连同步监听器
    /// 
    /// 监听重连事件，自动触发增量同步
    /// 启动重连同步监听器
    /// 
    /// # 参数
    /// - `strategy`: 重连同步策略（可选，如果不提供则使用默认策略）
    /// 
    /// # 注意
    /// - 此方法会监听连接事件，当检测到重连成功时，根据策略自动触发同步
    pub async fn start_reconnect_sync_listener(
        &self,
        strategy: Option<ReconnectSyncStrategy>,
    ) -> Result<()> {
        // 保存策略
        {
            let mut strategy_guard = self.reconnect_strategy.lock().await;
            *strategy_guard = Some(strategy.unwrap_or_default());
        }
        
        // 启动监听器
        let event_bus = Arc::clone(&self.event_bus);
        let sync_service = Arc::new(self.clone_for_listener());
        let tasks = Arc::clone(&self.tasks);
        let reconnect_strategy = Arc::clone(&self.reconnect_strategy);
        
        tokio_spawn(async move {
            let mut event_rx = event_bus.subscribe();
            let mut last_disconnect_time: Option<Instant> = None;
            
            while let Ok(event) = event_rx.recv().await {
                match event {
                    Event::Connection(ConnectionEvent::Disconnected) => {
                        last_disconnect_time = Some(Instant::now());
                        info!("Connection disconnected, will sync on reconnect");
                    }
                    Event::Connection(ConnectionEvent::Authenticated) => {
                        // 重连成功，检查是否需要同步
                        let strategy_guard = reconnect_strategy.lock().await;
                        if let Some(strategy) = strategy_guard.as_ref() {
                            if !strategy.auto_sync {
                                continue;
                            }
                            
                            // 计算离线时间
                            let offline_duration = last_disconnect_time
                                .map(|t| t.elapsed())
                                .unwrap_or(Duration::from_secs(0));
                            
                            // 根据策略决定同步模式
                            let sync_mode = match &strategy.sync_mode {
                                ReconnectSyncMode::None => {
                                    continue; // 不执行同步
                                }
                                ReconnectSyncMode::Full => ReconnectSyncMode::Full,
                                ReconnectSyncMode::Incremental => ReconnectSyncMode::Incremental,
                                ReconnectSyncMode::Smart { full_sync_threshold } => {
                                    if offline_duration > *full_sync_threshold {
                                        ReconnectSyncMode::Full
                                    } else {
                                        ReconnectSyncMode::Incremental
                                    }
                                }
                            };
                            
                            // 延迟执行同步
                            tokio::time::sleep(strategy.sync_delay).await;
                            
                            // 执行同步
                            let sync_service_clone = Arc::clone(&sync_service);
                            let tasks_clone = Arc::clone(&tasks);
                            let strategy_clone = strategy.clone();
                            tokio_spawn(async move {
                                // 使用超时执行同步
                                match timeout(strategy_clone.sync_timeout, async {
                                    sync_service_clone.trigger_sync(sync_mode, tasks_clone).await
                                }).await {
                                    Ok(Ok(_)) => {
                                        info!("Reconnect sync completed");
                                    }
                                    Ok(Err(e)) => {
                                        error!(error = %e, "Failed to trigger reconnect sync");
                                    }
                                    Err(_) => {
                                        error!("Reconnect sync timeout");
                                    }
                                }
                            });
                        }
                    }
                    _ => {}
                }
            }
        });
        
        Ok(())
    }
    
    /// 触发同步（内部方法，用于重连同步）
    async fn trigger_sync(
        &self,
        mode: ReconnectSyncMode,
        tasks: Arc<Mutex<HashMap<String, SyncTask>>>,
    ) -> Result<()> {
        match mode {
            ReconnectSyncMode::Full => {
                // 使用任务系统执行全量同步
                let executor = Arc::new(FullSyncTask::new(Arc::new(self.clone())));
                let task_id = format!("full-sync-{}", new_task_id());
                let mut task = SyncTask::new(task_id.clone(), executor);
                
                let context = self.create_sync_context().await?;
                
                // 执行任务
                if let Err(e) = task.execute(&context).await {
                    warn!(error = %e, "Full sync task failed");
                }
                
                // 保存任务
                tasks.lock().await.insert(task_id, task);
            }
            ReconnectSyncMode::Incremental => {
                // 增量同步所有会话
                let filter = crate::storage::SessionFilter {
                    session_type: None,
                    business_type: None,
                    unread_only: false,
                    limit: None,
                    offset: None,
                };
                let sessions = self.storage.get_sessions(filter).await?;
                for session in sessions {
                    // 获取本地最大seq
                    if let Some(local_max_seq) = self.storage.get_max_seq(&session.session_id).await? {
                        let executor = Arc::new(MessageSyncTask::new(
                            Arc::new(self.clone()),
                            session.session_id.clone(),
                            Some(local_max_seq),
                        ));
                        let task_id = format!("message-sync-{}-{}", session.session_id, new_task_id());
                        let mut task = SyncTask::new(task_id.clone(), executor);
                        
                        let context = self.create_sync_context().await?;
                        
                        // 执行任务
                        if let Err(e) = task.execute(&context).await {
                            warn!(error = %e, session_id = %session.session_id, "Message sync task failed");
                        }
                        
                        // 保存任务
                        tasks.lock().await.insert(task_id, task);
                    }
                }
            }
            _ => {}
        }
        
        Ok(())
    }
    
    /// 克隆同步服务（用于监听器）
    fn clone_for_listener(&self) -> Self {
        Self {
            connection: Arc::clone(&self.connection),
            storage: Arc::clone(&self.storage),
            event_bus: Arc::clone(&self.event_bus),
            request_manager: Arc::clone(&self.request_manager),
            config: self.config.clone(),
            user_id: self.user_id.clone(),
            tasks: Arc::clone(&self.tasks),
            task_executors: Arc::clone(&self.task_executors),
            running_tasks: Arc::clone(&self.running_tasks),
            tasks_enabled: Arc::clone(&self.tasks_enabled),
            reconnect_strategy: Arc::clone(&self.reconnect_strategy),
        }
    }
    
    /// 获取同步任务列表
    pub async fn get_sync_tasks(&self) -> Vec<SyncTask> {
        let tasks_guard = self.tasks.lock().await;
        tasks_guard.values().cloned().collect()
    }
    
    /// 取消同步任务
    pub async fn cancel_sync_task(&self, task_id: &str) -> Result<()> {
        let mut tasks_guard = self.tasks.lock().await;
        if let Some(task) = tasks_guard.get_mut(task_id) {
            task.status = TaskStatus::Cancelled;
        }
        Ok(())
    }
    
    /// 手动触发同步
    /// 
    /// # 参数
    /// - `mode`: 同步模式
    /// 
    /// # 返回
    /// - `Result<SyncTask>`: 同步任务
    pub async fn trigger_sync_manual(&self, mode: ReconnectSyncMode) -> Result<SyncTask> {
        let executor: Arc<dyn SyncTaskExecutor> = match mode {
            ReconnectSyncMode::Full => {
                Arc::new(FullSyncTask::new(Arc::new(self.clone())))
            }
            ReconnectSyncMode::Incremental => {
                // 对于增量同步，创建一个会话同步任务
                Arc::new(SessionSyncTask::new(
                    Arc::new(self.clone()),
                    None,
                ))
            }
            _ => {
                Arc::new(SessionSyncTask::new(
                    Arc::new(self.clone()),
                    None,
                ))
            }
        };
        
        let task_id = format!("manual-sync-{}", new_task_id());
        let task = SyncTask::new(task_id.clone(), executor);
        
        self.tasks.lock().await.insert(task_id.clone(), task.clone());
        
        // 异步执行同步
        let sync_service = Arc::new(self.clone_for_listener());
        let tasks = Arc::clone(&self.tasks);
        tokio_spawn(async move {
            if let Err(e) = sync_service.trigger_sync(mode, tasks.clone()).await {
                if let Some(task) = tasks.lock().await.get_mut(&task_id) {
                    task.status = TaskStatus::Failed;
                    task.error = Some(e.to_string());
                }
            }
        });
        
        Ok(task)
    }
    

    /// 重连后的增量同步
    /// 
    /// 检测离线时间，同步所有会话的增量消息
    async fn sync_after_reconnect(&self) -> Result<()> {
        let start_time = Instant::now();
        info!("Starting sync after reconnect");
        
        // 发布同步开始事件
        self.event_bus.publish(Event::Sync(SyncEvent::SyncStarted {
            sync_type: "incremental".to_string(),
            estimated_sessions: None,
        }));
        
        // 1. 获取所有会话的同步游标
        let cursors = self.storage.get_all_sync_cursors().await
            .context("Failed to get sync cursors")?;
        
        let mut total_message_count = 0;
        let mut synced_sessions = 0;
        
        // 2. 对每个会话进行增量同步
        for cursor in cursors {
            if let Some(last_seq) = cursor.last_seq {
                match self.sync_messages(&cursor.session_id, Some(last_seq)).await {
                    Ok(result) => {
                        total_message_count += result.message_count;
                        synced_sessions += 1;
                        debug!(
                            session_id = %cursor.session_id,
                            message_count = result.message_count,
                            "Synced messages for session"
                        );
                    }
                    Err(e) => {
                        warn!(
                            error = %e,
                            session_id = %cursor.session_id,
                            "Failed to sync messages for session"
                        );
                    }
                }
            }
        }
        
        // 3. 同步会话列表（增量）
        match self.sync_sessions(None).await {
            Ok(_) => {
                debug!("Synced sessions after reconnect");
            }
            Err(e) => {
                warn!(error = %e, "Failed to sync sessions after reconnect");
            }
        }
        
        info!(
            synced_sessions,
            total_message_count,
            "Sync after reconnect completed"
        );
        
        let duration_ms = start_time.elapsed().as_millis() as u64;
        
        // 发布同步完成事件
        self.event_bus.publish(Event::Sync(SyncEvent::SyncCompleted {
            sessions: synced_sessions,
            messages: total_message_count,
            duration_ms,
            has_background_sync: false,
        }));
        
        Ok(())
    }

    /// 从消息的 extra 字段中提取 seq
    /// 查询最近消息（层级1：用于UI显示）
    /// 
    /// 从服务器获取最近N条消息
    /// 
    /// **实现策略**：
    /// - 优先从本地获取（如果已有足够消息）
    /// - 如果本地消息不足，从服务器同步最近消息
    /// - 只同步消息实体，不下载所有历史消息
    async fn query_recent_messages(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<Message>> {
        // 1. 先检查本地是否有足够消息
        let local_messages = self.storage.get_messages(session_id, limit, None).await?;
        
        if local_messages.len() >= limit {
            // 本地已有足够消息，直接返回
            return Ok(local_messages);
        }
        
        // 2. 本地消息不足，从服务器同步最近消息
        // 使用 sync_messages 方法，从当前游标开始同步
        let _result = self.sync_messages(session_id, None).await?;
        
        // 3. 再次从本地获取（同步后应该有足够消息）
        self.storage.get_messages(session_id, limit, None).await
    }

    /// 查询会话信息（层级2：获取游标信息）
    /// 
    /// 从服务器获取会话信息，包含 max_seq 和 unread_count
    async fn query_session_info(&self, session_id: &str) -> Result<SessionSummary> {
        // 从本地获取会话信息
        if let Some(session) = self.storage.get_session(session_id).await? {
            return Ok(session);
        }
        
        // 如果本地没有，从服务器同步会话列表
        let sync_result = self.sync_sessions(None).await?;
        
        sync_result.sessions
            .into_iter()
            .find(|s| s.session_id == session_id)
            .ok_or_else(|| anyhow::anyhow!("Session not found: {}", session_id))
    }

    /// 查询指定序列号范围的消息（层级3：按需加载历史消息）
    /// 
    /// 从服务器获取指定序列号范围的消息
    /// 
    /// **注意**：这里加载的是更早的消息（向前加载历史消息）
    /// 例如：如果本地最小seq是100，则加载seq < 100的消息
    async fn query_messages_by_seq(
        &self,
        session_id: &str,
        after_seq: i64,
        limit: usize,
    ) -> Result<Vec<Message>> {
        // 1. 先检查本地是否有这些消息
        let local_messages = self.storage.get_messages_by_seq(session_id, after_seq, limit).await?;
        
        if local_messages.len() >= limit {
            // 本地已有足够消息
            return Ok(local_messages);
        }
        
        // 2. 本地消息不足，从服务器同步
        // 注意：这里需要从 after_seq 向前加载更早的消息
        // 实际实现需要根据服务端API调整（可能需要 before_seq 参数）
        let _result = self.sync_messages(session_id, Some(after_seq)).await?;
        
        // 3. 再次从本地获取
        self.storage.get_messages_by_seq(session_id, after_seq, limit).await
    }

    /// 全量同步消息（不推荐，用于特殊情况）
    async fn sync_messages_full(&self, session_id: &str) -> Result<SyncResult> {
        // 从seq 0开始，持续同步直到没有更多消息
        let mut after_seq = 0;
        let mut total_count = 0;
        
        loop {
            let result = self.sync_messages(session_id, Some(after_seq)).await?;
            
            if result.message_count == 0 {
                break;
            }
            
            total_count += result.message_count;
            
            if let Some(last_seq) = result.last_seq {
                after_seq = last_seq;
            } else {
                break;
            }
            
            if !result.has_more {
                break;
            }
        }
        
        Ok(SyncResult {
            session_id: session_id.to_string(),
            message_count: total_count,
            has_more: false,
            next_cursor: None,
            last_seq: Some(after_seq),
        })
    }

    /// 从消息中提取序列号（优先顶层字段，其次 extra）
    fn extract_seq_from_message(message: &Message) -> Option<i64> {
        if message.seq > 0 { Some(message.seq) } else {
            message.extra
                .get("seq")
                .and_then(|seq_str| seq_str.parse::<i64>().ok())
        }
    }
}

// 为 SyncService 实现 Clone（用于在异步任务中使用）
impl Clone for SyncService {
    fn clone(&self) -> Self {
        Self {
            connection: Arc::clone(&self.connection),
            storage: Arc::clone(&self.storage),
            event_bus: Arc::clone(&self.event_bus),
            request_manager: Arc::clone(&self.request_manager),
            config: self.config.clone(),
            user_id: self.user_id.clone(),
            tasks: Arc::clone(&self.tasks),
            task_executors: Arc::clone(&self.task_executors),
            running_tasks: Arc::clone(&self.running_tasks),
            tasks_enabled: Arc::clone(&self.tasks_enabled),
            reconnect_strategy: Arc::clone(&self.reconnect_strategy),
        }
    }

}

impl SyncService {
    pub async fn set_user_id(&self, user_id: String) {
        let mut guard = self.user_id.write().await;
        *guard = user_id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_sync_config_default() {
        let config = SyncConfig::default();
        assert_eq!(config.message_batch_size, 100);
        assert_eq!(config.session_batch_size, 50);
        assert_eq!(config.request_timeout, 30);
        assert_eq!(config.recent_message_limit, 50);
    }
}
#[cfg(target_arch = "wasm32")]
fn new_task_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("task-{}-{}", ts, c)
}

#[cfg(not(target_arch = "wasm32"))]
fn new_task_id() -> String { uuid::Uuid::new_v4().to_string() }
