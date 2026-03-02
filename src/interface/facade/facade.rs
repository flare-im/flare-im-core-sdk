//! Main SDK Entry Point
//!
//! [`ImCoreSdk`] is the primary entry point for the Flare IM Core SDK.
//! It provides access to all SDK functionality through facades and manages
//! the SDK lifecycle (login, connect, sync, etc.).
//!
//! ## Architecture
//!
//! The SDK follows a layered architecture:
//!
//! - **Interface Layer** (this module): Public API facades
//! - **Application Layer**: Command/Query handlers, FSM, sync coordinator
//! - **Domain Layer**: Business logic, aggregates, domain services
//! - **Infrastructure Layer**: Storage, network, event bus
//!
//! ## Example
//!
//! ```no_run
//! use flare_im_core_sdk::interface::facade::ImCoreSdk;
//! use flare_im_core_sdk::config::SdkConfig;
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Initialize SDK
//! let config = SdkConfig::default();
//! let sdk = ImCoreSdk::new(config).await?;
//!
//! // Login and connect
//! sdk.login("user_id".to_string(), "token".to_string()).await?;
//! sdk.connect().await?;
//!
//! // Bootstrap sync
//! sdk.bootstrap_sync().await?;
//!
//! // Use facades
//! let message_facade = sdk.message();
//! let conversation_facade = sdk.conversation();
//! # Ok(())
//! # }
//! ```

use std::sync::Arc;
use tokio::sync::Mutex;
use crate::application::fsm::FsmManager;
use crate::application::handlers::{CommandHandler, QueryHandler, ConversationSyncHandler, SyncHandler};
use crate::application::sync_coordinator::SyncCoordinator;
use crate::application::extension::{ExtensionRegistry, SdkContext, SdkExtension};
use crate::domain::repository::{EventStore, MessageRepository, ConversationRepository};
use crate::domain::session::Session;
use crate::domain::connection::Connection;
use crate::domain::sync::Sync;
use crate::domain::message_queue::{MessageQueue, MessageQueueProcessor};
use crate::infrastructure::event_bus::EventBus;
use crate::infrastructure::messaging::MessageSender;
use super::event_subscription_facade::EventSubscriptionFacade;
use super::default_message_handler::DefaultMessageHandler;
use crate::config::SdkConfig;

use super::message_facade::MessageFacade;
use super::conversation_facade::ConversationFacade;

/// Main SDK entry point
///
/// Provides access to all SDK functionality through facades and manages
/// the SDK lifecycle.
pub struct ImCoreSdk {
    #[allow(dead_code)]
    command_handler: Arc<CommandHandler>,
    query_handler: Arc<QueryHandler>,
    sync_coordinator: Arc<SyncCoordinator>,
    message_facade: MessageFacade,
    conversation_facade: ConversationFacade,
    #[allow(dead_code)]
    message_queue: Arc<MessageQueue>,
    #[allow(dead_code)]
    queue_processor: Option<Arc<MessageQueueProcessor>>,
    #[allow(dead_code)]
    extension_registry: Arc<ExtensionRegistry>,
    sdk_context: Arc<SdkContext>,
}

impl ImCoreSdk {
    /// Creates a new SDK instance with user-provided storage implementations
    ///
    /// Initializes all internal components including event bus,
    /// command/query handlers, and facades.
    ///
    /// ## Storage Implementation
    ///
    /// The SDK does not provide default storage implementations.
    /// Users must implement the storage traits themselves:
    ///
    /// - `EventStore`: For event sourcing (domain events)
    /// - `MessageRepository`: For message storage and queries
    /// - `ConversationRepository`: For conversation storage and queries
    ///
    /// See `domain::repository` module for trait definitions.
    ///
    /// # Arguments
    ///
    /// * `config` - SDK configuration
    /// * `event_store` - User-implemented event store
    /// * `message_repository` - User-implemented message repository
    /// * `conversation_repository` - User-implemented conversation repository
    ///
    /// # Returns
    ///
    /// Returns a new [`ImCoreSdk`] instance on success.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Configuration validation fails
    /// - Component initialization fails
    ///
    /// # Example
    ///
    /// ```no_run
    /// use flare_im_core_sdk::interface::facade::ImCoreSdk;
    /// use flare_im_core_sdk::config::SdkConfig;
    /// use flare_im_core_sdk::domain::repository::{EventStore, MessageRepository, ConversationRepository};
    /// use std::sync::Arc;
    ///
    /// // User implements storage
    /// struct MyEventStore { /* ... */ }
    /// struct MyMessageRepository { /* ... */ }
    /// struct MyConversationRepository { /* ... */ }
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let config = SdkConfig::default();
    /// let event_store = Arc::new(MyEventStore::new().await?) as Arc<dyn EventStore>;
    /// let message_repository = Arc::new(MyMessageRepository::new().await?) as Arc<dyn MessageRepository>;
    /// let conversation_repository = Arc::new(MyConversationRepository::new().await?) as Arc<dyn ConversationRepository>;
    /// let sdk = ImCoreSdk::new(config, event_store, message_repository, conversation_repository).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(
        config: SdkConfig,
        event_store: Arc<dyn EventStore>,
        message_repository: Arc<dyn MessageRepository>,
        conversation_repository: Arc<dyn ConversationRepository>,
    ) -> anyhow::Result<Self> {
        use crate::infrastructure::storage::media_cache::MediaCacheManager;
        
        config.validate()
            .map_err(|e| anyhow::anyhow!("配置验证失败: {}", e))?;
        
        // 创建媒体缓存管理器（从配置读取缓存路径和大小）
        let media_cache = Arc::new(
            MediaCacheManager::from_config(
                config.media.cache_path.clone(),
                config.media.max_cache_size_mb,
            )?
        );
        
        let device_id = uuid::Uuid::new_v4().to_string();
        let session = Session::new(device_id);
        let connection = Connection::new();
        let sync = Sync::new();
        
        let fsm = Arc::new(FsmManager::new(
            session,
            connection,
            sync,
            event_store.clone(),
        ));
        
        let _flare_core_config = config.to_flare_core_config()?;
        
        let event_bus = Arc::new(EventBus::new(1000));
        let extension_registry = Arc::new(ExtensionRegistry::new());
        let message_queue = Arc::new(MessageQueue::new());
        
        // 创建共享的网络客户端和消息发送器
        let network = Arc::new(Mutex::new(None));
        let message_sender = Arc::new(MessageSender::new(network.clone()));
        
        // 创建同步处理器
        let conversation_sync_handler = Arc::new(ConversationSyncHandler::new(
            conversation_repository.clone(),
            event_bus.clone(),
        ));
        
        let sync_handler = Arc::new(SyncHandler::new(
            message_queue.clone(),
            event_bus.clone(),
        ));
        
        let command_handler = Arc::new(CommandHandler::new(
            fsm.clone(),
            event_store.clone(),
            message_repository.clone(),
            conversation_repository.clone(),
            config.clone(),
            media_cache.clone(),
            event_bus.clone(),
            extension_registry.clone(),
            Some(message_queue.clone()),
            network.clone(),
            message_sender.clone(),
        )?);
        
        let _converter_registry = Arc::new(crate::infrastructure::converter::ConverterRegistry::new());
        let query_handler = Arc::new(QueryHandler::new(
            message_repository.clone(),
            conversation_repository.clone(),
            fsm.clone(),
        ));
        
        let sync_coordinator = Arc::new(
            SyncCoordinator::new(
                fsm.clone(),
                message_sender.clone() as Arc<dyn crate::application::ports::sync_transport::SyncTransport>,
                conversation_sync_handler.clone(),
                sync_handler.clone(),
                event_bus.clone(),
            )
            .with_extension_registry(extension_registry.clone())
        );
        
        let sdk_context = Arc::new(SdkContext::new(
            command_handler.clone(),
            query_handler.clone(),
            event_bus.clone(),
            event_store.clone(),
            message_repository.clone(),
            conversation_repository.clone(),
            sync_coordinator.clone(),
            fsm.clone(),
        ));
        
        let converter_registry = Arc::new(crate::infrastructure::converter::ConverterRegistry::new());
        
        let message_facade = MessageFacade::new(
            fsm.clone(),
            command_handler.clone(),
            query_handler.clone(),
            message_repository.clone(),
            conversation_repository.clone(),
            media_cache,
            converter_registry.clone(),
        );
        
        let conversation_facade = ConversationFacade::new(
            command_handler.clone(),
            query_handler.clone(),
        );
        
        let queue_handler = Arc::new(DefaultMessageHandler::new(
            message_repository.clone(),
            conversation_repository.clone(),
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
    
    /// Returns a reference to the message facade
    ///
    /// The message facade provides APIs for creating, sending, and managing messages.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ImCoreSdk;
    /// # async fn example(sdk: &ImCoreSdk) -> anyhow::Result<()> {
    /// let message_facade = sdk.message();
    /// // Use message APIs...
    /// # Ok(())
    /// # }
    /// ```
    pub fn message(&self) -> &MessageFacade {
        &self.message_facade
    }
    
    /// Returns a reference to the conversation facade
    ///
    /// The conversation facade provides APIs for managing conversations.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ImCoreSdk;
    /// # async fn example(sdk: &ImCoreSdk) -> anyhow::Result<()> {
    /// let conversation_facade = sdk.conversation();
    /// let conversations = conversation_facade.get_all_conversation_list().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn conversation(&self) -> &ConversationFacade {
        &self.conversation_facade
    }
    
    /// Authenticates the user and establishes a session
    ///
    /// # Arguments
    ///
    /// * `user_id` - The user ID
    /// * `token` - The authentication token (JWT)
    ///
    /// # Errors
    ///
    /// Returns an error if authentication fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ImCoreSdk;
    /// # async fn example(sdk: &ImCoreSdk) -> anyhow::Result<()> {
    /// sdk.login("user_id".to_string(), "jwt_token".to_string()).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn login(&self, user_id: String, token: String) -> anyhow::Result<()> {
        self.command_handler.login_direct(user_id, token).await
    }
    
    /// Logs out the current user and clears the session
    ///
    /// # Errors
    ///
    /// Returns an error if logout fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ImCoreSdk;
    /// # async fn example(sdk: &ImCoreSdk) -> anyhow::Result<()> {
    /// sdk.logout().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn logout(&self) -> anyhow::Result<()> {
        self.command_handler.logout_direct().await
    }
    
    /// Establishes a network connection to the server
    ///
    /// Must be called after [`login`](Self::login) succeeds.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ImCoreSdk;
    /// # async fn example(sdk: &ImCoreSdk) -> anyhow::Result<()> {
    /// sdk.login("user_id".to_string(), "token".to_string()).await?;
    /// sdk.connect().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect(&self) -> anyhow::Result<()> {
        self.command_handler.connect_direct().await
    }
    
    /// Performs bootstrap synchronization (full sync)
    ///
    /// This method synchronizes all conversations, messages, and related data
    /// from the server. It should be called after a successful connection.
    ///
    /// # Errors
    ///
    /// Returns an error if synchronization fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ImCoreSdk;
    /// # async fn example(sdk: &ImCoreSdk) -> anyhow::Result<()> {
    /// sdk.login("user_id".to_string(), "token".to_string()).await?;
    /// sdk.connect().await?;
    /// sdk.bootstrap_sync().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn bootstrap_sync(&self) -> anyhow::Result<()> {
        self.sync_coordinator.execute_bootstrap_sync().await
    }
    
    /// Performs asynchronous synchronization for a specific sync type
    ///
    /// # Arguments
    ///
    /// * `sync_type` - The type of sync to perform (e.g., "friend_status", "group_info")
    ///
    /// # Errors
    ///
    /// Returns an error if synchronization fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ImCoreSdk;
    /// # async fn example(sdk: &ImCoreSdk) -> anyhow::Result<()> {
    /// sdk.async_sync("friend_status".to_string()).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn async_sync(&self, sync_type: String) -> anyhow::Result<()> {
        self.sync_coordinator.execute_async_sync(sync_type).await
    }
    
    /// Performs asynchronous synchronization for all registered extensions
    ///
    /// # Errors
    ///
    /// Returns an error if any extension sync fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ImCoreSdk;
    /// # async fn example(sdk: &ImCoreSdk) -> anyhow::Result<()> {
    /// sdk.sync_all_extensions().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn sync_all_extensions(&self) -> anyhow::Result<()> {
        self.sync_coordinator.execute_all_extension_async_sync().await
    }
    
    /// Returns a reference to the message queue
    ///
    /// The message queue is used to receive incoming messages from the server.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ImCoreSdk;
    /// # async fn example(sdk: &ImCoreSdk) -> anyhow::Result<()> {
    /// let queue = sdk.message_queue();
    /// // Use queue to receive messages...
    /// # Ok(())
    /// # }
    /// ```
    pub fn message_queue(&self) -> &Arc<MessageQueue> {
        &self.message_queue
    }
    
    /// Returns message sending metrics
    ///
    /// Provides statistics about message sending including total count,
    /// success count, failure count, and average latency.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ImCoreSdk;
    /// # async fn example(sdk: &ImCoreSdk) -> anyhow::Result<()> {
    /// let metrics = sdk.get_message_metrics().await;
    /// println!("Total messages sent: {}", metrics.total_sent);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_message_metrics(&self) -> crate::infrastructure::metrics::MessageMetricsSnapshot {
        crate::infrastructure::metrics::get_metrics_snapshot().await
    }
    
    /// Registers an extension
    ///
    /// Extensions allow you to extend SDK functionality with custom business logic.
    ///
    /// # Arguments
    ///
    /// * `extension` - The extension to register
    ///
    /// # Errors
    ///
    /// Returns an error if registration fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flare_im_core_sdk::interface::facade::ImCoreSdk;
    /// # use flare_im_core_sdk::application::extension::SdkExtension;
    /// # use std::sync::Arc;
    /// # async fn example(sdk: &ImCoreSdk, extension: Arc<dyn SdkExtension>) -> anyhow::Result<()> {
    /// sdk.register_extension(extension).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn register_extension(
        &self,
        extension: Arc<dyn SdkExtension>,
    ) -> anyhow::Result<()> {
        self.extension_registry.register_extension(extension.clone()).await?;
        // 注意：这里需要创建一个临时的可变上下文
        let mut ctx = SdkContext::new(
            self.command_handler.clone(),
            self.query_handler.clone(),
            self.sdk_context.event_bus.clone(),
            self.sdk_context.event_store.clone(),
            self.sdk_context.message_repository.clone(),
            self.sdk_context.conversation_repository.clone(),
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
