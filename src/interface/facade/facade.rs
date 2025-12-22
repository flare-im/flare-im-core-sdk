//! SDK Facade
//!
//! 对外统一接口，隐藏内部实现细节

use std::sync::Arc;
use crate::application::fsm::FsmManager;
use crate::application::handlers::{CommandHandler, QueryHandler};
use crate::application::sync_coordinator::SyncCoordinator;
use crate::application::extension::{ExtensionRegistry, SdkContext, SdkExtension};
use crate::domain::repository::{EventStore, ReadStore};
use crate::domain::session::Session;
use crate::domain::connection::Connection;
use crate::domain::sync::Sync;
use crate::domain::message_queue::{MessageQueue, MessageQueueProcessor, MessageHandler};
use crate::infrastructure::event_bus::EventBus;
use super::event_subscription_facade::EventSubscriptionFacade;
use super::default_message_handler::DefaultMessageHandler;
use crate::config::SdkConfig;

use super::message_facade::MessageFacade;
use super::conversation_facade::ConversationFacade;

/// IM Core SDK 主入口
pub struct ImCoreSdk {
    command_handler: Arc<CommandHandler>,
    query_handler: Arc<QueryHandler>,
    sync_coordinator: Arc<SyncCoordinator>,
    message_facade: MessageFacade,
    conversation_facade: ConversationFacade,
    message_queue: Arc<MessageQueue>,
    queue_processor: Option<Arc<MessageQueueProcessor>>,
    extension_registry: Arc<ExtensionRegistry>,
    sdk_context: Arc<SdkContext>,
}

impl ImCoreSdk {
    /// 创建 SDK 实例
    pub async fn new(config: SdkConfig) -> anyhow::Result<Self> {
        // 创建存储层
        #[cfg(not(target_arch = "wasm32"))]
        use crate::infrastructure::storage::read_store::SqliteReadStore;
        #[cfg(not(target_arch = "wasm32"))]
        use crate::infrastructure::storage::event_store::SqliteEventStore;
        use crate::infrastructure::storage::read_store::MemoryReadStore;
        use crate::infrastructure::storage::event_store::MemoryEventStore;
        use crate::infrastructure::storage::media_cache::MediaCacheManager;
        
        // 根据配置选择存储类型
        // 如果配置了 storage.path，使用 SQLite；否则使用内存存储（用于测试）
        let (event_store, read_store) = {
            #[cfg(not(target_arch = "wasm32"))]
            {
                if let Some(ref storage_path) = config.storage.path {
                    // 使用 SQLite 存储
                    // 确保存储目录存在
                    std::fs::create_dir_all(storage_path)
                        .map_err(|e| anyhow::anyhow!("创建存储目录失败: {}", e))?;
                    
                    let db_path = storage_path.join(&config.storage.db_filename);
                    // SQLite 连接字符串：sqlx 使用 sqlite:/// 表示绝对路径（三个斜杠）
                    // 将路径转换为绝对路径，确保可以正确打开
                    let db_path_abs = match db_path.canonicalize() {
                        Ok(path) => path,
                        Err(_) => {
                            // 如果路径不存在，先创建父目录
                            if let Some(parent) = db_path.parent() {
                                std::fs::create_dir_all(parent)
                                    .map_err(|e| anyhow::anyhow!("创建数据库目录失败: {}", e))?;
                            }
                            // 返回原始路径（如果 canonicalize 失败，可能是文件还不存在）
                            db_path
                        }
                    };
                    
                    // 使用绝对路径，sqlx 需要三个斜杠
                    // 注意：路径中的特殊字符需要处理，使用 display() 可能包含空格等
                    // sqlx 的 SQLite 连接字符串格式：sqlite:///绝对路径
                    let db_url = format!("sqlite:///{}", db_path_abs.to_string_lossy());
                    
                    tracing::info!("使用 SQLite 存储: {}", db_url);
                    tracing::debug!("数据库文件路径: {}", db_path_abs.display());
                    
                    let event_store = Arc::new(
                        SqliteEventStore::new(&db_url).await
                            .map_err(|e| anyhow::anyhow!("创建 SQLite EventStore 失败: {} (路径: {})", e, db_path_abs.display()))?
                    ) as Arc<dyn EventStore>;
                    
                    let read_store = Arc::new(
                        SqliteReadStore::new(&db_url).await
                            .map_err(|e| anyhow::anyhow!("创建 SQLite ReadStore 失败: {} (路径: {})", e, db_path_abs.display()))?
                    ) as Arc<dyn ReadStore>;
                    
                    (event_store, read_store)
                } else {
                    // 使用内存存储（用于测试或 Web 平台）
                    tracing::info!("使用内存存储（测试模式）");
                    
                    let event_store = Arc::new(
                        MemoryEventStore::new()
                    ) as Arc<dyn EventStore>;
                    
                    let read_store = Arc::new(
                        MemoryReadStore::new()
                    ) as Arc<dyn ReadStore>;
                    
                    (event_store, read_store)
                }
            }
            
            #[cfg(target_arch = "wasm32")]
            {
                // Web 平台只能使用内存存储
                tracing::info!("使用内存存储（Web 平台）");
                
                let event_store = Arc::new(
                    MemoryEventStore::new()
                ) as Arc<dyn EventStore>;
                
                let read_store = Arc::new(
                    MemoryReadStore::new()
                ) as Arc<dyn ReadStore>;
                
                (event_store, read_store)
            }
        };
        
        // 验证配置
        config.validate()
            .map_err(|e| anyhow::anyhow!("配置验证失败: {}", e))?;
        
        // 创建媒体缓存管理器
        let cache_root = config.storage.path
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("./flare_im_cache"))
            .join("media_cache");
        let media_cache = Arc::new(
            MediaCacheManager::new(
                &cache_root,
                config.media.max_cache_size_mb * 1024 * 1024 // 转换为字节
            )?
        );
        
        // 创建聚合根
        let device_id = uuid::Uuid::new_v4().to_string();
        let session = Session::new(device_id);
        let connection = Connection::new();
        let sync = Sync::new();
        
        // 创建 FSM
        let fsm = Arc::new(FsmManager::new(
            session,
            connection,
            sync,
            event_store.clone(),
        ));
        
        // 转换为 flare-core 配置
        let flare_core_config = config.to_flare_core_config()?;
        
        // 创建事件总线（容量 1000）
        let event_bus = Arc::new(EventBus::new(1000));
        
        // 创建 Extension Registry
        let extension_registry = Arc::new(ExtensionRegistry::new());
        
        // 创建消息队列（必须在创建 CommandHandler 之前创建，以便传递给它）
        let message_queue = Arc::new(MessageQueue::new());
        
        // 创建 Command Handler（传递 MessageQueue）
        let command_handler = Arc::new(CommandHandler::new(
            fsm.clone(),
            event_store.clone(),
            read_store.clone(), // 传递 ReadStore
            config.clone(), // 传递完整配置
            media_cache.clone(),
            event_bus.clone(),
            extension_registry.clone(),
            Some(message_queue.clone()), // 传递 MessageQueue
        )?);
        
        // 创建 Query Handler（需要 FSM 以支持 Session 查询）
        let query_handler = Arc::new(QueryHandler::new(read_store.clone(), fsm.clone()));
        
        // 创建 Sync Coordinator（带 Extension Registry）
        let sync_coordinator = Arc::new(
            SyncCoordinator::new(
                fsm.clone(),
                event_store.clone(),
            )
            .with_extension_registry(extension_registry.clone())
        );
        
        // 创建 SdkContext（提供给扩展使用）
        let sdk_context = Arc::new(SdkContext::new(
            command_handler.clone(),
            query_handler.clone(),
            event_bus.clone(),
            event_store.clone(),
            read_store.clone(),
            sync_coordinator.clone(),
            fsm.clone(),
        ));
        
        // 创建 Facade
        let message_facade = MessageFacade::new(
            command_handler.clone(),
            query_handler.clone(),
            read_store.clone(), // 传递 ReadStore
            media_cache,
        );
        
        let conversation_facade = ConversationFacade::new(
            command_handler.clone(),
            query_handler.clone(),
        );
        
        // 创建消息队列处理器（使用 DefaultMessageHandler）
        let queue_handler = Arc::new(DefaultMessageHandler::new(
            command_handler.clone(),
            message_queue.clone(),
            read_store.clone(),
            event_store.clone(),
            event_bus.clone(),
            fsm.clone(),
        ));
        let queue_processor = Arc::new(MessageQueueProcessor::new(
            message_queue.clone(),
            queue_handler,
        ));
        
        // 启动消息队列处理循环（在后台任务中）
        let processor_clone = queue_processor.clone();
        tokio::spawn(async move {
            processor_clone.start().await;
        });
        
        Ok(Self {
            command_handler,
            query_handler,
            sync_coordinator,
            message_facade,
            conversation_facade,
            message_queue,
            queue_processor: Some(queue_processor),
            extension_registry,
            sdk_context,
        })
    }
    
    // ============================================================================
    // Facade 访问器
    // ============================================================================
    
    /// 获取消息 Facade
    pub fn message(&self) -> &MessageFacade {
        &self.message_facade
    }
    
    /// 获取会话 Facade
    pub fn conversation(&self) -> &ConversationFacade {
        &self.conversation_facade
    }
    
    // ============================================================================
    // 核心 API：登录、登出、连接、同步
    // ============================================================================
    
    /// 登录
    ///
    /// SDK 核心方法，用于用户身份认证和建立连接
    pub async fn login(&self, user_id: String, token: String) -> anyhow::Result<()> {
        use crate::application::commands::LoginCommand;
        self.command_handler.login_direct(user_id, token).await
    }
    
    /// 登出
    ///
    /// SDK 核心方法，用于断开连接和清理会话
    pub async fn logout(&self) -> anyhow::Result<()> {
        self.command_handler.logout_direct().await
    }
    
    /// 连接
    ///
    /// SDK 核心方法，用于建立网络连接
    pub async fn connect(&self) -> anyhow::Result<()> {
        self.command_handler.connect_direct().await
    }
    
    /// 执行 Bootstrap Sync
    ///
    /// SDK 核心方法，用于初始化同步（全量同步）
    ///
    /// 同步流程：
    /// 1. 核心 Bootstrap Sync（会话列表、未读消息等）
    /// 2. Extension Bootstrap Sync（扩展的 Bootstrap 同步）
    pub async fn bootstrap_sync(&self) -> anyhow::Result<()> {
        self.sync_coordinator.execute_bootstrap_sync().await
    }
    
    /// 执行 Async Sync
    ///
    /// SDK 核心方法，用于增量同步
    ///
    /// # 参数
    /// * `sync_type` - 同步类型（如 "friend_status", "group_info"）
    pub async fn async_sync(&self, sync_type: String) -> anyhow::Result<()> {
        self.sync_coordinator.execute_async_sync(sync_type).await
    }
    
    /// 执行所有扩展的 Async Sync
    ///
    /// 在后台执行所有扩展的异步同步
    pub async fn sync_all_extensions(&self) -> anyhow::Result<()> {
        self.sync_coordinator.execute_all_extension_async_sync().await
    }
    
    /// 获取消息队列（用于接收消息）
    pub fn message_queue(&self) -> &Arc<MessageQueue> {
        &self.message_queue
    }
    
    /// 获取消息发送指标
    ///
    /// 返回消息发送的统计信息（总数、成功数、失败数、平均延迟等）
    pub async fn get_message_metrics(&self) -> crate::infrastructure::metrics::MessageMetricsSnapshot {
        crate::infrastructure::metrics::get_metrics_snapshot().await
    }
    
    // ============================================================================
    // Extension 管理 API
    // ============================================================================
    
    /// 注册扩展
    ///
    /// # 参数
    /// * `extension` - 要注册的扩展
    ///
    /// # 返回
    /// * `Ok(())` - 注册成功
    /// * `Err` - 注册失败
    pub async fn register_extension(
        &self,
        extension: Arc<dyn SdkExtension>,
    ) -> anyhow::Result<()> {
        // 注册到 Extension Registry
        self.extension_registry.register_extension(extension.clone()).await?;
        
        // 创建可变的 SdkContext（用于注册）
        // 注意：这里需要创建一个临时的可变上下文
        let mut ctx = SdkContext::new(
            self.command_handler.clone(),
            self.query_handler.clone(),
            self.sdk_context.event_bus.clone(),
            self.sdk_context.event_store.clone(),
            self.sdk_context.read_store.clone(),
            self.sync_coordinator.clone(),
            self.sdk_context.fsm.clone(),
        );
        
        // 调用扩展的 register 方法
        extension.register(&mut ctx)?;
        
        // 调用扩展的初始化回调
        extension.on_initialized(&self.sdk_context)?;
        
        tracing::info!("Extension '{}' registered and initialized", extension.name());
        
        Ok(())
    }
    
    /// 获取扩展
    pub async fn get_extension(&self, name: &str) -> Option<Arc<dyn SdkExtension>> {
        self.extension_registry.get_extension(name).await
    }
    
    /// 列出所有已注册的扩展
    pub async fn list_extensions(&self) -> Vec<String> {
        self.extension_registry.list_extensions().await
    }
    
    /// 取消注册扩展
    pub async fn unregister_extension(&self, name: &str) -> anyhow::Result<()> {
        // 获取扩展
        if let Some(extension) = self.extension_registry.get_extension(name).await {
            // 调用销毁回调
            extension.on_destroyed()?;
        }
        
        // 从注册表中移除
        self.extension_registry.unregister_extension(name).await
    }
    
    /// 获取 SDK Context（供扩展使用）
    pub fn sdk_context(&self) -> &Arc<SdkContext> {
        &self.sdk_context
    }
    
    /// 获取 Extension Registry
    pub fn extension_registry(&self) -> &Arc<ExtensionRegistry> {
        &self.extension_registry
    }
    
    // ============================================================================
    // 事件订阅 API（推荐使用）
    // ============================================================================
    
    /// 获取事件订阅 Facade
    ///
    /// 提供便捷的事件订阅 API，封装 EventBus 的复杂操作
    ///
    /// # 示例
    ///
    /// ```rust
    /// use flare_im_core_sdk::interface::event::subscribers::*;
    ///
    /// let event_facade = sdk.events();
    /// let subscriber_id = event_facade.subscribe_message(Arc::new(MyMessageSubscriber)).await;
    /// ```
    pub fn events(&self) -> EventSubscriptionFacade {
        EventSubscriptionFacade::new(self.sdk_context.event_bus.clone())
    }
    
    /// 获取事件总线（直接访问，高级用法）
    ///
    /// 用于直接访问事件总线的底层功能，一般用户应该使用 `events()` 方法
    ///
    /// # 示例
    ///
    /// ```rust
    /// let event_bus = sdk.event_bus();
    /// let stats = event_bus.get_statistics().await;
    /// ```
    pub fn event_bus(&self) -> &Arc<EventBus> {
        &self.sdk_context.event_bus
    }
}

