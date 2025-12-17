//! Flare IM Client - SDK 主入口
//!
//! 整合所有模块，提供统一的 API
//!
//! ## 架构设计
//!
//! API 层采用模块化设计，按功能域拆分：
//!
//! - `client.rs`: 客户端主入口和结构体定义
//! - `traits.rs`: API trait 定义（ConnectionApi, SessionApi, MessageApi 等）
//! - `connection.rs`: 连接管理 API 实现
//! - `session.rs`: 会话管理 API 实现
//! - `message.rs`: 消息管理 API 实现
//! - `event.rs`: 事件通知 API 实现
//! - `sync.rs`: 数据同步 API 实现
//! - `extension.rs`: 扩展功能 API 实现
//! - `utility.rs`: 工具方法 API 实现
//!
//! ## 使用方式
//!
//! 所有 API 通过 `FlareIMClient` 统一入口访问，保持向后兼容：
//!
//! ```rust,no_run
//! let client = FlareIMClient::new(config).await?;
//! client.login("user_123", "token").await?;
//! client.send_message("session_123", content).await?;
//! ```

use crate::application::handlers::{
    MessageCommandHandler, MessageQueryHandler, SessionCommandHandler, SessionQueryHandler,
    SyncCommandHandler, SyncQueryHandler,
};
use crate::domain::message::repository::MessageRepository;
use crate::domain::message::service::{MessageDomainService, MessageDomainServiceImpl};
use crate::domain::session::repository::SessionRepository;
use crate::domain::session::service::{SessionDomainService, SessionDomainServiceImpl};
use crate::domain::sync::repository::SyncRepository;
use crate::domain::sync::service::{SyncDomainService, SyncDomainServiceImpl};
use crate::infrastructure::connection::ConnectionManager;
use crate::infrastructure::event::{ConnectionEvent, Event, EventBus};
use crate::infrastructure::persistence::storage::{
    MessageRepositoryImpl, SessionRepositoryImpl, SyncRepositoryImpl,
};
use crate::infrastructure::storage::StorageBackend;
use crate::infrastructure::task::{
    TaskManager, TaskManagerBuilder, TaskScheduler, TaskSchedulerConfig,
};
use crate::shared::config::ClientConfig;
#[cfg(feature = "extensions")]
use crate::shared::extension::ExtensionInfoManager as ExtensionManager;
#[cfg(debug_assertions)]
use crate::shared::memory_leak_detector::MemoryLeakDetector;
use crate::shared::metrics::Metrics;
use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::warn;

#[cfg(not(target_arch = "wasm32"))]
use tokio::spawn as tokio_spawn;
use tokio::sync::RwLock;
#[cfg(target_arch = "wasm32")]
use tokio::task::spawn_local as tokio_spawn;

/// Flare IM 客户端主入口
///
/// 整合所有模块，提供统一的 API
pub struct FlareIMClient {
    /// 连接管理器
    pub(crate) connection: Arc<ConnectionManager>,

    /// 消息命令处理器
    pub(crate) message_command_handler: Arc<MessageCommandHandler>,

    /// 消息查询处理器
    pub(crate) message_query_handler: Arc<MessageQueryHandler>,

    /// 会话命令处理器
    pub(crate) session_command_handler: Arc<SessionCommandHandler>,

    /// 会话查询处理器
    pub(crate) session_query_handler: Arc<SessionQueryHandler>,

    /// 同步命令处理器
    pub(crate) sync_command_handler: Arc<SyncCommandHandler>,

    /// 同步查询处理器
    pub(crate) sync_query_handler: Arc<SyncQueryHandler>,

    /// 本地存储
    pub(crate) storage: Arc<dyn StorageBackend>,

    /// 事件总线
    pub(crate) event_bus: Arc<EventBus>,

    /// 配置
    pub(crate) config: Arc<tokio::sync::RwLock<ClientConfig>>,

    /// 当前用户 ID
    pub(crate) user_id: Arc<RwLock<String>>,

    /// 消息帧处理器
    pub(crate) message_frame_handler: Arc<crate::infrastructure::handler::MessageFrameHandler>,

    /// 消息观察者注册表
    pub(crate) observer_registry: Arc<crate::shared::observer::MessageObserverRegistry>,

    /// 扩展管理器（用于填充扩展信息）
    /// 如果启用了 extensions feature，则必需；否则使用空实现
    #[cfg(feature = "extensions")]
    pub(crate) extension_manager: Arc<ExtensionManager>,

    /// 业务扩展注册中心（用于管理业务扩展点）
    #[cfg(feature = "extensions")]
    pub(crate) business_extension_registry:
        Arc<crate::shared::extension::BusinessExtensionRegistry>,

    /// 任务管理器（统一管理所有后台任务）
    pub(crate) task_manager: Arc<TaskManager>,

    /// 任务调度器（统一调度所有任务）
    pub(crate) task_scheduler: Arc<TaskScheduler>,

    /// 性能指标收集器
    pub(crate) metrics: Arc<Metrics>,

    /// 内存泄漏检测器（仅在 debug 模式启用）
    #[cfg(debug_assertions)]
    pub(crate) leak_detector: Arc<MemoryLeakDetector>,
}

/// 登录结果
#[derive(Debug, Clone)]
pub struct LoginResult {
    /// 用户 ID
    pub user_id: String,

    /// 会话 ID
    pub session_id: String,
}

impl FlareIMClient {
    /// 创建客户端实例（预初始化模式）
    ///
    /// 此方法创建客户端但不连接，适用于应用启动时预初始化
    /// 登录时只需要调用 `login()` 即可，无需重新创建客户端
    ///
    /// # 参数
    /// - `config`: 客户端配置（可以暂时不设置 user_id 和 token，登录时再设置）
    ///
    /// # 返回
    /// - `Result<FlareIMClient>`: 客户端实例（未连接状态）
    ///
    /// # 示例
    /// ```rust,no_run
    /// // 应用启动时预初始化
    /// let config = ClientConfig::builder()
    ///     .server_url("wss://im.example.com")
    ///     .device_id("device_123")
    ///     .build()?;
    /// let client = FlareIMClient::new(config).await?;
    ///
    /// // 登录时直接使用
    /// client.login("user_123", "token").await?;
    /// ```
    pub async fn new(config: ClientConfig) -> Result<Self> {
        // 0. 创建性能指标收集器（将在后面创建处理器时使用）
        // 注意：metrics 将在后面创建，这里先不创建

        // 0.1 创建内存泄漏检测器（仅在 debug 模式）
        #[cfg(debug_assertions)]
        let leak_detector = Arc::new(MemoryLeakDetector::default());

        // 1. 创建任务管理器（统一管理所有后台任务）
        let task_manager = Arc::new(
            TaskManagerBuilder::new()
                .shutdown_timeout(10) // 10 秒关闭超时
                .build(),
        );

        // 2. 创建事件总线
        let event_bus = Arc::new(EventBus::new());

        // 2.1 提前读取数据库路径（在 config 被移动之前）
        let db_path = config
            .db_path
            .clone()
            .or_else(|| std::env::var("FLARE_IM_DB_PATH").ok())
            .unwrap_or_else(|| "flare-im.db".to_string());

        // 3. 创建存储（根据平台自动选择）
        let base_storage: Arc<dyn StorageBackend> = {
            use crate::shared::platform::{Platform, get_platform};
            let platform = get_platform();

            match platform {
                Platform::Web => {
                    #[cfg(target_arch = "wasm32")]
                    {
                        use crate::infrastructure::storage::indexeddb::IndexedDBStorage;
                        Arc::new(
                            IndexedDBStorage::new("flare-im")
                                .await
                                .context("Failed to create IndexedDB storage")?,
                        )
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        anyhow::bail!("Web platform requires wasm32 target");
                    }
                }
                Platform::Desktop | Platform::Android | Platform::IOS | Platform::HarmonyOS => {
                    use crate::infrastructure::storage::sqlite::SqliteStorage;
                    Arc::new(
                        SqliteStorage::new(&db_path)
                            .await
                            .context("Failed to create SQLite storage")?,
                    )
                }
            }
        };

        // 3.1 应用缓存层（如果配置了缓存）
        // 注意：CachedStorageBackend 可能不存在，暂时直接使用 base_storage
        let storage: Arc<dyn StorageBackend> = base_storage;

        // 4. 创建连接管理器
        let config_arc = Arc::new(tokio::sync::RwLock::new(config.clone()));
        let connection = Arc::new(ConnectionManager::new(
            Arc::clone(&config_arc),
            Arc::clone(&event_bus),
        ));

        // 5. 创建用户 ID 锁
        let user_id = Arc::new(RwLock::new(config.user_id.clone()));

        // 6. 创建消息观察者注册表
        let observer_registry = Arc::new(crate::shared::observer::MessageObserverRegistry::new());

        // 7. 创建仓储实现
        let message_repository: Arc<dyn MessageRepository> =
            Arc::new(MessageRepositoryImpl::new(Arc::clone(&storage)));
        let session_repository: Arc<dyn SessionRepository> =
            Arc::new(SessionRepositoryImpl::new(Arc::clone(&storage)));
        let sync_repository: Arc<dyn SyncRepository> = Arc::new(SyncRepositoryImpl::new());

        // 8. 创建领域服务实现
        let message_domain_service: Arc<dyn MessageDomainService> =
            Arc::new(MessageDomainServiceImpl::with_storage(Arc::clone(&storage)));
        let session_domain_service: Arc<dyn SessionDomainService> =
            Arc::new(SessionDomainServiceImpl::new());
        let sync_domain_service: Arc<dyn SyncDomainService> =
            Arc::new(SyncDomainServiceImpl::new());

        // 8.5 创建监控指标（用于性能监控）
        let metrics = Arc::new(Metrics::new());

        // 8.6 创建待发送消息队列（用于消息重试）
        use crate::infrastructure::storage::PendingMessageQueue;
        let pending_message_queue = Arc::new(PendingMessageQueue::new(Arc::clone(&storage)));

        // 9. 创建处理器
        let message_command_handler = Arc::new(
            MessageCommandHandler::new(
                Arc::clone(&message_domain_service),
                Arc::clone(&message_repository),
                Arc::clone(&event_bus),
            )
            .with_connection_manager(Arc::clone(&connection))
            .with_pending_queue(Arc::clone(&pending_message_queue))
            .with_metrics(Arc::clone(&metrics)),
        );
        let message_query_handler =
            Arc::new(MessageQueryHandler::new(Arc::clone(&message_repository)));
        let session_command_handler = Arc::new(
            SessionCommandHandler::new(
                Arc::clone(&session_domain_service),
                Arc::clone(&session_repository),
                Arc::clone(&event_bus),
            )
            .with_connection_manager(Arc::clone(&connection)),
        );
        let session_query_handler =
            Arc::new(SessionQueryHandler::new(Arc::clone(&session_repository)));

        // 创建请求管理器（用于同步请求/响应）
        use crate::infrastructure::protocol::RequestManager;
        let request_manager = Arc::new(RequestManager::new());

        let sync_command_handler = Arc::new(
            SyncCommandHandler::new(
                Arc::clone(&sync_domain_service),
                Arc::clone(&sync_repository),
                Arc::clone(&message_repository),
                Arc::clone(&session_repository),
                Arc::clone(&event_bus),
                Arc::clone(&request_manager),
            )
            .with_connection_manager(Arc::clone(&connection)),
        );
        let sync_query_handler = Arc::new(SyncQueryHandler::new(Arc::clone(&sync_repository)));

        // 10. 创建消息应用服务
        use crate::application::services::MessageService;
        let message_service = Arc::new(MessageService::new(
            Arc::clone(&message_command_handler),
            Arc::clone(&message_query_handler),
            Arc::clone(&message_repository),
            Arc::clone(&event_bus),
        ));

        // 10.1 创建消息接收器
        use crate::application::receivers::MessageReceiver;
        let message_receiver = Arc::new(MessageReceiver::new(Arc::clone(&message_service)));

        // 10.2 创建消息帧处理器
        let message_frame_handler = Arc::new(
            crate::infrastructure::handler::MessageFrameHandler::new(
                Arc::clone(&message_receiver),
                Arc::clone(&message_command_handler),
                Arc::clone(&event_bus),
            )
            .with_connection_manager(Arc::clone(&connection)),
        );

        // 10.1 创建消息监听器并设置到连接管理器
        use crate::infrastructure::connection::message_listener::SDKMessageListener;
        let message_listener = Arc::new(SDKMessageListener::new(
            Arc::clone(&message_frame_handler),
            Arc::clone(&sync_command_handler),
            Arc::clone(&event_bus),
        ));
        connection
            .set_message_listener(Arc::clone(&message_listener))
            .await;

        // 11. 创建扩展管理器（如果启用了 extensions feature）
        #[cfg(feature = "extensions")]
        let extension_manager = Arc::new(ExtensionManager::new());

        // 11.1 创建业务扩展注册中心
        #[cfg(feature = "extensions")]
        let business_extension_registry =
            Arc::new(crate::shared::extension::BusinessExtensionRegistry::new());

        // 12. 请求管理器已在前面创建（用于同步和任务系统）

        // 12.1 创建任务调度器
        // 创建同步上下文（用于任务执行器）
        // 注意：这里先创建一个临时的上下文，登录后会更新 user_id
        use crate::infrastructure::task::config::SyncConfig;
        use crate::infrastructure::task::executor::SyncContext;
        let sync_context = SyncContext {
            connection: Arc::clone(&connection),
            storage: Arc::clone(&storage),
            event_bus: Arc::clone(&event_bus),
            request_manager: Arc::clone(&request_manager),
            config: SyncConfig::default(),
            user_id: config.user_id.clone(),
        };

        let scheduler_config = TaskSchedulerConfig::default();
        let task_scheduler = Arc::new(TaskScheduler::new(
            Arc::clone(&task_manager),
            sync_context,
            scheduler_config,
        ));

        // 13. 创建消息重试任务（生产级特性）
        use crate::infrastructure::task::message_retry::MessageRetryTask;
        let message_retry_task = Arc::new(MessageRetryTask::new(
            Arc::clone(&pending_message_queue),
            Arc::clone(&message_command_handler),
            Arc::clone(&connection),
            Arc::clone(&event_bus),
        ));
        let retry_handle = message_retry_task.start();

        // 注册重试任务到任务管理器
        task_manager
            .register(
                "message-retry".to_string(),
                retry_handle,
                crate::infrastructure::task::TaskType::Other("MessageRetry".to_string()),
                "MessageRetry".to_string(),
                true, // 可取消
            )
            .await;

        // 14. 注册内置任务执行器
        use crate::infrastructure::task::builtin::{FullSyncTask, SessionSyncTask};
        let session_sync_task = Arc::new(SessionSyncTask::new(
            Arc::clone(&sync_command_handler),
            None,
        ));
        task_scheduler.register_task(session_sync_task).await;
        let full_sync_task = Arc::new(FullSyncTask::new(Arc::clone(&sync_command_handler)));
        task_scheduler.register_task(full_sync_task).await;

        // 注意：MessageSyncTask 需要 session_id，所以不能在这里注册，需要在需要时动态创建

        // 14. 注册连接事件监听器（用于自动重连和状态同步）
        let task_scheduler_for_listener = Arc::clone(&task_scheduler);
        let mut event_rx = event_bus.subscribe();
        tokio_spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                match event {
                    Event::Connection(ConnectionEvent::Disconnected) => {
                        // 连接断开，禁用任务调度器
                        task_scheduler_for_listener.disable().await;
                    }
                    Event::Connection(ConnectionEvent::Connected { .. }) => {
                        // 连接建立，启用任务调度器
                        if let Err(e) = task_scheduler_for_listener.enable().await {
                            warn!(error = %e, "Failed to enable task scheduler");
                        }
                    }
                    _ => {}
                }
            }
        });

        Ok(Self {
            connection,
            message_command_handler,
            message_query_handler,
            session_command_handler,
            session_query_handler,
            sync_command_handler,
            sync_query_handler,
            storage,
            event_bus,
            config: config_arc,
            user_id,
            message_frame_handler,
            observer_registry,
            #[cfg(feature = "extensions")]
            extension_manager,
            #[cfg(feature = "extensions")]
            business_extension_registry,
            task_manager,
            task_scheduler,
            metrics,
            #[cfg(debug_assertions)]
            leak_detector,
        })
    }
}

// Trait 实现已在各模块文件中完成（connection.rs, session.rs, message.rs 等）
// 这里只提供向后兼容的公共方法，委托给 trait 实现

#[cfg(feature = "extensions")]
use crate::api::traits::ExtensionApi;
use crate::api::traits::{
    ConnectionApi, EventApi, MessageApi, SessionApi, SyncApi, TaskApi, UtilityApi,
};

// 为了保持向后兼容，提供直接方法（委托给 trait）
impl FlareIMClient {
    /// 登录到服务器（委托给 ConnectionApi）
    pub async fn login(&self, user_id: &str, token: &str) -> Result<LoginResult> {
        <Self as ConnectionApi>::login(self, user_id, token).await
    }

    /// 登出（委托给 ConnectionApi）
    pub async fn logout(&self) -> Result<()> {
        <Self as ConnectionApi>::logout(self).await
    }

    /// 获取连接状态（委托给 ConnectionApi）
    pub async fn connection_state(&self) -> crate::infrastructure::connection::ConnectionState {
        <Self as ConnectionApi>::connection_state(self).await
    }

    /// 设置 AES-256 加密（委托给 ConnectionApi）
    pub async fn set_crypto_aes256(&self, key: &[u8]) -> Result<()> {
        <Self as ConnectionApi>::set_crypto_aes256(self, key).await
    }

    /// 设置自定义加密服务（委托给 ConnectionApi）
    pub async fn set_crypto(
        &self,
        crypto: Arc<dyn crate::application::CryptoService>,
    ) -> Result<()> {
        <Self as ConnectionApi>::set_crypto(self, crypto).await
    }

    /// 获取会话列表（委托给 SessionApi）
    pub async fn get_sessions(
        &self,
        filter: crate::infrastructure::storage::SessionFilter,
    ) -> Result<Vec<crate::application::vo::SessionVO>> {
        <Self as SessionApi>::get_sessions(self, filter).await
    }

    /// 获取会话列表（带扩展信息）（委托给 SessionApi）
    #[cfg(feature = "extensions")]
    pub async fn get_sessions_extended(
        &self,
        filter: crate::infrastructure::storage::SessionFilter,
    ) -> Result<Vec<crate::domain::session::ExtendedSessionSummary>> {
        <Self as SessionApi>::get_sessions_extended(self, filter).await
    }

    /// 获取会话详情（带扩展信息）（委托给 SessionApi）
    #[cfg(feature = "extensions")]
    pub async fn get_session_extended(
        &self,
        session_id: &str,
    ) -> Result<crate::domain::session::ExtendedSessionSummary> {
        <Self as SessionApi>::get_session_extended(self, session_id).await
    }

    /// 创建会话（委托给 SessionApi）
    pub async fn create_session(
        &self,
        session_id: Option<String>,
        session_type: String,
        business_type: String,
        display_name: Option<String>,
        participants: Option<Vec<String>>,
    ) -> Result<String> {
        <Self as SessionApi>::create_session(
            self,
            session_id,
            session_type,
            business_type,
            display_name,
            participants,
        )
        .await
    }

    /// 标记会话已读（委托给 SessionApi）
    pub async fn mark_session_read(
        &self,
        session_id: &str,
        message_seq: Option<i64>,
    ) -> Result<()> {
        <Self as SessionApi>::mark_read(self, session_id, message_seq).await
    }

    /// 标记消息已读（委托给 SessionApi，已废弃，使用 mark_read）
    #[deprecated(note = "Use mark_read instead")]
    pub async fn mark_as_read(&self, session_id: &str, message_seq: Option<i64>) -> Result<()> {
        <Self as SessionApi>::mark_read(self, session_id, message_seq).await
    }

    /// 发送消息（委托给 MessageApi）
    ///
    /// # 参数
    /// - `message`: 完整的 Message 对象（由 MessageBuilder 构建）
    /// - `receiver_id`: 接收者 ID（可选）
    /// - `channel_id`: 频道 ID（可选）
    ///
    /// # 返回
    /// - `Result<String>`: 消息 ID
    pub async fn send_message(
        &self,
        message: crate::domain::message::Message,
        receiver_id: Option<String>,
        channel_id: Option<String>,
    ) -> Result<String> {
        <Self as MessageApi>::send_message(self, message, receiver_id, channel_id).await
    }

    /// 回复消息（使用 MessageBuilder 构建消息并发送）
    ///
    /// # 参数
    /// - `message`: 完整的 Message 对象（由 MessageBuilder 构建）
    /// - `reply_to_message_id`: 被回复的消息ID
    ///
    /// # 返回
    /// - `Result<String>`: 消息 ID
    pub async fn reply_message(
        &self,
        message: crate::domain::message::Message,
        reply_to_message_id: &str,
    ) -> Result<String> {
        // 使用 send_message 发送，reply_to 字段已在 MessageBuilder 中设置
        self.send_message(message, None, None).await
    }

    /// 添加线程回复（使用 MessageBuilder 构建消息并发送）
    ///
    /// # 参数
    /// - `message`: 完整的 Message 对象（由 MessageBuilder 构建）
    /// - `thread_id`: 线程ID
    ///
    /// # 返回
    /// - `Result<String>`: 消息 ID
    pub async fn add_thread_reply(
        &self,
        message: crate::domain::message::Message,
        _thread_id: &str,
    ) -> Result<String> {
        // TODO: 实现线程回复逻辑
        self.send_message(message, None, None).await
    }

    /// 撤回消息（委托给 MessageApi）
    pub async fn recall_message(&self, message_id: &str) -> Result<()> {
        <Self as MessageApi>::recall_message(self, message_id).await
    }

    /// 删除消息（委托给 MessageApi）
    pub async fn delete_message(
        &self,
        message_id: &str,
        delete_type: i32,
        notify_others: bool,
    ) -> Result<()> {
        <Self as MessageApi>::delete_message(self, message_id, delete_type, notify_others).await
    }

    /// 重试发送消息（TODO: 实现重试逻辑）
    pub async fn retry_message(&self, _message_id: &str) -> Result<()> {
        // TODO: 实现消息重试逻辑
        anyhow::bail!("Retry message not implemented yet")
    }

    /// 取消消息重试（TODO: 实现取消重试逻辑）
    pub async fn cancel_message_retry(&self, _message_id: &str) -> Result<()> {
        // TODO: 实现取消重试逻辑
        anyhow::bail!("Cancel message retry not implemented yet")
    }

    /// 获取消息重试状态（TODO: 实现重试状态查询）
    pub async fn get_message_retry_state(&self, _message_id: &str) -> Option<()> {
        // TODO: 实现重试状态查询
        None
    }

    /// 获取正在重试的消息列表（TODO: 实现重试列表查询）
    pub async fn get_retrying_messages(&self) -> Vec<String> {
        // TODO: 实现重试列表查询
        vec![]
    }

    /// 添加表情反应（委托给 MessageApi）
    pub async fn add_reaction(&self, message_id: &str, emoji: &str) -> Result<()> {
        <Self as MessageApi>::add_reaction(self, message_id, emoji).await
    }

    /// 移除表情反应（委托给 MessageApi）
    pub async fn remove_reaction(&self, message_id: &str, emoji: &str) -> Result<()> {
        <Self as MessageApi>::remove_reaction(self, message_id, emoji).await
    }

    /// 编辑消息（委托给 MessageApi）
    pub async fn edit_message(&self, message_id: &str, new_content: &str) -> Result<()> {
        <Self as MessageApi>::edit_message(self, message_id, new_content).await
    }

    /// 获取会话消息（委托给 MessageApi）
    pub async fn get_messages(
        &self,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<crate::application::vo::MessageVO>> {
        <Self as MessageApi>::get_messages(self, session_id, limit, cursor).await
    }

    /// 获取消息列表（带扩展信息）（委托给 MessageApi）
    #[cfg(feature = "extensions")]
    pub async fn get_messages_extended(
        &self,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<crate::domain::message::ExtendedMessage>> {
        <Self as MessageApi>::get_messages_extended(self, session_id, limit, cursor).await
    }

    /// 获取事件总线（委托给 EventApi）
    pub fn event_bus(&self) -> Arc<EventBus> {
        <Self as EventApi>::event_bus(self)
    }

    /// 注册消息观察者（委托给 EventApi）
    pub async fn register_message_observer(
        &self,
        observer: crate::shared::observer::ArcMessageObserver,
    ) {
        <Self as EventApi>::register_message_observer(self, observer).await
    }

    /// 同步消息（委托给 SyncApi）
    pub async fn sync_messages(
        &self,
        session_id: &str,
        after_seq: Option<i64>,
    ) -> Result<crate::domain::sync::SyncResult> {
        <Self as SyncApi>::sync_messages(self, session_id, after_seq).await
    }

    /// 同步会话（委托给 SyncApi）
    pub async fn sync_sessions(
        &self,
        cursor: Option<String>,
    ) -> Result<crate::application::vo::session::SessionSyncResultVO> {
        <Self as SyncApi>::sync_sessions(self, cursor).await
    }

    /// 注册扩展提供者（委托给 ExtensionApi）
    #[cfg(feature = "extensions")]
    pub async fn register_extension_provider(
        &self,
        provider: Arc<dyn crate::domain::ExtensionProvider>,
    ) -> Result<()> {
        <Self as ExtensionApi>::register_extension_provider(self, provider).await
    }

    /// 注册用户业务扩展点（委托给 ExtensionApi）
    #[cfg(feature = "extensions")]
    pub async fn register_user_business_extension(
        &self,
        extension: Arc<dyn crate::shared::extension::business::UserBusinessExtension>,
    ) -> Result<()> {
        <Self as ExtensionApi>::register_user_business_extension(self, extension).await
    }

    /// 注册群组业务扩展点（委托给 ExtensionApi）
    #[cfg(feature = "extensions")]
    pub async fn register_group_business_extension(
        &self,
        extension: Arc<dyn crate::shared::extension::business::GroupBusinessExtension>,
    ) -> Result<()> {
        <Self as ExtensionApi>::register_group_business_extension(self, extension).await
    }

    /// 注册频道业务扩展点（委托给 ExtensionApi）
    #[cfg(feature = "extensions")]
    pub async fn register_channel_business_extension(
        &self,
        extension: Arc<dyn crate::shared::extension::business::ChannelBusinessExtension>,
    ) -> Result<()> {
        <Self as ExtensionApi>::register_channel_business_extension(self, extension).await
    }

    /// 设置扩展缓存（委托给 ExtensionApi）
    #[cfg(feature = "extensions")]
    pub async fn set_extension_cache(
        &self,
        cache: Arc<dyn crate::domain::ExtensionCache>,
    ) -> Result<()> {
        <Self as ExtensionApi>::set_extension_cache(self, cache).await
    }

    /// 获取当前用户 ID（委托给 UtilityApi）
    pub async fn user_id(&self) -> Result<String> {
        <Self as UtilityApi>::user_id(self).await
    }

    /// 获取性能指标快照（委托给 UtilityApi）
    pub fn metrics_snapshot(&self) -> crate::shared::metrics::MetricsSnapshot {
        <Self as UtilityApi>::metrics_snapshot(self)
    }

    /// 重置性能指标（委托给 UtilityApi）
    pub fn reset_metrics(&self) {
        <Self as UtilityApi>::reset_metrics(self)
    }

    /// 获取任务管理器（委托给 UtilityApi）
    pub fn task_manager(&self) -> Arc<crate::infrastructure::task::TaskManager> {
        <Self as UtilityApi>::task_manager(self)
    }

    /// 获取任务调度器（用于注册和调度自定义任务）
    pub fn task_scheduler(&self) -> Arc<crate::infrastructure::task::TaskScheduler> {
        <Self as UtilityApi>::task_scheduler(self)
    }

    /// 注册自定义任务执行器（委托给 TaskApi）
    pub async fn register_task(
        &self,
        executor: Arc<dyn crate::infrastructure::task::executor::SyncTaskExecutor>,
    ) {
        <Self as TaskApi>::register_task(self, executor).await
    }

    /// 取消注册任务执行器（委托给 TaskApi）
    pub async fn unregister_task(&self, name: &str) -> bool {
        <Self as TaskApi>::unregister_task(self, name).await
    }

    /// 获取所有已注册的任务名称（委托给 TaskApi）
    pub async fn get_registered_tasks(&self) -> Vec<String> {
        <Self as TaskApi>::get_registered_tasks(self).await
    }

    /// 调度任务（通过任务名称）（委托给 TaskApi）
    pub async fn schedule_task_by_name(
        &self,
        task_name: &str,
        task_id: Option<String>,
    ) -> Result<String> {
        <Self as TaskApi>::schedule_task_by_name(self, task_name, task_id).await
    }

    /// 转发消息（委托给 MessageApi）
    pub async fn forward_message(
        &self,
        message_ids: Vec<String>,
        target_session_id: &str,
        merge_forward: bool,
        _reason: Option<String>,
    ) -> Result<Vec<String>> {
        <Self as MessageApi>::forward_messages(self, message_ids, target_session_id, merge_forward)
            .await
    }

    /// 引用消息（委托给 MessageApi）
    pub async fn quote_message(
        &self,
        session_id: &str,
        quoted_message_id: &str,
        text: &str,
        preview_text: Option<String>,
    ) -> Result<String> {
        let message = <Self as MessageApi>::create_quote_message(
            self,
            session_id,
            quoted_message_id,
            text,
            preview_text,
        )?;
        self.send_message(message, None, None).await
    }

    /// 置顶消息
    pub async fn pin_message(
        &self,
        message_id: &str,
        _reason: Option<String>,
        expire_at: Option<prost_types::Timestamp>,
    ) -> Result<()> {
        let user_id = self.user_id.read().await.clone();
        self.message_command_handler
            .handle_pin_message(crate::application::commands::message::PinMessageCommand {
                message_id: crate::domain::MessageId::new(message_id.to_string()),
                user_id: crate::domain::UserId::new(user_id),
                expire_at,
            })
            .await
    }

    /// 取消置顶
    pub async fn unpin_message(&self, message_id: &str) -> Result<()> {
        let user_id = self.user_id.read().await.clone();
        self.message_command_handler
            .handle_unpin_message(crate::application::commands::message::UnpinMessageCommand {
                message_id: crate::domain::MessageId::new(message_id.to_string()),
                user_id: crate::domain::UserId::new(user_id),
            })
            .await
    }

    /// 收藏消息
    pub async fn favorite_message(
        &self,
        message_id: &str,
        tags: Option<Vec<String>>,
        note: Option<String>,
    ) -> Result<()> {
        let user_id = self.user_id.read().await.clone();
        self.message_command_handler
            .handle_favorite_message(
                crate::application::commands::message::FavoriteMessageCommand {
                    message_id: crate::domain::MessageId::new(message_id.to_string()),
                    user_id: crate::domain::UserId::new(user_id),
                    tags,
                    note,
                },
            )
            .await
    }

    /// 取消收藏
    pub async fn unfavorite_message(&self, message_id: &str) -> Result<()> {
        let user_id = self.user_id.read().await.clone();
        self.message_command_handler
            .handle_unfavorite_message(
                crate::application::commands::message::UnfavoriteMessageCommand {
                    message_id: crate::domain::MessageId::new(message_id.to_string()),
                    user_id: crate::domain::UserId::new(user_id),
                },
            )
            .await
    }

    /// 标记消息（TODO: 实现标记逻辑）
    pub async fn mark_message(
        &self,
        _message_id: &str,
        _mark_type: i32,
        _color: Option<String>,
    ) -> Result<()> {
        // TODO: 实现消息标记逻辑
        anyhow::bail!("Mark message not implemented yet")
    }

    /// 批量标记已读（TODO: 实现批量标记已读逻辑）
    pub async fn batch_mark_message_read(
        &self,
        _session_id: &str,
        _message_ids: Option<Vec<String>>,
        _burn_after_read: Option<bool>,
    ) -> Result<i32> {
        // TODO: 实现批量标记已读逻辑
        anyhow::bail!("Batch mark message read not implemented yet")
    }

    /// 获取任务状态（委托给 TaskApi）
    pub async fn get_task_status(
        &self,
        task_id: &str,
    ) -> Option<crate::infrastructure::task::standard::TaskStatus> {
        <Self as TaskApi>::get_task_status(self, task_id).await
    }

    /// 取消任务（委托给 TaskApi）
    pub async fn cancel_task(&self, task_id: &str) -> bool {
        <Self as TaskApi>::cancel_task(self, task_id).await
    }

    /// 获取任务调度器统计信息（委托给 TaskApi）
    pub async fn get_task_scheduler_stats(
        &self,
    ) -> crate::infrastructure::task::TaskSchedulerStats {
        <Self as TaskApi>::get_task_scheduler_stats(self).await
    }

    /// 获取任务调度器性能快照（委托给 TaskApi）
    pub async fn get_task_scheduler_performance(
        &self,
    ) -> crate::infrastructure::task::TaskSchedulerPerformanceSnapshot {
        <Self as TaskApi>::get_task_scheduler_performance(self).await
    }

    /// 获取存储后端（委托给 UtilityApi）
    pub fn storage(&self) -> Arc<dyn crate::infrastructure::storage::StorageBackend> {
        <Self as UtilityApi>::storage(self)
    }

    /// 获取消息服务（委托给 UtilityApi）
    /// 获取消息命令处理器
    pub fn message_command_handler(&self) -> Arc<MessageCommandHandler> {
        Arc::clone(&self.message_command_handler)
    }

    /// 获取消息查询处理器
    pub fn message_query_handler(&self) -> Arc<MessageQueryHandler> {
        Arc::clone(&self.message_query_handler)
    }

    /// 创建消息构建器
    ///
    /// # 返回
    /// - `MessageBuilder`: 消息构建器实例
    ///
    /// # 示例
    /// ```rust,no_run
    /// let message = client.message_builder()
    ///     .new("session_123", "user_456")
    ///     .receiver_id("user_789")
    ///     .text("Hello, World!")
    ///     .build();
    ///
    /// let message_id = client.send_message(message).await?;
    /// ```
    pub fn message_builder(&self) -> crate::domain::MessageBuilder {
        // 获取当前用户 ID
        let user_id = {
            // 注意：这里使用 blocking_read，因为 message_builder() 是同步方法
            // 如果需要在异步上下文中使用，应该使用异步版本
            let guard = self.user_id.blocking_read();
            guard.clone()
        };

        // 创建消息构建器（session_id 需要用户设置）
        // 返回一个基础的 MessageBuilder，用户可以链式调用设置属性
        crate::domain::MessageBuilder::new()
            .session_id(String::new())
            .sender_id(user_id)
    }

    /// 获取内存泄漏检测器（委托给 UtilityApi）
    #[cfg(debug_assertions)]
    pub fn leak_detector(&self) -> Arc<crate::shared::memory_leak_detector::MemoryLeakDetector> {
        <Self as UtilityApi>::leak_detector(self)
    }
}
